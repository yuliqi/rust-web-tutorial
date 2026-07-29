//! 健康检查端点，双探针的讲解见 todo_api_pg/src/routes/health.rs（第 16 章）。
//! SaaS 版的差异：就绪探针要探**所有硬依赖**——限流和幂等键都压在 Redis 上，
//! Redis 挂了服务同样不可用，所以 ready 除了 SELECT 1 还要 PING 一次 Redis。

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

/// 存活探针：进程活着就 200，不碰依赖（理由见 todo_api_pg 版注释）。
async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// 就绪探针：Postgres 和 Redis 任一不可用都算「没就绪」——只摘流量、不重启。
async fn ready(State(state): State<AppState>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if let Err(e) = sqlx::query("SELECT 1").execute(&state.pool).await {
        tracing::warn!("readiness check failed (postgres): {e}");
        return Err(unavailable());
    }
    let mut conn = state.redis.clone();
    if let Err(e) = redis::cmd("PING").query_async::<String>(&mut conn).await {
        tracing::warn!("readiness check failed (redis): {e}");
        return Err(unavailable());
    }
    Ok(Json(json!({ "status": "ready" })))
}

fn unavailable() -> (StatusCode, Json<Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "status": "unavailable" })),
    )
}
