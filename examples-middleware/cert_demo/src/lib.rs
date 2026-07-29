//! 第 27 章配套示例库：证书管理（ACME / SSL 自动签发与续期）。
//!
//! 面板 / SaaS 要给用户站点自动配 HTTPS，手动买证书、传证书早就过时了；主流做法是用 **ACME
//! 协议**（代表实现 Let's Encrypt）**自动签发** + **到期前自动续期**。本 crate 把这套东西
//! 拆成三块，按「离线可测的程度」组织：
//!
//! - [`cert`]：**证书生命周期**（纯离线、重点）。生成自签证书 / 私钥 / CSR，解析证书有效期，
//!   判断是否该续期。全是纯函数，穷尽单测覆盖。
//! - [`challenge`]：**HTTP-01 挑战**（可离线测的部分）。向 CA 证明「域名归你」的机制；
//!   内存 token 表 + axum 路由都能本地验证，只有「CA 真的来访问」需要公网。
//! - [`acme`]：**ACME 下单流程**（能编译，真跑要公网域名）。用 `instant-acme` 写的完整签发
//!   流程，配套 `#[ignore]` 集成测试与 README 里的「怎么真跑」说明。
//!
//! **本 crate 刻意不依赖 Postgres/Redis**：证书生命周期是纯计算，挑战应答是内存态，
//! ACME 通信直接走 HTTPS，没有需要落库的状态。这让离线单测能默认全跑、无需 docker。
//! 真实自动续期怎么和第 21 章的定时任务串起来，见 [`acme`] 模块头注释。

pub mod acme;
pub mod cert;
pub mod challenge;

use std::sync::Arc;

use axum::extract::{FromRef, State};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use crate::challenge::ChallengeStore;

/// `GET /cert/info` 返回的当前证书信息（教学用的精简视图）。
#[derive(Debug, Clone, Serialize)]
pub struct CertInfo {
    /// 证书覆盖的域名。
    pub domains: Vec<String>,
    /// 到期时间（Unix 秒），由 [`cert::parse_not_after`] 从证书里解析而来。
    pub not_after: i64,
    /// 判定用的「当前时刻」（Unix 秒）；真实服务里就是 `now`，这里显式带出便于演示。
    pub now: i64,
    /// 按 [`cert::needs_renewal`]（阈值 30 天）判断此刻是否该续期。
    pub needs_renewal: bool,
}

/// 应用共享状态：挑战表 + 当前证书信息。
#[derive(Clone)]
pub struct AppState {
    /// HTTP-01 挑战应答表，与 [`acme::order_certificate`] 用的必须是同一个。
    pub challenges: ChallengeStore,
    /// 当前证书信息，供 `/cert/info` 展示。用 `Arc` 便于将来热重载时整体替换。
    pub cert: Arc<CertInfo>,
}

// 让「只需要 ChallengeStore 的处理器」也能在 Router<AppState> 里工作：
// axum 通过 FromRef 从整体 state 里摘出子状态，于是 challenge::serve_challenge
// （它要 State<ChallengeStore>）不必改签名就能挂进主应用。
impl FromRef<AppState> for ChallengeStore {
    fn from_ref(state: &AppState) -> Self {
        state.challenges.clone()
    }
}

/// 组装整体应用。
///
/// | 路由 | 方法 | 作用 |
/// |---|---|---|
/// | `/cert/info` | GET | 返回当前证书信息（域名 / 到期时间 / 是否该续期） |
/// | `/.well-known/acme-challenge/{token}` | GET | HTTP-01 挑战应答（供 CA 探测） |
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/cert/info", get(cert_info))
        .route(
            "/.well-known/acme-challenge/{token}",
            get(challenge::serve_challenge),
        )
        .with_state(state)
}

/// `GET /cert/info`：返回当前证书信息。
async fn cert_info(State(state): State<AppState>) -> Json<CertInfo> {
    Json((*state.cert).clone())
}
