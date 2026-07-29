//! 二进制入口（`cargo run -p webterm_demo`）：读配置、连 Postgres、组装 app、监听端口。
//!
//! 连不上库会直接报错退出（错误链里含 `docker compose up -d postgres` 提示）。
//! 启动后浏览器打开提示的地址，用演示令牌连上 Web 终端敲命令，再看 /sessions 里的审计。

use std::net::SocketAddr;

use webterm_demo::config::Config;
use webterm_demo::{app, build_state};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info,webterm_demo=debug")),
        )
        .init();

    let config = Config::from_env();
    let addr = config.bind_addr();
    let demo_token = config.demo_token.clone();
    let program = config.pty.program.clone();

    // Postgres 连不上就在这里报错退出。
    let state = build_state(config).await?;
    let app = app(state);

    let listener = tokio::net::TcpListener::bind(&addr).await?;

    tracing::warn!("⚠️  安全警告：这是一个 Web shell（可在服务器上执行任意命令）。");
    tracing::warn!("⚠️  本示例的鉴权/授权/命令限制都是教学级简化，切勿直接暴露到公网或用于生产。");
    tracing::info!("webterm_demo listening on http://{addr}");
    tracing::info!("浏览器打开 http://{addr}，用演示令牌连接终端（PTY 将 spawn：{program}）");
    tracing::info!("演示令牌：{demo_token}（可用 WEBTERM_TOKEN 覆盖）");
    tracing::info!("审计：GET /sessions 列会话，GET /sessions/{{id}}/replay 取回放");

    // ConnectInfo 需要用 into_make_service_with_connect_info 启动，
    // 这样 ws.rs 里才能拿到客户端 IP 写进审计（「从哪连的」）。
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    tracing::info!("webterm_demo shut down gracefully");
    Ok(())
}

/// 同时监听 Ctrl-C 与 SIGTERM，收到即进入优雅停机（见第 16 章 / todo_api_saas）。
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("install Ctrl-C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("received Ctrl-C, shutting down"),
        _ = terminate => tracing::info!("received SIGTERM, shutting down"),
    }
}
