//! 路由汇总层：四类端点，鉴权方式各不相同，正好是一张「入口安全地图」——
//! - health：完全公开（探针没法登录）；
//! - auth：公开（登录本身就是换取凭证的入口）；
//! - todos：JWT 保护（handler 参数里的 AuthUser 提取器，见 auth.rs）；
//! - webhooks：HMAC 验签保护（机器对机器，没有用户会话，见 signing.rs）。

mod auth;
mod health;
mod todos;
mod webhooks;

use crate::AppState;
use axum::Router;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(health::router())
        .merge(auth::router())
        .merge(todos::router())
        .merge(webhooks::router())
}
