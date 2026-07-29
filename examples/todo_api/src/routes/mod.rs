//! 路由汇总层：把各功能模块的子路由 merge 成一棵树，供 lib.rs 挂载。
//! 新增一组接口时只需在这里多 merge 一行，lib.rs 完全不用动。
//!
//! `Router<AppState>` 的类型参数表示「还欠一个 AppState」——
//! 直到 lib.rs 调 `with_state` 补上，它才变成可服务的 `Router`。

mod health;
mod todos;

use crate::AppState;
use axum::Router;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(health::router())
        .merge(todos::router())
}
