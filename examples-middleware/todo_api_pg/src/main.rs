//! 二进制入口：读配置、连库、组装 app、监听端口，与 SQLite 版一致。
//!
//! 差异点：SQLite 版连的是本地文件，几乎不会失败；Postgres 是外部服务，
//! 「库没起」是最常见的启动失败。db::connect 的错误信息里已带上
//! `docker compose up -d postgres` 的提示，这里用 `?` 冒泡后 anyhow 会完整打印。

use todo_api_pg::config::Config;
use todo_api_pg::{app, db, AppState};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info,todo_api_pg=info")),
        )
        .init();

    let config = Config::from_env();
    // 连不上 Postgres 时这里直接报错退出（错误链里含 docker compose 提示）。
    let pool = db::connect(&config.database_url).await?;
    let state = AppState { pool };
    let app = app(state);

    let addr = config.bind_addr();
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("todo_api_pg listening on http://{addr}");
    // 优雅停机（见第 16 章）：收到 Ctrl-C / SIGTERM 后停止接收新连接，
    // 等在途请求处理完再退出。k8s 滚动更新时先发 SIGTERM 再等 grace period，
    // 没有这一步，更新窗口内的请求会被硬切断。
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    tracing::info!("todo_api_pg shut down gracefully");
    Ok(())
}

/// 同时监听 Ctrl-C（本地开发）与 SIGTERM（容器/k8s 停止实例时发的信号）。
/// 任一到达就返回，axum 随即进入「不收新请求、送完在途请求」的排空阶段。
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install Ctrl-C handler");
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
