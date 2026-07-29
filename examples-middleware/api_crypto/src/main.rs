//! 二进制入口：装载教学密钥、组装 app、监听端口。
//! 本示例不依赖任何外部服务（数据库/Redis 都不需要），`cargo run -p api_crypto`
//! 后浏览器打开 http://localhost:3004 即可体验完整加密链路。

use api_crypto::{app, AppState};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info,api_crypto=info")),
        )
        .init();

    // 教学固定密钥（见 keys/ 下 PEM 文件头的警告）。
    // 生产：私钥从 KMS/环境变量注入，这里应换成读 env 或密钥管理客户端。
    let state = AppState::from_dev_keys();
    let app = app(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "3004".to_string());
    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("api_crypto listening on http://localhost:{port}");
    tracing::info!("打开浏览器访问首页，配合 DevTools Network 面板观察密文信封");
    axum::serve(listener, app).await?;
    Ok(())
}
