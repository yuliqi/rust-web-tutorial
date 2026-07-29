//! 统一错误处理。用 thiserror 定义错误枚举，再实现 `IntoResponse` 映射到状态码。
//!
//! 401（没带/带错令牌）与 500（内部错误）是本示例的主要分类。终端相关的错误
//! （PTY 打开失败等）归 Internal：细节只进日志，不向客户端外泄。

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl AppError {
    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::Unauthorized(msg.into())
    }
}

/// services 里对 sqlx 调用写 `?` 时由编译器自动插入这里的转换。
impl From<sqlx::Error> for AppError {
    fn from(value: sqlx::Error) -> Self {
        Self::Internal(value.into())
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            // 对外只说「无效或过期」，不区分具体原因——细分只会帮攻击者调试伪造的凭证。
            AppError::Unauthorized(_) => {
                (StatusCode::UNAUTHORIZED, "invalid or missing token".to_string())
            }
            AppError::Internal(err) => {
                tracing::error!(error = %err, "internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".to_string(),
                )
            }
        };
        (status, Json(ErrorBody { error: message })).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
