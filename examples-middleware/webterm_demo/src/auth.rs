//! 鉴权：`AuthUser` 提取器。终端**必须**鉴权——这是本章反复强调的第一道红线。
//!
//! ## 为什么这里做得比 todo_api_saas 简单，但注释更吓人
//!
//! 结构上照抄 todo_api_saas/src/auth.rs 的 `AuthUser` 提取器思路：handler 参数里
//! 写 `user: AuthUser`，这个 handler 就自动受保护，没带有效凭证的请求根本进不了
//! 函数体。区别是本示例用一个**静态 Bearer token** 代替完整的 JWT 登录流程，只为
//! 把注意力集中在「PTY over WebSocket + 录制 + 审计」这个骨架上。
//!
//! ⚠️ 生产的堡垒机鉴权比这里重得多，缺一不可：
//! - 身份认证：JWT / 密码哈希 / MFA（第 17 章）——证明「你是谁」；
//! - 授权：谁能连哪台目标机、能用什么账号（第 23 章 RBAC/ABAC）——决定「你能做什么」；
//! - 会话审计：谁在什么时候连了什么、敲了什么（session.rs）——事后「你做了什么」。
//!
//! Web shell 能在服务器上执行任意命令，鉴权失守 = 服务器失守，怎么强调都不为过。
//!
//! ## WebSocket 鉴权的一个现实坑
//!
//! 浏览器的 `WebSocket` API **不能自定义请求头**，没法像普通 fetch 那样带
//! `Authorization: Bearer`。所以终端连接的令牌通常走 **查询参数**（`?token=`）或
//! `Sec-WebSocket-Protocol`。本提取器两种来源都认：普通 HTTP（/sessions）走
//! Authorization 头，WebSocket 升级（/terminal/ws）走查询参数。
//! （查询参数会进日志/浏览器历史，生产更推荐子协议或先换一次性 ticket。）

use axum::extract::{FromRequestParts, Query};
use axum::http::header;
use axum::http::request::Parts;
use serde::Deserialize;

use crate::error::AppError;
use crate::AppState;

/// 已认证的访问者身份。account_id 会写进会话审计（terminal_sessions.account_id），
/// 事后追责靠的就是它——「哪个账号在什么时候开了这个终端」。
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub account_id: i64,
}

#[derive(Debug, Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // 1) 优先取 Authorization: Bearer <token>（普通 HTTP 端点用）。
        let from_header = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|s| s.to_string());

        // 2) 退而取 ?token=<token>（浏览器 WebSocket 没法带自定义头，只能走这里）。
        let token = match from_header {
            Some(t) => t,
            None => Query::<TokenQuery>::from_request_parts(parts, state)
                .await
                .ok()
                .and_then(|q| q.0.token)
                .ok_or_else(|| AppError::unauthorized("missing bearer token"))?,
        };

        // 3) 常量比对。教学里是单一静态 token；生产这里是 JWT 验签 + 查库得到真实身份。
        //    注意：真实实现应做**恒定时间比较**避免时序侧信道，这里从简。
        if token != state.config.demo_token {
            return Err(AppError::unauthorized("bad token"));
        }

        // 静态 token 对应固定的演示账号 id=1；真实系统这里是 token 里解出的用户 id。
        Ok(AuthUser { account_id: 1 })
    }
}
