//! Todo REST API（SaaS 版）库入口：负责「组装」整个应用。
//! 是 todo_api_pg 的「SaaS 化升级」——分层不变（routes → services → db），
//! 叠加了 SaaS 与单租户后端的四大差异（第 17-18 章的主线）：
//!
//! 1. 鉴权与 RBAC（第 17 章）：登录换 JWT，`AuthUser` 提取器保护路由（auth.rs）；
//! 2. 多租户隔离（第 17 章）：所有数据访问以 token 里的 tenant_id 为界（services/todos.rs）;
//! 3. 限流 / 配额 / 幂等键（第 18 章）：Redis 承载的横切能力（limits.rs）；
//! 4. 计费 webhook（第 18 章）：HMAC 验签的公网裸端点（signing.rs、routes/webhooks.rs）。
//!
//! 数据流：HTTP 请求 → AuthUser 提取器（验 JWT，得出租户身份）→ routes（限流、
//! 解析参数）→ services（配额、业务规则，SQL 一律带 tenant_id）→ Postgres/Redis。
//! 组装放 lib 而不是 main，集成测试(tests/saas.rs)才能直接拿 `app()` 在内存中发请求。

pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod limits;
pub mod models;
pub mod routes;
pub mod services;
pub mod signing;

use anyhow::Context;
use axum::Router;
use config::Config;
use redis::aio::ConnectionManager;
use sqlx::PgPool;
use std::sync::Arc;
use tower_http::trace::TraceLayer;

/// 全应用共享状态。对比 todo_api_pg 多了两样：
/// - redis：限流计数与幂等键都存这里（ConnectionManager 内部是自动重连的
///   多路复用连接，clone 是廉价句柄复制，所有 handler 共享同一条物理连接）；
/// - config：AuthUser 提取器要拿 jwt_secret 验签、webhook 要拿 webhook_secret，
///   都从 state 里取。Arc 包一层：Config 里全是 String，逐请求深拷贝不值当。
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub redis: ConnectionManager,
    pub config: Arc<Config>,
}

/// 从配置构建完整状态：连 Postgres（含迁移+播种）、连 Redis。
/// main 和集成测试共用这一个入口，保证「测试组装的就是生产组装的」。
pub async fn build_state(config: Config) -> anyhow::Result<AppState> {
    let pool = db::connect(&config.database_url).await?;
    let client = redis::Client::open(config.redis_url.as_str())
        .with_context(|| format!("parse REDIS_URL failed: {}", config.redis_url))?;
    let redis = ConnectionManager::new(client).await.with_context(|| {
        format!(
            "connect redis failed: {}\n\
             提示：请先在 examples-middleware/ 下执行 `docker compose up -d postgres redis`",
            config.redis_url
        )
    })?;
    Ok(AppState {
        pool,
        redis,
        config: Arc::new(config),
    })
}

/// 路由、中间件、状态拼成完整 Router。
/// 顺序有讲究：`.layer()` 只作用于在它之前注册的路由，所以 merge 在前、layer 在后。
pub fn app(state: AppState) -> Router {
    Router::new()
        .merge(routes::router())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
