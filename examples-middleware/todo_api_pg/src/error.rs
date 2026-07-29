//! 统一错误处理：与 SQLite 版（examples/todo_api/src/error.rs）逐字同构——
//! AppError 同时扮演「业务错误」与「HTTP 响应」两个角色，`From` 让 `?` 自动完成转换。
//! 这一层完全不感知底下是哪种数据库：sqlx::Error 是跨驱动的统一错误类型，
//! 换成 Postgres 后这里一行都不用改——这正是分层的价值。

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

/// 按「客户端该看到什么」分类：BadRequest/NotFound 携带可展示的消息，
/// Internal 只进日志、不外泄细节。
#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
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

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }
}

/// services 里对 sqlx 调用写 `?` 时由编译器自动插入这里的转换。
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

/// handler 直接返回 AppResult<T>：Ok 走正常序列化，Err 走这里变成带状态码的 JSON。
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            AppError::Internal(err) => {
                tracing::error!(error = %err, "internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".into(),
                )
            }
        };

        (status, Json(ErrorBody { error: message })).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
