//! 多云资产同步引擎（第 22 章综合示例）：一个 SaaS 虚拟资产管理系统的核心。
//!
//! ## 它解决什么问题
//!
//! 租户在系统里填入各家云的 API 凭证（AK/SK 或 token），系统定时用这些凭证拉取
//! 该账号下的云资产（ECS、对象存储桶、EC2、私有云 VM……），**归一化成统一模型**入库。
//! 于是「这个租户跨所有云一共有多少台机器、都在哪些区域」这类问题有了唯一答案——
//! 这就是资产管理系统的核心价值。
//!
//! ## 本示例把教程后半的哪些机制总装到了一起
//!
//! | 机制 | 模块 | 呼应章节 |
//! |---|---|---|
//! | trait 抽象多态（一朵云一个 impl，开闭原则） | [`provider`] | 第 6 章 |
//! | 多租户隔离（所有查询带 tenant_id） | [`credentials`] / [`sync`] | 第 17 章 |
//! | 计数限流（保护云厂商 API 配额） | [`ratelimit`] | 第 18 章 |
//! | 敏感字段加密存储（AK/SK 加密 + 盲索引） | [`crypto`] / [`credentials`] | 第 20 章 |
//! | 分布式锁做集群去重（同租户同步不重复跑） | [`lock`] / [`sync`] | 第 21 章 |
//!
//! ## 模块划分
//!
//! - [`provider`]：多云统一抽象 `CloudProvider` + 三个 mock 实现 + 注册表；
//! - [`credentials`]：凭证的加密存取（表 `cloudsync.credentials`）；
//! - [`crypto`]：字段加密精简版（同第 20 章，生产应抽公共 crate）；
//! - [`lock`] / [`ratelimit`]：同步任务的两道保护闸（集群去重 + 配额友好）；
//! - [`sync`]：同步引擎，把上面全部串成一条流水线，落库到 `cloudsync.assets`；
//! - `routes`（本文件内）：最小演示路由；`main.rs`：可运行的端到端演示。

pub mod credentials;
pub mod crypto;
pub mod lock;
pub mod provider;
pub mod ratelimit;
pub mod sync;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use redis::aio::ConnectionManager;
use sqlx::PgPool;

use crate::crypto::Keyring;

/// 全应用共享状态：连接池 + Redis 句柄 + 密钥环。三者都是 Clone 即共享底层资源
/// （PgPool/ConnectionManager 内部是 Arc，Keyring 只是两把 32 字节钥），
/// clone 成本极低，每个请求 handler 各拿一份互不干扰。
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub redis: ConnectionManager,
    pub keyring: Keyring,
}

/// 组装路由。演示用两个端点：触发同步、列资产。
///
/// **鉴权从简**：真实系统这两个端点必须接第 17 章的 JWT 中间件，从 token 里取出
/// 当前登录租户的 tenant_id，而**不是**像这里一样让调用方在 URL 里随便填一个——
/// 否则任何人都能同步/查看任意租户的资产，是灾难级越权。这里为聚焦同步引擎本身而从简。
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/sync/{tenant_id}", post(trigger_sync))
        .route("/assets/{tenant_id}", get(list_tenant_assets))
        .with_state(state)
}

/// POST /sync/{tenant_id}：触发一次同步，返回 [`sync::SyncReport`] 的 JSON。
async fn trigger_sync(
    State(state): State<AppState>,
    Path(tenant_id): Path<i64>,
) -> Result<Json<sync::SyncReport>, AppError> {
    // ConnectionManager clone 共享底层连接，拿一份可变句柄给需要 &mut 的 Redis 调用。
    let mut cm = state.redis.clone();
    let report = sync::sync_tenant(&state.pool, &mut cm, &state.keyring, tenant_id).await?;
    Ok(Json(report))
}

/// GET /assets/{tenant_id}：列出该租户已归一化的全部资产。
async fn list_tenant_assets(
    State(state): State<AppState>,
    Path(tenant_id): Path<i64>,
) -> Result<Json<Vec<provider::CloudAsset>>, AppError> {
    let assets = sync::list_assets(&state.pool, tenant_id).await?;
    Ok(Json(assets))
}

/// 极简错误壳：内部错误只进日志、对外统一 500，不外泄细节（同 todo_api_pg 的思路）。
pub struct AppError(anyhow::Error);

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        tracing::error!(error = %self.0, "internal error");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "internal server error" })),
        )
            .into_response()
    }
}
