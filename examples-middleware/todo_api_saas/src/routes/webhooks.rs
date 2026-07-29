//! 计费 webhook 端点（第 18 章）：支付商的服务器在订阅变更时回调这里。
//!
//! 这是一个**公网可达的裸端点**——没有 JWT（回调方不是登录用户），
//! 验签就是唯一防线：不验签，任何人 curl 一下就能把自己升成 pro。
//! 流程遵循支付商回调的通用模式：验签 → 幂等处理 → 快速 200（见 services/billing.rs）。

use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};
use crate::models::BillingEvent;
use crate::services::billing;
use crate::signing;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/webhooks/billing", post(billing_webhook))
}

/// POST /webhooks/billing，头 `X-Signature` = HMAC-SHA256(secret, body) 的 hex。
///
/// 注意参数用 `Bytes` 拿**原始字节**而不是 `Json<T>`：HMAC 是对字节流算的，
/// 必须先对原文验签、后解析 JSON。若先 Json 再序列化回去验签，字段顺序/空白
/// 的差异会让签名对不上——「先验原文，再解析」是 webhook 处理的铁律。
async fn billing_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> AppResult<Json<Value>> {
    let signature = headers
        .get("x-signature")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::unauthorized("missing X-Signature header"))?;

    // 恒定时间比较在 signing::verify_hex 内部完成（防时序攻击，见该文件注释）。
    if !signing::verify_hex(state.config.webhook_secret.as_bytes(), &body, signature) {
        return Err(AppError::unauthorized("invalid webhook signature"));
    }

    // 验签通过后才碰 body 的内容——顺序不能反：解析发生在信任建立之后。
    let event: BillingEvent = serde_json::from_slice(&body)
        .map_err(|e| AppError::bad_request(format!("invalid event body: {e}")))?;

    billing::handle_event(&state.pool, event).await?;
    Ok(Json(json!({ "status": "ok" })))
}
