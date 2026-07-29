//! 统一错误处理：在 todo_api_pg 的基础上扩出 SaaS 场景需要的三种状态——
//! 401 Unauthorized（没登录/token 无效）、403 Forbidden（登录了但没权限：
//! RBAC 拒绝、套餐配额超限）、429 Too Many Requests（限流）。
//!
//! 401 与 403 的区别值得记牢：401 是「你是谁我不知道」（缺少或无效的凭证），
//! 403 是「我知道你是谁，但你不能做这件事」。而跨租户访问我们返回 404 而不是
//! 403——见 services/todos.rs 的「安全红线」注释。

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

/// 按「客户端该看到什么」分类：前五种携带可展示的消息，
/// Internal 只进日志、不外泄细节（数据库连接串、SQL 片段都可能藏在错误链里）。
#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
    TooManyRequests(String),
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

    pub fn too_many_requests(msg: impl Into<String>) -> Self {
        Self::TooManyRequests(msg.into())
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

/// Redis 出错（连不上、命令失败）对客户端是 500：限流/幂等是服务端内部机制，
/// 细节不外泄。生产上还要考虑「Redis 挂了限流怎么办」——放行（fail-open）
/// 还是拒绝（fail-close），取决于限流是保护自己还是计费手段，这里从简走 500。
impl From<redis::RedisError> for AppError {
    fn from(value: redis::RedisError) -> Self {
        Self::Internal(value.into())
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
            AppError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            AppError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg.clone()),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            AppError::TooManyRequests(msg) => (StatusCode::TOO_MANY_REQUESTS, msg.clone()),
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
