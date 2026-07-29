//! HTTP-01 挑战：向 CA 证明「这个域名确实归你控制」。
//!
//! CA（如 Let's Encrypt）凭什么相信申请证书的人真的拥有 `example.com`？答案是让你完成一次
//! **挑战（challenge）**。ACME 定义了三种挑战，各有适用场景：
//!
//! - **HTTP-01**（本模块实现）：CA 会来访问 `http://你的域名/.well-known/acme-challenge/{token}`，
//!   期待拿到 `{token}.{key_authorization}` 这个字符串。能在该域名的 80 端口放出正确内容，
//!   就证明你控制这个域名。最常用，但**要求 80 端口公网可达**，且**不能给通配符域名签发**。
//! - **DNS-01**：在域名的 DNS 里加一条 `_acme-challenge` TXT 记录。唯一能签**通配符证书**
//!   （`*.example.com`）的方式，也适合服务器不对外开 80 端口的场景；代价是要有 DNS 服务商 API。
//! - **TLS-ALPN-01**：在 443 端口用一个特殊的 ALPN 协议应答。适合只想开 443、由负载均衡器
//!   统一处理的场景。
//!
//! 本模块把 HTTP-01「可离线测」的那部分做成可运行代码：一个内存里的 token 表，加一个 axum 路由。
//! 真实的 CA 来访问是需要公网域名的（那部分见 [`crate::acme`] 与 README），但「token 存进去、
//! 路由能原样取出来」这套逻辑完全可以在本地断网验证。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;

/// HTTP-01 挑战响应的固定 URL 前缀（ACME 协议规定，不能改）。
pub const CHALLENGE_PREFIX: &str = "/.well-known/acme-challenge/";

/// 内存版挑战应答表：`token -> key_authorization`。
///
/// 下单流程（[`crate::acme`]）在完成挑战前，把 CA 给的 token 和算出来的 key_authorization
/// 存进这里；CA 随后来 GET 对应 token 时，路由就能查表应答。`Clone` 即共享同一份数据
/// （内部 `Arc<Mutex<...>>`），所以能直接塞进 axum 的 state。
///
/// 生产上要给每个 token 加**过期清理**（挑战完成后就没用了），这里为教学从简。
#[derive(Clone, Default)]
pub struct ChallengeStore {
    inner: Arc<Mutex<HashMap<String, String>>>,
}

impl ChallengeStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 存入一条挑战应答。
    pub fn insert(&self, token: impl Into<String>, key_authorization: impl Into<String>) {
        self.inner
            .lock()
            .expect("ChallengeStore 锁中毒")
            .insert(token.into(), key_authorization.into());
    }

    /// 取出某个 token 对应的应答内容（CA 探测时用）。
    pub fn get(&self, token: &str) -> Option<String> {
        self.inner
            .lock()
            .expect("ChallengeStore 锁中毒")
            .get(token)
            .cloned()
    }

    /// 挑战完成后移除，避免 token 长期驻留。
    pub fn remove(&self, token: &str) {
        self.inner.lock().expect("ChallengeStore 锁中毒").remove(token);
    }

    /// 当前待应答的挑战数量。
    pub fn len(&self) -> usize {
        self.inner.lock().expect("ChallengeStore 锁中毒").len()
    }

    /// 是否没有任何待应答挑战。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 独立的挑战路由（state 就是 [`ChallengeStore`] 本身）。
///
/// 单独暴露这个函数，方便只针对 HTTP-01 应答做集成测试（见 `tests/challenge.rs`）；
/// 组合进整体应用时另有走法（见 [`crate::app`]，通过 `FromRef` 共享同一个 store）。
pub fn router(store: ChallengeStore) -> Router {
    Router::new()
        .route(
            "/.well-known/acme-challenge/{token}",
            get(serve_challenge),
        )
        .with_state(store)
}

/// `GET /.well-known/acme-challenge/{token}`：查表返回对应的 key_authorization。
///
/// 命中就返回纯文本内容（就是 CA 期待的 `{token}.{thumbprint}`），未命中返回 404。
pub async fn serve_challenge(
    Path(token): Path<String>,
    State(store): State<ChallengeStore>,
) -> impl IntoResponse {
    match store.get(&token) {
        Some(value) => (StatusCode::OK, value),
        None => (StatusCode::NOT_FOUND, "unknown challenge token".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_insert_get_remove() {
        let store = ChallengeStore::new();
        assert!(store.is_empty());

        store.insert("tok123", "tok123.thumbprint");
        assert_eq!(store.len(), 1);
        assert_eq!(store.get("tok123").as_deref(), Some("tok123.thumbprint"));
        assert!(store.get("missing").is_none());

        store.remove("tok123");
        assert!(store.is_empty());
    }

    #[test]
    fn store_clone_shares_state() {
        let a = ChallengeStore::new();
        let b = a.clone();
        a.insert("t", "t.k");
        // b 是 a 的克隆，应看到同一份数据。
        assert_eq!(b.get("t").as_deref(), Some("t.k"));
    }
}
