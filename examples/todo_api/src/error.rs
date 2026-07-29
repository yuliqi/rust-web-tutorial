//! 统一错误处理：全应用只有一种错误类型 `AppError`，它同时扮演两个角色——
//! 1. 业务层的 Result 错误（services 返回它）；
//! 2. HTTP 响应（实现 `IntoResponse` 后 axum 能直接把它变成带状态码的 JSON）。
//!
//! 配合 `From` 实现，「底层错误 → AppError」的转换发生在 services 层写 `?` 的那一刻；
//! handler 里的 `?` 只是把 AppError 原样向上传，最后由 `IntoResponse` 变成 HTTP 响应。
//! 整条链上错误处理不再散落各处。错误处理详见第 4 章，trait 详见第 6 章。

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

/// 按「客户端该看到什么」分类，而不是按错误来源分类：
/// BadRequest/NotFound 携带可展示的消息；Internal 只留给日志，不外泄细节。
#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    NotFound(String),
    Internal(anyhow::Error),
}

/// 错误响应体固定为 `{"error": "..."}`，客户端只需处理一种格式。
#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

impl AppError {
    // `impl Into<String>` 让调用方既能传 &str 也能传 String，省去到处 .to_string()。
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }
}

/// 关键一环：有了这个 From，services 里对 sqlx 调用写 `?` 时，
/// 编译器会自动插入这里的转换（`?` 的糖就是 `return Err(From::from(e))`）。
/// RowNotFound 单独映射成 404，其余数据库错误一律视为 500。
impl From<sqlx::Error> for AppError {
    fn from(value: sqlx::Error) -> Self {
        match value {
            sqlx::Error::RowNotFound => Self::NotFound("resource not found".into()),
            other => Self::Internal(other.into()),
        }
    }
}

/// 兜底转换：任何 anyhow::Error（如 db::migrate 的返回）也能用 `?` 变成 AppError。
impl From<anyhow::Error> for AppError {
    fn from(value: anyhow::Error) -> Self {
        Self::Internal(value)
    }
}

/// axum 的约定：handler 的返回值（包括 Result 的 Err 分支）只要实现 IntoResponse
/// 就能作为 HTTP 响应。因此 handler 可以直接返回 `AppResult<T>`——
/// Ok 走正常序列化，Err 走这里，框架替我们完成分发。
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            AppError::Internal(err) => {
                // 内部错误：细节进日志、给客户端的消息固定化，避免泄露实现信息。
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

/// 全项目统一的 Result 别名：handler、service 的签名因此保持简短一致。
pub type AppResult<T> = Result<T, AppError>;
