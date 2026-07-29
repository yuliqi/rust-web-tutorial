//! 可运行演示（`cargo run -p monitor_demo`）：起一个实时监控服务，浏览器打开后
//! 每 2 秒自动刷新本机的 CPU / 内存 / 磁盘状态，超阈值时页面上会亮出告警。
//!
//! **无需任何外部服务**：采集与推送是纯内存 + 本机指标，开箱即跑。
//! （只有时序落库 [`monitor_demo::store`] 才用到 Postgres，那是 `#[ignore]` 集成测试的范畴。）

use std::net::SocketAddr;

use anyhow::Context;
use monitor_demo::{app, default_rules, spawn_collector, MonitorState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,monitor_demo=debug".into()),
        )
        .init();

    // 端口默认 3008，可用 PORT 环境变量覆盖（与本 workspace 其他示例的约定一致）。
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3008);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    // 共享状态 + 后台采集任务：一起启动，任务随进程存活。
    let state = MonitorState::new(default_rules());
    let _collector = spawn_collector(state.clone());
    let app = app(state);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("端口 {port} 被占用？换个 PORT 再试"))?;

    tracing::info!("实时监控服务已启动：http://{addr}");
    tracing::info!("  仪表盘：      http://127.0.0.1:{port}/");
    tracing::info!("  Prometheus： http://127.0.0.1:{port}/metrics");
    tracing::info!("  实时流(SSE)： http://127.0.0.1:{port}/stream");
    tracing::info!("  当前告警：    http://127.0.0.1:{port}/alerts");

    axum::serve(listener, app)
        .await
        .context("HTTP 服务异常退出")?;

    Ok(())
}
