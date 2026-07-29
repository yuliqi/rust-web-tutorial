//! 二进制入口：只做「启动」这一件事——读配置、连库、组装 app、监听端口。
//! 所有可复用逻辑都在 lib.rs 里，main 越薄，可测试的部分就越多。

use todo_api::config::Config;
use todo_api::{app, db, AppState};
use tracing_subscriber::EnvFilter;

// `#[tokio::main]` 把 async fn main 包进 tokio 运行时（见第 10 章）。
// 返回 anyhow::Result 让启动阶段的错误可以直接用 `?` 冒泡并打印出错误链。
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 初始化日志：优先读 RUST_LOG 环境变量，没有则用默认级别。
    // 应尽早调用（在任何会打日志的代码之前）——init 之前产生的日志事件会丢失。
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info,todo_api=info")),
        )
        .init();

    let config = Config::from_env();
    // 连接池在启动时建好一次，之后通过 AppState 共享给所有请求（见 lib.rs）。
    let pool = db::connect(&config.database_url).await?;
    let state = AppState { pool };
    let app = app(state);

    let addr = config.bind_addr();
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("todo_api listening on http://{addr}");
    // axum::serve 接管 listener，进入事件循环，直到进程被终止。
    axum::serve(listener, app).await?;
    Ok(())
}
