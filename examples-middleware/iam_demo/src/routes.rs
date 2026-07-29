//! 最小路由：把本章的账号体系暴露成四个端点。安全边界一目了然——
//! - `/auth/login`、`/auth/login/totp`：公开（登录本身是换凭证的入口）；
//! - `/accounts`、`/grants`、`/me/permissions`：JWT 保护（handler 参数里的 AuthUser）。
//!
//! 登录做成两段式：密码过后，若账号开了 2FA，只发一枚「半程 token」
//! （mfa_pending=true，短寿命），客户端必须再带它去 /auth/login/totp 换正式 token。
//! 半程 token 进不了任何业务路由（AuthUser 提取器会拒），第二步没过就寸步难行。

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::auth::{self, AuthUser, Claims};
use crate::error::{AppError, AppResult};
use crate::model::{Grant, GrantSpec};
use crate::store;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/login/totp", post(login_totp))
        .route("/accounts", post(create_account))
        .route("/grants", post(add_grant))
        .route("/me/permissions", get(my_permissions))
}

// ---------- 登录（两段式）----------

#[derive(Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Serialize)]
struct LoginResponse {
    /// true 表示还需第二步；此时 token 是半程 token，去 /auth/login/totp 换正式的。
    mfa_required: bool,
    token: String,
}

/// POST /auth/login {email,password}
/// → 200 {mfa_required:false, token}（没开 2FA，直接给正式 token）
/// → 200 {mfa_required:true,  token}（开了 2FA，给半程 token）
/// → 401（账号不存在 / 密码错——两者返回同一句话，不给攻击者区分账号是否存在的线索）
async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> AppResult<Json<LoginResponse>> {
    let account = store::find_account_by_email(&state.pool, &req.email)
        .await?
        .filter(|a| auth::verify_password(&a.password_hash, &req.password))
        .ok_or_else(|| AppError::unauthorized("invalid email or password"))?;

    let claims = if account.totp_enabled {
        Claims::mfa_pending(account.id, account.org_id, account.role.clone())
    } else {
        Claims::full(account.id, account.org_id, account.role.clone())
    };
    let token = auth::sign_token(&state.config.jwt_secret, &claims)?;
    Ok(Json(LoginResponse {
        mfa_required: account.totp_enabled,
        token,
    }))
}

#[derive(Deserialize)]
struct TotpRequest {
    /// 上一步拿到的半程 token。
    token: String,
    /// Authenticator 上的 6 位码。
    code: String,
}

/// POST /auth/login/totp {token(半程), code} → 200 {mfa_required:false, token(正式)} / 401。
async fn login_totp(
    State(state): State<AppState>,
    Json(req): Json<TotpRequest>,
) -> AppResult<Json<LoginResponse>> {
    // 半程 token 必须验签通过、且确实是 mfa_pending 态。
    let claims = auth::decode_token(&state.config.jwt_secret, &req.token)
        .map_err(|_| AppError::unauthorized("invalid or expired token"))?;
    if !claims.mfa_pending {
        return Err(AppError::bad_request("token is not an MFA-pending token"));
    }

    let now = unix_now();
    let ok = store::verify_login_totp(&state.pool, claims.sub, &req.code, now).await?;
    if !ok {
        return Err(AppError::unauthorized("invalid TOTP code"));
    }

    // 第二步过关，签发正式 token。
    let full = Claims::full(claims.sub, claims.org_id, claims.role);
    let token = auth::sign_token(&state.config.jwt_secret, &full)?;
    Ok(Json(LoginResponse {
        mfa_required: false,
        token,
    }))
}

// ---------- 子账号 ----------

#[derive(Deserialize)]
struct CreateAccountRequest {
    email: String,
    password: String,
    role: String,
    /// 要授予新子账号的权限；每一条都必须落在「我」（父账号）的授权范围内。
    grants: Vec<GrantSpec>,
}

#[derive(Serialize)]
struct CreateAccountResponse {
    account_id: i64,
}

/// POST /accounts —— 以当前登录账号为父，建子账号并授权。
/// 越权授予（超过父账号）→ store 返回 Forbidden → 403。
async fn create_account(
    user: AuthUser,
    State(state): State<AppState>,
    Json(req): Json<CreateAccountRequest>,
) -> AppResult<Json<CreateAccountResponse>> {
    let account_id = store::create_sub_account(
        &state.pool,
        user.org_id, // 🔴 org_id 取自 token，不信任请求体
        user.account_id,
        &req.email,
        &req.password,
        &req.role,
        &req.grants,
    )
    .await?;
    Ok(Json(CreateAccountResponse { account_id }))
}

// ---------- 授权 ----------

#[derive(Deserialize)]
struct GrantRequest {
    account_id: i64,
    resource_type: String,
    resource_id: String,
    action: String,
}

#[derive(Serialize)]
struct GrantResponse {
    grant_id: i64,
}

/// POST /grants —— 给本组织内某账号追加一条授权，仍受「不超过其父账号」约束。
async fn add_grant(
    _user: AuthUser,
    State(state): State<AppState>,
    Json(req): Json<GrantRequest>,
) -> AppResult<Json<GrantResponse>> {
    let spec = GrantSpec::new(req.resource_type, req.resource_id, req.action);
    let grant_id = store::grant(&state.pool, _user.org_id, req.account_id, &spec).await?;
    Ok(Json(GrantResponse { grant_id }))
}

// ---------- 自省 ----------

#[derive(Serialize)]
struct PermissionsResponse {
    account_id: i64,
    grants: Vec<Grant>,
}

/// GET /me/permissions —— 当前账号自身声明的授权清单（自省，方便前端渲染权限）。
async fn my_permissions(
    user: AuthUser,
    State(state): State<AppState>,
) -> AppResult<Json<PermissionsResponse>> {
    let grants = store::load_grants(&state.pool, user.account_id).await?;
    Ok(Json(PermissionsResponse {
        account_id: user.account_id,
        grants,
    }))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs()
}
