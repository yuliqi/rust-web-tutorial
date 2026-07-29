//! 登录端点：整棵路由树里唯二不要求 JWT 的业务入口（另一个是 webhook）。

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};

use crate::error::AppResult;
use crate::models::{LoginRequest, LoginResponse};
use crate::services::auth as auth_service;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/auth/login", post(login))
}

/// POST /auth/login {email, password} → 200 {token} / 401。
/// 注：登录端点没有挂租户限流（限流键按租户，登录前还不知道租户）。
/// 生产上它反而是最需要限流的入口（撞库攻击），常按 IP + email 双维度限，
/// 留作练习。
async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> AppResult<Json<LoginResponse>> {
    let token = auth_service::login(
        &state.pool,
        &state.config.jwt_secret,
        &payload.email,
        &payload.password,
    )
    .await?;
    Ok(Json(LoginResponse { token }))
}
