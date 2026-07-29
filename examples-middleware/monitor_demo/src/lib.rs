//! 实时监控与告警（第 28 章配套）。
//!
//! 面板 / SaaS 都需要「看到服务器和业务的实时状态，异常时告警」。本 crate 把这条链路走通：
//!
//! ```text
//!            ┌─────────────┐  每 2 秒   ┌──────────────┐
//!  sysinfo → │ collect_*   │ ────tick──▶│ 后台采集任务  │
//!            └─────────────┘            └──────┬───────┘
//!                                              │  一帧 Snapshot（指标 + 告警）
//!                        ┌─────────────────────┼───────────────────────┐
//!                        ▼                     ▼                        ▼
//!                 更新共享「最新快照」   broadcast 给 SSE 订阅者      evaluate 告警
//!                  (/metrics /alerts)      (/stream 推浏览器)        (阈值规则)
//! ```
//!
//! 四个模块各司其职、耦合极低：
//! - [`metrics`]：采集 + Prometheus 文本渲染（渲染是**纯函数**，可穷尽单测）；
//! - [`alert`]：阈值判定（**纯函数**，重点可测）；
//! - [`store`]：时序落库（依赖 Postgres，做成 `#[ignore]` 集成测试）；
//! - [`sse`]：把快照实时推给浏览器仪表盘。
//!
//! ## 与其他章节的串联
//!
//! - **采集调度**：这里用一个 `tokio::time::interval` 每 2 秒采一次，最朴素直接；若要「按 cron
//!   表达式、可配置、多任务统一编排」，换成第 21 章的定时任务框架（tokio-cron-scheduler）即可，
//!   采集函数本身不用动。
//! - **告警投递**：本示例只把告警**判定**出来（[`alert::evaluate`]）并在 `/alerts` 暴露、
//!   在页面上显示。生产要把告警**发出去**（邮件 / 短信 / 电话 / IM），正确姿势是把告警事件投进
//!   第 14 章的消息队列，由独立的通知服务异步消费——采集/判定的主链路绝不能被「发通知」这种
//!   慢 IO 阻塞。

pub mod alert;
pub mod metrics;
pub mod sse;
pub mod store;

use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::header;
use axum::response::{Html, IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, RwLock};

use crate::alert::{evaluate, Alert, Op, Rule};
use crate::metrics::{now_millis, render_prometheus, Collector, Metric};

/// 演示页 HTML：编译期内嵌，免得运行时还要找文件路径。
const INDEX_HTML: &str = include_str!("../static/index.html");

/// broadcast 环形缓冲容量：能缓存多少条「还没被所有订阅者读走」的快照。
/// 快照每 2 秒才一帧、频率很低，64 远够；慢消费者落后会收到 Lagged（见 [`sse`]）。
const CHANNEL_CAPACITY: usize = 64;

/// 采集间隔：每 2 秒一帧。对系统级大盘足够实时，又给了 sysinfo 一个稳定的 CPU 采样窗口
/// （CPU 使用率是「两次刷新求差」，这 2 秒天然就是采样区间，见 [`metrics::Collector`]）。
pub const COLLECT_INTERVAL: Duration = Duration::from_secs(2);

/// 一次采集的**快照**：这一刻的全部指标 + 由这批指标判定出的告警 + 采集时刻。
///
/// 呼应「消息即契约」：SSE 推给前端的、`/alerts` 返回的，都是这同一份 schema，
/// 前端只认这一个结构就能重绘整个面板。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Snapshot {
    /// 本次采集到的全部指标。
    pub metrics: Vec<Metric>,
    /// 基于本批指标、按当前规则判定出的告警（可能为空）。
    pub alerts: Vec<Alert>,
    /// 采集时刻（Unix 毫秒）。取自本批第一条样本的 `ts_ms`，同一批内一致。
    pub ts_ms: i64,
}

impl Snapshot {
    /// 空快照：服务刚起、还没采到第一帧时的初值。
    fn empty() -> Self {
        Self {
            metrics: Vec::new(),
            alerts: Vec::new(),
            ts_ms: 0,
        }
    }
}

/// 应用共享状态（注入 axum `State`，`Clone` 即共享同一份底层数据）。
///
/// 三块共享数据：
/// - `tx`：广播发送端，后台任务每采一帧就 `send` 给所有 SSE 订阅者；
/// - `latest`：最新一帧快照，供 `/metrics`、`/alerts` 这类「拉」的接口即时读取
///   （新连接也靠它补发初始帧）。用 `RwLock` 因为「读多写少」——每 2 秒写一次，接口随时读；
/// - `rules`：告警规则，`Arc` 只读共享（生产可换成可热更新的结构）。
#[derive(Clone)]
pub struct MonitorState {
    tx: broadcast::Sender<Snapshot>,
    latest: Arc<RwLock<Snapshot>>,
    rules: Arc<Vec<Rule>>,
}

impl MonitorState {
    /// 用给定告警规则新建状态。
    pub fn new(rules: Vec<Rule>) -> Self {
        let (tx, _rx) = broadcast::channel(CHANNEL_CAPACITY);
        Self {
            tx,
            latest: Arc::new(RwLock::new(Snapshot::empty())),
            rules: Arc::new(rules),
        }
    }

