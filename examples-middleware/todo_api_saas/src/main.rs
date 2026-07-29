//! 二进制入口：读配置、连 Postgres + Redis、组装 app、监听端口。
//! 与 todo_api_pg 的差异：多连一个 Redis（限流/幂等键），启动日志里
//! 打印种子账号——克隆下来 30 秒就能开始试玩多租户。

use todo_api_saas::config::Config;
use todo_api_saas::{app, build_state};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info,todo_api_saas=info")),
        )
        .init();

    let config = Config::from_env();
    let addr = config.bind_addr();
    // Postgres 或 Redis 连不上都在这里直接报错退出（错误链里含 docker compose 提示）。
    let state = build_state(config).await?;
    let app = app(state);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("todo_api_saas listening on http://{addr}");
    // 种子账号提示：先 login 拿 token，再带 Authorization: Bearer <token> 访问 /todos。
    tracing::info!("试玩账号（密码均为 password123）：");
    tracing::info!("  admin@acme.test    租户 acme  (free)  角色 admin");
    tracing::info!("  member@acme.test   租户 acme  (free)  角色 member");
    tracing::info!("  admin@globex.test  租户 globex (pro)  角色 admin");
    tracing::info!(
        r#"  curl -s -X POST http://{addr}/auth/login -H 'content-type: application/json' -d '{{"email":"admin@acme.test","password":"password123"}}'"#
    );

    // 优雅停机（见第 16 章）：收到 Ctrl-C / SIGTERM 后停止接收新连接，
    // 等在途请求处理完再退出。k8s 滚动更新时先发 SIGTERM 再等 grace period，
    // 没有这一步，更新窗口内的请求会被硬切断。
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    tracing::info!("todo_api_saas shut down gracefully");
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
