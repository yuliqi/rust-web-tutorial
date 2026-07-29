//! 广播中心（第 24 章配套，纯逻辑、可离线单测）。
//!
//! ## 为什么用 `broadcast` 而不是 `mpsc`
//!
//! 实时推送的本质是「一条消息，投递给当前所有在线连接」——这是**一对多**。
//!
//! - [`tokio::sync::mpsc`]：多**生产者**、单**消费者**。一条消息只会被一个接收者
//!   取走，天生是「任务分发」（谁抢到谁干），不适合广播。
//! - [`tokio::sync::broadcast`]：单/多生产者、**多消费者**，每个订阅者都能收到
//!   *自己订阅之后* 发出的每一条消息。这正是聊天室、实时监控、实时日志想要的语义。
//!
//! ## lagging（慢消费者会丢消息）—— 必须理解的语义
//!
//! broadcast 内部是一个**固定容量的环形缓冲**。发送端不会因为某个订阅者读得慢而
//! 阻塞（否则一个卡住的客户端能拖垮所有人）；相反，当一个订阅者落后太多、老消息
//! 已被新消息覆盖时，它的 `recv()` 会返回 [`RecvError::Lagged(n)`]，告诉你
//! 「你错过了 n 条」，然后从当前最老的可用消息继续。
//!
//! 这是实时系统的一个根本取舍：**要么阻塞快的人等慢的人，要么让慢的人丢消息**。
//! broadcast 选了后者。对「实时」场景通常正确——监控面板掉几帧旧数据无所谓，
//! 保住整体不被拖垮更重要。真要「一条都不能少」，那是消息队列（第 14 章）的活，
//! 得靠持久化 + 消费确认，而不是内存广播。

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// 环形缓冲容量：能缓存多少条「还没被所有订阅者读走」的消息。
/// 偏小 → 慢消费者更易触发 Lagged 丢消息；偏大 → 占更多内存。
/// 256 对教学演示足够；生产按「最慢消费者能容忍的滞后 × 消息速率」估。
const CHANNEL_CAPACITY: usize = 256;

/// 实时协议里流动的**统一消息结构**。
///
/// 呼应第 14 / 19 章「消息即契约」：不管走 WebSocket 还是 SSE，两端都认这同一份
/// schema。有稳定的 `kind` + `payload` + `ts`，前端才能按 `kind` 分发渲染、按 `ts`
/// 排序去重。裸传字符串看似省事，实则把「这条是什么、发生在何时」的信息丢在了
/// 协议之外，双方全靠口头约定，改一处就两边崩。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    /// 事件类型，前端据此分发（如 "chat" / "metric" / "log"）。
    pub kind: String,
    /// 事件负载，用 `serde_json::Value` 承载任意结构，保持 schema 顶层稳定、
    /// 细节灵活。
    pub payload: serde_json::Value,
    /// 事件发生时间（Unix 毫秒）。让接收端能排序、去重、算延迟。
    pub ts: i64,
}

impl Event {
    /// 用「当前时间」构造一个事件。`ts` 取系统时钟的 Unix 毫秒。
    pub fn now(kind: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            kind: kind.into(),
            payload,
            ts: now_millis(),
        }
    }
}

/// 广播中心：本质就是持有一个 broadcast 发送端。
///
/// `Clone` 即共享同一个底层 channel（`Sender` clone 是引用计数），所以可以放进
/// axum 的 `State` 里，每个连接 handler 各拿一份，publish/subscribe 都作用在
/// 同一个广播域上。
#[derive(Clone)]
pub struct Hub {
    tx: broadcast::Sender<Event>,
}

impl Hub {
    /// 新建一个空的广播中心。
    pub fn new() -> Self {
        // broadcast::channel 会同时给出 tx 和一个 rx；我们只留 tx。
        // rx 在这里被丢弃没关系：只要 Hub（含 tx）还活着，channel 就不会关闭，
        // 后续 subscribe() 随时能再要新的接收端。
        let (tx, _rx) = broadcast::channel(CHANNEL_CAPACITY);
        Self { tx }
    }

    /// 订阅：拿到一个接收端，从**此刻起**发出的消息都会进它的缓冲。
    /// 订阅之前发生的历史消息收不到（broadcast 不回放历史，这点和 MQ 不同）。
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }

    /// 发布一条事件给当前所有订阅者。
    ///
    /// 返回「收到这条消息的订阅者数量」。返回 0 不是错误，只是**此刻没人在线**
    /// （所有 receiver 都已 drop）——发送端不因此报错，符合「推送尽力而为」的语义。
    pub fn publish(&self, event: Event) -> usize {
        // send 只在「一个订阅者都没有」时返回 Err，我们把它归一成 0。
        self.tx.send(event).unwrap_or(0)
    }

    /// 当前在线订阅者数量（用于演示页展示 / 健康检查）。
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

/// 当前 Unix 毫秒时间戳。抽成独立函数，既给 [`Event::now`] 用，也方便测试。
pub fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 广播语义：一条 publish，多个订阅者都能各自收到同一条。
    #[tokio::test]
    async fn broadcast_reaches_all_subscribers() {
        let hub = Hub::new();
        let mut a = hub.subscribe();
        let mut b = hub.subscribe();

        let n = hub.publish(Event::now("chat", json!({ "text": "hi" })));
        assert_eq!(n, 2, "两个订阅者都应收到");

        let ea = a.recv().await.expect("a 应收到");
        let eb = b.recv().await.expect("b 应收到");
        assert_eq!(ea, eb, "两个订阅者收到的是同一条");
        assert_eq!(ea.kind, "chat");
        assert_eq!(ea.payload["text"], "hi");
    }

    /// 没有订阅者时 publish 返回 0，且不 panic（尽力而为语义）。
    #[test]
    fn publish_without_subscribers_returns_zero() {
        let hub = Hub::new();
        assert_eq!(hub.publish(Event::now("noop", json!(null))), 0);
        assert_eq!(hub.subscriber_count(), 0);
    }

    /// Event 的 JSON round-trip：序列化再反序列化，字段一模一样。
    /// 这是「消息即契约」的最小保证——两端靠这份 schema 通信。
    #[test]
    fn event_json_round_trip() {
        let ev = Event {
            kind: "metric".into(),
            payload: json!({ "cpu": 0.42, "host": "web-1" }),
            ts: 1_700_000_000_000,
        };
        let s = serde_json::to_string(&ev).expect("序列化");
        let back: Event = serde_json::from_str(&s).expect("反序列化");
        assert_eq!(ev, back);
    }

    /// 慢消费者语义：容量填满后，落后的接收端会收到 Lagged 而非阻塞发送端。
    /// 这里显式验证「broadcast 选择丢消息而不是拖垮全场」。
    #[tokio::test]
    async fn slow_subscriber_gets_lagged() {
        // 单独用小容量 channel 直接构造，方便触发 Lagged。
        let (tx, mut rx) = broadcast::channel::<Event>(2);
        // 塞 4 条，只有容量 2，rx 一直没读 → 老的两条被覆盖。
        for i in 0..4 {
            let _ = tx.send(Event::now("x", json!(i)));
        }
        // 第一次 recv 应报 Lagged（错过了 2 条），之后能继续读到最新的。
        match rx.recv().await {
            Err(broadcast::error::RecvError::Lagged(missed)) => {
                assert_eq!(missed, 2, "应报错过 2 条");
            }
            other => panic!("预期 Lagged，实际 {other:?}"),
        }
        // 恢复后仍能读到缓冲里最老的可用消息。
        assert!(rx.recv().await.is_ok());
    }
}
