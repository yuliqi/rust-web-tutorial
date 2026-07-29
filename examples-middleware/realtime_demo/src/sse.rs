//! Server-Sent Events（SSE，第 24 章配套）：服务端→客户端的**单向**推送流。
//!
//! ## SSE vs WebSocket —— 怎么选
//!
//! | 维度 | SSE | WebSocket |
//! |---|---|---|
//! | 方向 | 单向（仅服务端→客户端） | 双向全双工 |
//! | 底层 | 就是一条不结束的普通 HTTP 响应（`text/event-stream`） | 独立协议，需 Upgrade 握手 |
//! | 断线重连 | 浏览器 `EventSource` **自动重连**，还能带 `Last-Event-ID` 续传 | 要自己写重连逻辑 |
//! | 代理/防火墙 | 就是 HTTP，穿透性好 | 偶有中间设备不认 Upgrade |
//! | 实现复杂度 | 低（一个流式 handler 就行） | 高（读写两半、心跳、状态机） |
//!
//! **一句话取舍**：只需要「服务端不停往下推、客户端只看」——实时日志、监控大盘、
//! 进度条、消息通知——选 SSE，简单可靠还自带重连。需要客户端也频繁上行、要交互
//! ——Web 终端、协同编辑、在线游戏——才上 WebSocket。别为了「显得实时」无脑上
//! WebSocket，多数推送场景 SSE 就够。
//!
//! ## 实现要点
//!
//! 把 [`Hub`] 的订阅（一个 broadcast 接收端）适配成一个 `Stream<Item = Event>`，
//! 交给 axum 的 [`Sse`] 包装。再挂一个 `KeepAlive`：SSE 也需要心跳——定期发注释行
//! （`: ping`）防止空闲连接被中间代理掐断，同 WebSocket 的 Ping 一个道理。

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use futures_util::Stream;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::hub::Hub;

/// `GET /sse`：把 Hub 订阅转成持续推送的事件流。
///
/// 返回类型是 `Sse<impl Stream<...>>`：axum 会自动设好 `Content-Type:
/// text/event-stream` 并保持连接常开，我们只管把 [`Event`](crate::hub::Event)
/// 一条条塞进流里。
pub async fn sse_handler(
    State(hub): State<Hub>,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    // BroadcastStream 把 broadcast::Receiver 适配成 Stream；每个元素是
    // Result<Event, BroadcastStreamRecvError>（后者即 Lagged）。
    let stream = BroadcastStream::new(hub.subscribe()).map(|res| {
        let sse = match res {
            Ok(event) => {
                // SSE 帧：把统一 Event 序列化进 data 字段；再带上 event 名（= kind）
                // 让前端能用 addEventListener(kind, ...) 精准分发。
                let json = serde_json::to_string(&event).unwrap_or_else(|_| "{}".into());
                SseEvent::default().event(event.kind).data(json)
            }
            // 慢消费者丢了 n 条：如实告诉前端，别假装无事发生。
            Err(_lagged) => SseEvent::default()
                .event("lagged")
                .data(r#"{"kind":"lagged"}"#),
        };
        // 我们的映射永不失败，用 Infallible 表达「这个流不会产生错误项」。
        Ok::<_, Infallible>(sse)
    });

    // KeepAlive：每 15 秒发一个 SSE 注释行当心跳，防止空闲连接被代理/NAT 静默切断。
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}
