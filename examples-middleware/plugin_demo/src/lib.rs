//! 第 29 章配套示例库：应用商店与插件机制。
//!
//! 面板类软件（如 1Panel）有两个横向扩展维度，本 crate 各给一个能跑、可测的最小实现：
//!
//! - [`appstore`]：**应用商店**。1Panel 的「应用」本质是一份 docker-compose 模板，
//!   安装 = 填参数 → 渲染模板 → 起容器。本模块做「渲染 + 记录」的纯逻辑（可穷尽单测），
//!   重点讲**模板注入**的防线；真正的 `docker compose up` 交给文档。
//! - [`plugin`]：**插件机制**。用「子进程 + stdio JSON-RPC」让第三方扩展面板本身。
//!   样例插件是本 crate 的第二个 bin（`src/bin/sample_plugin.rs`），集成测试能真 spawn、真通信。
//!   模块头注释详列了「子进程 / WASM / 动态库」三种方案的取舍。
//!
//! 组装（[`app`]）放 lib，集成测试才能直接拿 `app(state)` 在内存里发请求。全程无外部依赖。

pub mod appstore;
pub mod config;
pub mod error;
pub mod plugin;
pub mod routes;

use axum::Router;
use tower_http::trace::TraceLayer;

pub use routes::{AppState, PluginRegistry};

/// 组装路由 + 中间件，注入共享状态。
pub fn app(state: AppState) -> Router {
    Router::new()
        .merge(routes::router())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
