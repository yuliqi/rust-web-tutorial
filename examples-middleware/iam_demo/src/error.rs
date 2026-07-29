//! 统一错误处理，沿用第 17 章 todo_api_saas 的分类与语义：
//! 401（没登录 / 凭证无效）、403（登录了但没权限：授权判定拒绝）、
//! 400（参数不合法，如子账号索要超过父账号的权限）、404、500。
//!
//! 401 vs 403 值得记牢：401 是「你是谁我不知道」，403 是「我知道你是谁，但你不能」。
//! 本章 authz 判定失败走 403——身份是清楚的，只是这条资源不在你的授权内。

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
    Internal(anyhow::Error),
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

impl AppError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }
    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::Unauthorized(msg.into())
    }
    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::Forbidden(msg.into())
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }
}

/// store 里对 sqlx 调用写 `?` 时由编译器自动插入这里的转换。
impl From<sqlx::Error> for AppError {
    fn from(value: sqlx::Error) -> Self {
        match value {
            sqlx::Error::RowNotFound => Self::NotFound("resource not found".into()),
            other => Self::Internal(other.into()),
        }
    }
}

impl From<anyhow::Error> for AppError {
    fn from(value: anyhow::Error) -> Self {
        Self::Internal(value)
    }
}

/// store 的业务拒绝映射成 403，系统故障映射成 500——「谁的错」在状态码上就分清。
impl From<crate::store::StoreError> for AppError {
    fn from(value: crate::store::StoreError) -> Self {
        match value {
            crate::store::StoreError::Forbidden(msg) => Self::Forbidden(msg),
            crate::store::StoreError::Other(err) => Self::Internal(err),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            AppError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg.clone()),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            AppError::Internal(err) => {
                tracing::error!(error = %err, "internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal server error".into())
            }
        };
        (status, Json(ErrorBody { error: message })).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
