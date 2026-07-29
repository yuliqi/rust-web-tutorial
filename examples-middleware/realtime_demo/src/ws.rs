//! WebSocket 双向实时通道（第 24 章配套）。
//!
//! WebSocket 在一条 TCP 连接上做**全双工**：升级握手后，服务端能主动推、客户端也能
//! 随时发，互不阻塞。这正是「Web 终端 / 堡垒机」那种要交互的场景所需——用户敲的键
//! 要上行，服务端的输出要下行，同一条连接上双向流动。
//!
//! ## 一条连接的处理骨架
//!
//! 升级后把 socket `split()` 成**读半**（Stream）和**写半**（Sink），各自交给独立
//! 任务：
//! - **写任务**：订阅 [`Hub`]，把广播来的 [`Event`] 序列化后 `send` 给客户端；同时
//!   按心跳周期发 Ping。
//! - **读任务**：收客户端消息。文本消息当作一次 publish 源（回灌进 Hub，于是所有连接
//!   都能看到）；收到 Pong 就刷新「上次活跃时间」；收到 Close 就收尾。
//!
//! 两个任务用 `tokio::select!` 编排：**任一半结束（对端断开、出错、心跳超时），就取消
//! 另一半**，保证一条连接的两个任务同生共死，不留半死不活的僵尸任务。这是把第 16 章
//! 「优雅停机」的思路下沉到**连接级**。

use std::time::Duration;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast::error::RecvError;

use crate::hub::{Event, Hub};

/// 心跳间隔：多久没动静就主动发一个 Ping 探活。
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
/// 判定「假死」的超时：超过这么久没收到对端任何消息（含 Pong），就断开清理。
/// 取心跳间隔的数倍，容忍偶发网络抖动/丢包，别一次没应答就误杀。
pub const CLIENT_TIMEOUT: Duration = Duration::from_secs(45);

/// 纯逻辑：给定「距上次收到对端消息的时长」，判断该不该判定为假死。
///
/// 抽成独立函数是为了**能离线单测**——心跳超时是实时连接最容易写错、又最难在集成测试
/// 里稳定复现的一环（要精确控时）。把决策逻辑从 IO 里剥出来单独验证，是这类代码的
/// 通用手法。
///
/// ## 为什么应用层还要自己做心跳（TCP 明明有 keepalive）
///
/// TCP 的 keepalive 默认周期以**小时**计，且很多中间设备（NAT、负载均衡）会在几分钟
/// 空闲后**静默丢弃**连接，两端却都以为还连着——这就是「假死连接」。等到真正写数据
/// 才发现早断了，期间服务端一直为一个幽灵连接占着内存和文件描述符。应用层主动
/// Ping/Pong 才能**秒级**察觉对端失联，及时清理。
pub fn is_timed_out(since_last_seen: Duration) -> bool {
    since_last_seen > CLIENT_TIMEOUT
}

/// `GET /ws`：WebSocket 升级入口。axum 先完成 HTTP→WS 握手，再把裸 socket 交给
/// [`handle_socket`]。
pub async fn ws_handler(ws: WebSocketUpgrade, State(hub): State<Hub>) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, hub))
}

/// 处理单条已升级的连接。见文件头「处理骨架」。
async fn handle_socket(socket: WebSocket, hub: Hub) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = hub.subscribe();

    // 心跳计时器：每到点发一次 Ping。
    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    // 超时看门狗：每收到对端任何消息就重置。这里用「一个到期就断开」的 sleep，
    // 通过 reset 续期实现「一段时间没消息才超时」。
    let idle_deadline = tokio::time::sleep(CLIENT_TIMEOUT);
    tokio::pin!(idle_deadline);

    loop {
        tokio::select! {
            // —— 下行：广播来的事件推给客户端 ——
            recv = rx.recv() => {
                match recv {
                    Ok(event) => {
                        let text = serde_json::to_string(&event)
                            .unwrap_or_else(|_| "{}".to_string());
                        // 写失败 = 对端已断开，退出清理。
                        if sender.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    // 本连接读得太慢、丢了 n 条：不算致命，发一条提示后继续。
                    // 这是把 broadcast 的 Lagged 语义（见 hub.rs）在连接级如实告知前端。
                    Err(RecvError::Lagged(n)) => {
                        let warn = Event::now(
                            "lagged",
                            serde_json::json!({ "missed": n }),
                        );
                        let text = serde_json::to_string(&warn).unwrap_or_default();
                        if sender.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    // Hub 被销毁（正常不会，除非全程序退出）：结束。
                    Err(RecvError::Closed) => break,
                }
            }

            // —— 上行：客户端发来的消息 ——
            msg = receiver.next() => {
                // 收到任何帧都算「对端还活着」，续期看门狗。
                idle_deadline.as_mut().reset(tokio::time::Instant::now() + CLIENT_TIMEOUT);
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        // 把客户端文本当作一次 publish 源：包成 Event 回灌 Hub，
                        // 于是所有连接（含发送者自己）都会收到 —— 这就是「广播」的可见效果。
                        let payload = serde_json::from_str::<serde_json::Value>(&text)
                            .unwrap_or_else(|_| serde_json::json!({ "text": text.as_str() }));
                        hub.publish(Event::now("ws", payload));
                    }
                    // 收到 Pong：对端应答了我们的 Ping，看门狗已在上面续期，无需额外动作。
                    Some(Ok(Message::Pong(_))) => {}
                    // 有些客户端也会主动 Ping，axum 默认会自动回 Pong；这里不用管。
                    Some(Ok(Message::Ping(_))) => {}
                    Some(Ok(Message::Close(_))) => break, // 对端优雅关闭
                    Some(Ok(Message::Binary(_))) => {}    // 本示例不处理二进制
                    Some(Err(_)) => break,                // 连接出错
                    None => break,                        // 流结束 = 对端断开
                }
            }

            // —— 心跳：定时发 Ping ——
            _ = heartbeat.tick() => {
                if sender.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }

            // —— 看门狗：太久没收到任何消息，判定假死，断开 ——
            _ = &mut idle_deadline => {
                tracing::debug!("WebSocket 连接心跳超时，主动断开清理");
                break;
            }
        }
    }

    // 循环退出即连接结束。sender/receiver/rx 在此 drop：
    // 订阅端一 drop，Hub 的 receiver_count 自动减一，不泄漏。这就是连接级的优雅收尾。
    let _ = sender.close().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_decision() {
        // 刚有消息：没超时。
        assert!(!is_timed_out(Duration::from_secs(0)));
        // 一个心跳周期内：没超时。
        assert!(!is_timed_out(HEARTBEAT_INTERVAL));
        // 恰好等于超时阈值：还不算超时（严格大于才超时）。
        assert!(!is_timed_out(CLIENT_TIMEOUT));
        // 超过阈值：判超时。
        assert!(is_timed_out(CLIENT_TIMEOUT + Duration::from_millis(1)));
    }

    #[test]
    fn timeout_is_a_multiple_of_heartbeat() {
        // 设计约束：超时必须明显大于心跳间隔，否则一次丢包就误杀活连接。
        assert!(CLIENT_TIMEOUT >= HEARTBEAT_INTERVAL * 2);
    }
}
