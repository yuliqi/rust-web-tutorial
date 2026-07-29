//! Web 终端与堡垒机示例（第 26 章配套）：PTY over WebSocket + 会话录制 + 访问审计。
//!
//! # 什么是堡垒机（跳板机 / bastion host）
//!
//! 运维安全的核心设施：**所有**对生产服务器的访问都强制经过它，由它统一做
//! 认证、授权、**全程录制、可审计**。直连服务器 = 无人知道谁做了什么；经堡垒机 =
//! 每一次访问、每一条命令都留痕可查。本章做一个最小但真实的 Web 终端，抓住堡垒机的
//! 本质能力：在浏览器里操作服务器 shell，服务端把 shell 的 [PTY](pty) 通过
//! [WebSocket](ws) 桥接到浏览器，并把整条会话[录制 + 审计](session)进 Postgres。
//!
//! # ⚠️ 安全第一：Web shell 是极其危险的能力
//!
//! 一个能在服务器上执行任意命令的网页入口，若被攻破，等于把服务器直接送人。所以这条
//! 链路上每一环都要绷紧：**鉴权**（[`auth`]，证明你是谁）、**授权**（谁能连哪台、
//! 用什么账号）、**审计与录制**（[`session`]，事后可追责）、**命令限制**（黑白名单）。
//! 本 crate 把注释里的「⚠️」留给每一处教学从简、生产必须补强的地方。
//!
//! # 本 crate 是「堡垒机骨架」，不是完整堡垒机
//!
//! 这里演示的是 **「PTY over WebSocket + 录制 + 审计」** 这个核心骨架。一台生产级
//! 堡垒机在此之上还有：
//!
//! - **SSH 协议代理**：本示例在跳板机本地开 shell 只为演示；真实堡垒机用 russh 之类
//!   在进程内做 SSH 代理，[`pty`] 里 spawn 的应是「ssh 到被授权的目标机」而非本地 shell。
//! - **按授权决定谁能连哪台**：接第 23 章的 RBAC/ABAC——不是「登录了就能连一切」，
//!   而是「这个人被授权了这台机的这个账号」才放行。
//! - **会话实时监控与阻断**：管理员能实时旁观在线会话，发现危险操作一键掐断（本示例
//!   的[广播](ws)骨架加个「监控订阅者」即可扩展，思路见第 24 章 realtime_demo）。
//! - **命令黑白名单**：在写 PTY 前拦截 `rm -rf /`、`shutdown` 等高危命令。
//! - **录像加密与防篡改存储**：审计证据必须防止事后被删改（见 [`session`] 的 ⚠️）。
//!
//! 数据流：HTTP 升级请求 → [`auth::AuthUser`] 鉴权 → 升级 WebSocket →
//! 开 [PTY](pty) + [`session::start_session`] → 双向桥接并逐段 [`session::record_event`]
//! → 断开时 [`session::end_session`]。审计查询走 `GET /sessions` 与
//! `GET /sessions/{id}/replay`。

pub mod auth;
pub mod config;
pub mod error;
pub mod pty;
pub mod session;
pub mod ws;

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::response::Html;
use axum::routing::get;
use axum::{Json, Router};

use crate::auth::AuthUser;
use crate::config::Config;
use crate::error::AppResult;
use crate::session::{SessionEvent, SessionSummary};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// 全应用共享状态：一个 Postgres 连接池（审计存这里）+ 配置（Arc 包一层免逐请求深拷贝）。
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Arc<Config>,
}

/// 演示页 HTML：编译期内嵌，免得运行时找文件。
const INDEX_HTML: &str = include_str!("../static/index.html");

/// 从配置构建状态：连 Postgres（含建表迁移）。main 与集成测试共用这一个入口。
pub async fn build_state(config: Config) -> anyhow::Result<AppState> {
    let pool = connect(&config.database_url).await?;
    Ok(AppState {
        pool,
        config: Arc::new(config),
    })
}

/// 建连接池并迁移。池参数讲解见 todo_api_pg/src/db.rs。
async fn connect(database_url: &str) -> anyhow::Result<PgPool> {
    use anyhow::Context;
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(std::time::Duration::from_secs(3))
        .connect(database_url)
        .await
        .with_context(|| {
            format!(
                "connect db failed: {database_url}\n\
                 提示：请先在 examples-middleware/ 下执行 `docker compose up -d postgres`"
            )
        })?;
    session::migrate(&pool).await?;
    Ok(pool)
}

/// 组装路由。
///
/// | 路由 | 方法 | 鉴权 | 作用 |
/// |---|---|---|---|
/// | `/` | GET | 公开 | 极简 Web 终端演示页 |
/// | `/terminal/ws` | GET | AuthUser | WebSocket 升级 → PTY 桥接（录制 + 审计） |
/// | `/sessions` | GET | AuthUser | 列出会话（访问审计视图） |
/// | `/sessions/{id}/replay` | GET | AuthUser | 取某会话的事件用于回放 |
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/terminal/ws", get(ws::terminal_ws))
        .route("/sessions", get(list_sessions))
        .route("/sessions/{id}/replay", get(replay_session))
        .with_state(state)
}

/// `GET /`：返回内嵌演示页。
async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

/// `GET /sessions`：审计视图。要求鉴权——审计日志本身是敏感数据。
async fn list_sessions(
    _user: AuthUser,
    State(state): State<AppState>,
) -> AppResult<Json<Vec<SessionSummary>>> {
    Ok(Json(session::list_sessions(&state.pool).await?))
}

/// `GET /sessions/{id}/replay`：取某会话的事件序列（按时间排序）用于回放。
async fn replay_session(
    _user: AuthUser,
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> AppResult<Json<Vec<SessionEvent>>> {
    Ok(Json(session::replay(&state.pool, id).await?))
}