    /// 订阅快照流（给 SSE 用）。订阅之后发出的每一帧都会进它的缓冲。
    pub fn subscribe(&self) -> broadcast::Receiver<Snapshot> {
        self.tx.subscribe()
    }

    /// 读取当前最新快照的副本（给 `/metrics`、`/alerts` 及 SSE 初始帧用）。
    pub async fn latest(&self) -> Snapshot {
        self.latest.read().await.clone()
    }
}

/// 默认告警规则：CPU / 内存过高、磁盘将满。教学用几条直观的阈值；生产规则通常来自配置中心/数据库。
pub fn default_rules() -> Vec<Rule> {
    vec![
        Rule::new("cpu_usage_percent", Op::Gt, 90.0, "critical"),
        Rule::new("memory_used_percent", Op::Gt, 90.0, "critical"),
        Rule::new("disk_used_percent", Op::Ge, 85.0, "warning"),
    ]
}

/// 组装路由。
///
/// | 路由 | 方法 | 作用 |
/// |---|---|---|
/// | `/` | GET | 仪表盘页（原生 JS，`EventSource` 连 `/stream` 实时显示） |
/// | `/metrics` | GET | Prometheus 文本格式，供 Prometheus server 抓取 |
/// | `/stream` | GET | SSE，实时推送每帧快照（指标 + 告警）的 JSON |
/// | `/alerts` | GET | 当前告警（JSON），当刻快照里的告警列表 |
pub fn app(state: MonitorState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/metrics", get(metrics_handler))
        .route("/stream", get(sse::stream_handler))
        .route("/alerts", get(alerts_handler))
        .with_state(state)
}

/// `GET /`：返回内嵌的仪表盘页。
async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

/// `GET /metrics`：把最新快照的指标渲染成 Prometheus 文本。
///
/// Content-Type 用 Prometheus 约定的 `text/plain; version=0.0.4`——抓取端据此确认这是
/// exposition 格式。渲染逻辑在 [`metrics::render_prometheus`]（纯函数、已单测）。
async fn metrics_handler(State(st): State<MonitorState>) -> impl IntoResponse {
    let snap = st.latest().await;
    let body = render_prometheus(&snap.metrics);
    (
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        body,
    )
}

/// `GET /alerts`：返回当刻快照里的告警列表（JSON）。
async fn alerts_handler(State(st): State<MonitorState>) -> Json<Vec<Alert>> {
    Json(st.latest().await.alerts)
}

/// 启动后台采集任务：每 [`COLLECT_INTERVAL`] 采一次 → 更新最新快照 → 广播给 SSE → 判定告警。
///
/// 返回 `JoinHandle`，调用方可持有（`main` 里让它随进程存活即可）。
///
/// 采集本身是几次内存/系统调用级的刷新，微秒到毫秒级返回，直接在异步任务里做不会明显阻塞
/// worker；若将来要采「很重」的指标（逐进程扫描等），应挪进 `tokio::task::spawn_blocking`。
pub fn spawn_collector(state: MonitorState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut collector = Collector::new();
        // interval 的第一拍立即触发；此刻距 Collector::new 的首次刷新很近，CPU 使用率可能接近 0，
        // 属正常（见 metrics 模块说明），下一拍起就是真实的 2 秒窗口值。
        let mut ticker = tokio::time::interval(COLLECT_INTERVAL);
        loop {
            ticker.tick().await;
            let metrics = collector.collect();
            let alerts = evaluate(&state.rules, &metrics);
            let ts_ms = metrics.first().map(|m| m.ts_ms).unwrap_or_else(now_millis);
            let snap = Snapshot {
                metrics,
                alerts,
                ts_ms,
            };
            // 先更新「最新快照」（拉接口与新连接的初始帧靠它），再广播给在线的 SSE 订阅者。
            *state.latest.write().await = snap.clone();
            // send 只在「没有任何订阅者」时返回 Err，这不是错误——没人看就没人看，忽略即可。
            let _ = state.tx.send(snap);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 默认规则可用：数量、指标名与严重级别符合预期。
    #[test]
    fn default_rules_are_sane() {
        let rules = default_rules();
        assert!(!rules.is_empty());
        assert!(rules.iter().any(|r| r.metric == "cpu_usage_percent"));
        assert!(rules.iter().any(|r| r.severity == "critical"));
    }

    /// Snapshot 的 JSON round-trip：SSE 与前端靠这份 schema 通信，必须稳定。
    #[test]
    fn snapshot_json_round_trip() {
        let snap = Snapshot {
            metrics: vec![Metric::now("cpu_usage_percent", 12.5, vec![])],
            alerts: evaluate(
                &[Rule::new("cpu_usage_percent", Op::Gt, 10.0, "warning")],
                &[Metric::now("cpu_usage_percent", 12.5, vec![])],
            ),
            ts_ms: 1_700_000_000_000,
        };
        let s = serde_json::to_string(&snap).unwrap();
        let back: Snapshot = serde_json::from_str(&s).unwrap();
        assert_eq!(snap, back);
        assert_eq!(back.alerts.len(), 1, "12.5 > 10 应产出一条告警");
    }
}
