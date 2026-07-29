//! SSE 实时推送（第 24 章配套）：把每 2 秒一次的采集[快照](crate::Snapshot)推给浏览器仪表盘。
//!
//! 为什么监控大盘选 SSE 而不是 WebSocket：数据流是**纯单向**的（服务端不停往下推、
//! 浏览器只看），SSE 就是一条 `text/event-stream` 的长 HTTP 响应，实现最简、还自带断线重连
//! （浏览器 `EventSource` 自动重连）。需要客户端频繁上行才轮到 WebSocket。realtime_demo/src/sse.rs
//! 对二者的取舍有更完整的对照表，这里沿用同一套写法。
//!
//! 与 realtime_demo 的两点不同：
//! 1. 推的是整份 [`Snapshot`]（一批指标 + 当刻告警），前端一次拿到就能重绘整个面板；
//! 2. 连接建立时**先补发一帧当前快照**——否则新开的页面要干等最多 2 秒才有数据。

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use futures_util::Stream;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::{MonitorState, Snapshot};

/// `GET /stream`：SSE 事件流。先补发当前快照，再持续转发后台任务广播的每一帧。
pub async fn stream_handler(
    State(st): State<MonitorState>,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    // 先订阅、再取当前快照：顺序很重要——先拿到接收端，两次动作之间万一有新帧也不会漏，
    // 顶多和补发的初始帧重复一次（前端按 ts_ms 幂等重绘，重复无害）。
    let rx = st.subscribe();
    let initial = st.latest().await;

    // 初始帧：让刚连上的页面立刻有内容，不必干等下一个采集周期。
    let init_stream = tokio_stream::once(snapshot_event(&initial));

    // 实时帧：BroadcastStream 把 broadcast::Receiver 适配成 Stream。
    let live_stream = BroadcastStream::new(rx).map(|res| match res {
        Ok(snap) => snapshot_event(&snap),
        // 慢消费者落后被覆盖：如实告知前端丢了帧，别假装无事（监控掉几帧旧数据可接受）。
        Err(_lagged) => SseEvent::default()
            .event("lagged")
            .data(r#"{"lagged":true}"#),
    });

    // 先初始帧、后实时帧；整条流永不产生错误项，用 Infallible 表达。
    let stream = init_stream.chain(live_stream).map(Ok::<_, Infallible>);

    // KeepAlive：每 15 秒发一个注释行心跳，防止空闲连接被代理/NAT 静默切断。
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}

/// 把一份快照包成 SSE 帧：`event: snapshot` + JSON 数据体。
/// 前端用 `addEventListener("snapshot", ...)` 精准接收。
fn snapshot_event(snap: &Snapshot) -> SseEvent {
    let json = serde_json::to_string(snap).unwrap_or_else(|_| "{}".into());
    SseEvent::default().event("snapshot").data(json)
}
