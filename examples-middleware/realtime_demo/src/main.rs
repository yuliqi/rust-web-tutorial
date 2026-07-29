//! 可运行演示（`cargo run -p realtime_demo`）：起一个实时服务，浏览器打开后
//! 同时建立 WebSocket 与 SSE 两条连接，任意标签页发消息，**所有标签页**都会实时
//! 收到——这就是广播中心的直观效果。
//!
//! 无需任何外部服务（不用 Postgres/Redis），开箱即跑。

use std::net::SocketAddr;

use anyhow::Context;
use realtime_demo::app;
use realtime_demo::hub::Hub;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,realtime_demo=debug".into()),
        )
        .init();

    // 端口默认 3005，可用 PORT 环境变量覆盖。
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3005);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    // 全局唯一的广播中心，注入路由。
    let hub = Hub::new();
    let app = app(hub);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("端口 {port} 被占用？换个 PORT 再试"))?;

    tracing::info!("实时服务已启动：http://{addr}");
    tracing::info!("浏览器打开 http://127.0.0.1:{port}，开两个标签页看广播效果");

    axum::serve(listener, app)
        .await
        .context("HTTP 服务异常退出")?;

    Ok(())
}
