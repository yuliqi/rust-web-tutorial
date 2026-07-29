//! 健康检查端点。生产上要区分两种探针（见第 16 章）：
//! - /health（liveness，存活）：进程活着就返回 200，**不碰依赖**——
//!   如果把数据库故障算进去，k8s 会不停重启一个本来没病的进程。
//! - /health/ready（readiness，就绪）：真探一次数据库（SELECT 1）。
//!   没就绪时返回 503，负载均衡/k8s 就不往这个实例发流量，但也不重启它。
//!   数据库恢复后探针转绿，流量自动回来。

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/health/ready", get(ready))
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// 就绪探针：依赖可用才算 ready。探针要快、要便宜（SELECT 1 即可），
/// 它会被 k8s 每隔几秒调一次，绝不能做昂贵查询。
async fn ready(State(state): State<AppState>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    match sqlx::query("SELECT 1").execute(&state.pool).await {
        Ok(_) => Ok(Json(json!({ "status": "ready" }))),
        Err(e) => {
            // 探针失败要打日志（运维排查靠它），但对外只说「没就绪」，不泄内部细节。
            tracing::warn!("readiness check failed: {e}");
            Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "status": "unavailable" })),
            ))
        }
    }
}
