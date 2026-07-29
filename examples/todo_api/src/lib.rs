//! Todo REST API 库入口：负责「组装」整个应用。
//!
//! 数据流自上而下：HTTP 请求 → routes(解析参数) → services(业务逻辑) → db(SQLite)。
//! 把组装逻辑放在 lib 而不是 main，是为了让集成测试(tests/api.rs)
//! 能直接调用 `app()` 拿到 Router，在内存中发请求，完全不需要真的监听端口。

pub mod config;
pub mod db;
pub mod error;
pub mod models;
pub mod routes;
pub mod services;

use axum::Router;
use sqlx::SqlitePool;
use tower_http::trace::TraceLayer;

/// 全应用共享的状态。axum 要求它实现 `Clone`：
/// 每个请求 handler 拿到的都是一份 clone。
/// `SqlitePool` 内部是 `Arc` 包着的连接池，clone 只是引用计数 +1，
/// 所有 clone 共享同一批连接——这就是「一个 Pool、处处使用」的原理。
#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
}

/// 把路由、中间件、状态拼成完整的 Router。
/// `with_state` 注入后，handler 里就能用 `State(state)` 提取器取回它（见 routes/todos.rs）。
/// `TraceLayer` 给每个请求自动打日志，属于横切关注点，放中间件层而不是散落在各 handler。
/// 顺序有讲究：`.layer()` 只作用于在它**之前**注册的路由，所以 merge 在前、layer 在后。
pub fn app(state: AppState) -> Router {
    Router::new()
        .merge(routes::router())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
