//! 健康检查端点：给负载均衡器/监控探活用，也是最小的 handler 示例。
//! 它不碰数据库、不需要 State——handler 的参数按需声明，axum 只注入你要的。

use axum::routing::get;
use axum::Router;
use serde_json::{json, Value};

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/health", get(health))
}

// 返回 Json<Value> 而不是裸字符串：让响应自动带上 application/json 头。
async fn health() -> axum::Json<Value> {
    axum::Json(json!({ "status": "ok" }))
}
