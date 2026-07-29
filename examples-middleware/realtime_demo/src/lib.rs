//! 实时通信基础示例（第 24 章配套）：WebSocket + SSE + 广播中心。
//!
//! 全书到此为止都是**请求-响应**式 HTTP：客户端问一句、服务端答一句，服务端无法
//! 主动开口。可「Web 终端的输出、实时日志、监控大盘」都要求服务端**主动往下推**，
//! 这就得靠两种技术：
//!
//! - **WebSocket**（[`ws`]）：一条 TCP 上的双向全双工通道，适合要交互的场景
//!   （后续「Web 终端 / 堡垒机」的基础）。
//! - **SSE**（[`sse`]）：基于普通 HTTP 的单向服务端推送，简单、自动重连，适合
//!   「只推不收」的日志/监控。
//!
//! 两者共享同一个[广播中心](hub)：任意来源 publish 一条 [`Event`](hub::Event)，
//! 所有在线的 WebSocket 和 SSE 连接都会同时收到。这就是「一处发生、处处可见」的
//! 实时体验的核心。
//!
//! 本 crate **刻意不依赖 Postgres/Redis**：实时推送在这里是纯内存的连接管理与广播，
//! 没有需要持久化的状态。这也让它能默认跑全部测试，无需 `docker compose`。
//! （真要「消息一条都不能丢、支持历史回放」，那是消息队列的职责，见第 14 章。）

pub mod hub;
pub mod sse;
pub mod ws;

use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::Router;
use serde::Deserialize;

use crate::hub::{Event, Hub};

/// 演示页 HTML：编译期内嵌，免得运行时还要找文件路径。
const INDEX_HTML: &str = include_str!("../static/index.html");

/// 组装应用路由。共享状态就是一个 [`Hub`]（Clone 即共享同一广播域）。
///
/// | 路由 | 方法 | 作用 |
/// |---|---|---|
/// | `/` | GET | 伺服演示页（原生 JS，同时开 WS + SSE 两条连接） |
/// | `/ws` | GET | WebSocket 升级入口（双向） |
/// | `/sse` | GET | SSE 事件流（单向服务端推送） |
/// | `/publish?msg=` | GET | 方便测试的 HTTP 触发点：发一条广播给所有连接 |
pub fn app(hub: Hub) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/ws", get(ws::ws_handler))
        .route("/sse", get(sse::sse_handler))
        .route("/publish", get(publish))
        .with_state(hub)
}

/// `GET /`：返回内嵌的演示页。
async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

/// `/publish?msg=...` 的查询参数。
#[derive(Debug, Deserialize)]
pub struct PublishQuery {
    /// 要广播的文本；缺省给个占位，方便直接点开 URL 试。
    #[serde(default = "default_msg")]
    msg: String,
}

fn default_msg() -> String {
    "hello from /publish".to_string()
}

/// `GET /publish?msg=xxx`：把一条文本包成 [`Event`] 广播出去，返回收到的连接数。
///
/// 这是个**方便测试/演示**的旁路入口——真实系统里广播源通常是内部事件
/// （定时任务、MQ 消费者、业务动作），而不是这样一个开放 GET。教学里留它，是为了
/// 用 `curl` 或浏览器地址栏就能触发一次推送，肉眼看到所有标签页同时刷新。
async fn publish(State(hub): State<Hub>, Query(q): Query<PublishQuery>) -> impl IntoResponse {
    let n = hub.publish(Event::now(
        "publish",
        serde_json::json!({ "text": q.msg }),
    ));
    format!("已广播给 {n} 个在线连接")
}
