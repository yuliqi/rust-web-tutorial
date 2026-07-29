//! 统一错误处理，沿用前面章节的分类：400（参数不合法）、404（找不到应用/插件）、
//! 502（插件子进程这一「上游」出错——它像一个外部依赖，坏了不算宿主自己的 bug）、500（宿主内部故障）。
//!
//! 为什么给插件单独一档 502：插件是第三方进程，可能崩溃、返回错误、协议不对。
//! 把这类失败和宿主自身的 500 分开，运维一眼就能判断「是插件的锅还是面板的锅」。

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

use crate::plugin::PluginError;

#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    NotFound(String),
    /// 插件子进程相关的失败（启动不了 / 协议错 / 返回 error / 已退出）。
    PluginFailure(String),
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

/// 插件错误映射：能力未授权/未知能力是「调用方申请不当」，归 400；
/// 其余（启动失败、协议错、插件回 error、进程退出）都是插件这一上游的问题，归 502。
impl From<PluginError> for AppError {
    fn from(value: PluginError) -> Self {
        match value {
            PluginError::CapabilityDenied(_) | PluginError::UnknownCapability(_) => {
                Self::BadRequest(value.to_string())
            }
            other => Self::PluginFailure(other.to_string()),
        }
    }
}

impl From<anyhow::Error> for AppError {
    fn from(value: anyhow::Error) -> Self {
        Self::Internal(value)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            AppError::PluginFailure(msg) => (StatusCode::BAD_GATEWAY, msg.clone()),
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
