//! 第 23 章配套示例库入口：2FA + 层级子账号 + 资源级授权。
//!
//! 从第 17 章 todo_api_saas 的扁平「租户 + admin/member」升级到真实 SaaS 的账号体系：
//! 1. 两步验证（totp.rs）：TOTP + 一次性恢复码，密码之外再加一道「你拥有的东西」；
//! 2. 层级子账号（model.rs / store.rs）：组织下账号成树，子账号权限不超父账号；
//! 3. 资源级授权（authz.rs / grants 表）：从粗粒度 role 细化到「只能访问某个云账号」。
//!
//! 复用的前章机制：argon2 密码哈希 + JWT 无状态鉴权 + AuthUser 提取器（第 17 章 auth.rs）、
//! pg_advisory_xact_lock 幂等迁移 + 独立 schema（第 17 章 db.rs）。
//!
//! 组装放 lib、集成测试（tests/iam.rs）才能直接拿 `app()` 在内存中发请求。

pub mod auth;
pub mod authz;
pub mod config;
pub mod error;
pub mod model;
pub mod routes;
pub mod store;
pub mod totp;

use axum::Router;
use config::Config;
use sqlx::PgPool;
use std::sync::Arc;
use tower_http::trace::TraceLayer;

/// 全应用共享状态。比第 17 章精简：本章聚焦账号与授权，不涉及 Redis 限流，
/// 所以只有 Postgres 连接池 + 配置（AuthUser 提取器要拿 jwt_secret 验签）。
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Arc<Config>,
}

pub async fn build_state(config: Config) -> anyhow::Result<AppState> {
    let pool = store::connect(&config.database_url).await?;
    Ok(AppState {
        pool,
        config: Arc::new(config),
    })
}

pub fn app(state: AppState) -> Router {
    Router::new()
        .merge(routes::router())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
