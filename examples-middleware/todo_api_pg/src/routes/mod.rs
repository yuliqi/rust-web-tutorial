//! 路由汇总层：把各功能模块的子路由 merge 成一棵树，供 lib.rs 挂载。
//! HTTP 层不感知数据库方言，本模块及子模块对比 SQLite 版几乎零改动。

mod health;
mod todos;

use crate::AppState;
use axum::Router;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(health::router())
        .merge(todos::router())
}
