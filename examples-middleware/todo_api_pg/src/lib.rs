//! Todo REST API（PostgreSQL 版）库入口：负责「组装」整个应用。
//! 是 examples/todo_api（SQLite 版）的移植——分层、路由、错误处理原样保留，
//! 改动集中在 db 层与 services 的 SQL 方言上，逐处有「差异点」注释。
//!
//! 数据流不变：HTTP 请求 → routes(解析参数) → services(业务逻辑) → db(Postgres)。
//! 组装放 lib 而不是 main，集成测试(tests/api.rs)才能直接拿 `app()` 在内存中发请求。

pub mod config;
pub mod db;
pub mod error;
pub mod models;
pub mod routes;
pub mod services;

use axum::Router;
use sqlx::PgPool;
use tower_http::trace::TraceLayer;

/// 全应用共享状态。差异点仅是类型名：SqlitePool → PgPool。
/// 原理相同：Pool 内部是 Arc 包着的连接池，clone 只是引用计数 +1，
/// 所有请求 handler 共享同一批物理连接。
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
}

/// 路由、中间件、状态拼成完整 Router。
/// 顺序有讲究：`.layer()` 只作用于在它之前注册的路由，所以 merge 在前、layer 在后。
pub fn app(state: AppState) -> Router {
    Router::new()
        .merge(routes::router())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
