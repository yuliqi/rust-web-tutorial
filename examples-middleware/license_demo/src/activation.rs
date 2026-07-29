//! 在线激活（online activation）。
//!
//! 与离线签名授权互补的另一条路线：客户端启动 / 定期向厂商的**激活服务器**汇报，
//! 服务器决定它能不能继续用。相比离线签名，它多了两样离线永远做不到的能力：
//! - **远程吊销**：客户退款 / 违约 / 盗版，厂商在服务器上把 key 拉黑，
//!   客户端下一次 heartbeat 就会被告知失效并应停止服务（离线签名只能等它自然过期）；
//! - **按机器计量**：一个 key 限几台机器，靠服务器记录已激活的 machine_fp 数量来卡。
//!
//! 代价：依赖网络与激活服务的可用性。所以很多商业软件是「两者结合」——
//! 离线签名保证断网也能用（保底），在线激活提供吊销与计量（增强管控）。
//!
//! 存储用内存（Mutex 包一坨 HashMap/HashSet），仅供教学；生产会换成数据库，
//! 这样多台激活服务器实例才能共享激活记录与吊销名单。

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// 激活令牌的租约时长（秒）。教学用短值方便演示续租；生产可能是几小时到几天。
pub const LEASE_SECS: i64 = 3600;

// ---------------------------------------------------------------------------
// 请求 / 响应报文
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ActivateReq {
    pub license_key: String,
    pub machine_fp: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActivateResp {
    /// 激活令牌：后续 heartbeat 凭它续租，别再传 license_key。
    pub activation_token: String,
    /// 租约到期时间（Unix 秒）。客户端应在此之前 heartbeat 续租。
    pub lease_expires: i64,
}

#[derive(Debug, Deserialize)]
pub struct HeartbeatReq {
    pub activation_token: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HeartbeatResp {
    pub lease_expires: i64,
}

// ---------------------------------------------------------------------------
// 错误
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ActivationError {
    #[error("未知的 license_key")]
    UnknownKey,
    #[error("该 key 已被吊销")]
    Revoked,
    #[error("超出该 key 的机器数配额")]
    QuotaExceeded,
    #[error("未知或已失效的激活令牌")]
    UnknownToken,
}

impl ActivationError {
    fn status(&self) -> StatusCode {
        match self {
            // 403：key 本身合法但被禁用 / 超配额，属于「不允许」。
            ActivationError::Revoked | ActivationError::QuotaExceeded => StatusCode::FORBIDDEN,
            // 401/404 语义上都行，这里用 401 表示「拿不出有效凭证」。
            ActivationError::UnknownKey | ActivationError::UnknownToken => StatusCode::UNAUTHORIZED,
        }
    }
}

impl IntoResponse for ActivationError {
    fn into_response(self) -> Response {
        let body = Json(serde_json::json!({ "error": self.to_string() }));
        (self.status(), body).into_response()
    }
}

// ---------------------------------------------------------------------------
// 内存存储（教学用）
// ---------------------------------------------------------------------------

/// 一条激活记录。
#[derive(Debug, Clone)]
struct Activation {
    license_key: String,
    lease_expires: i64,
}

#[derive(Default)]
struct Inner {
    /// 已知的合法 key -> 允许的最大机器数。生产里这来自「售卖记录」。
    key_quota: HashMap<String, usize>,
    /// 吊销名单。命中即失效——这是在线方案相对离线的核心武器。
    revoked: HashSet<String>,
    /// 激活令牌 -> 激活记录。
    tokens: HashMap<String, Activation>,
    /// key -> 已激活的机器指纹集合，用来卡配额（同一台机器重复激活不重复计数）。
    machines_of_key: HashMap<String, HashSet<String>>,
}

/// 激活服务的存储。内部用 Mutex 保护，`activate`/`heartbeat`/`revoke` 都接收
/// `now`（Unix 秒）作为参数，好让单元测试用固定时钟、handler 用真实时钟。
#[derive(Clone, Default)]
pub struct ActivationStore {
    inner: Arc<Mutex<Inner>>,
}

impl ActivationStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一个可激活的 key 及其机器数配额（模拟「卖出一份授权」）。
    pub fn register_key(&self, key: &str, max_machines: usize) {
        self.inner
            .lock()
            .unwrap()
            .key_quota
            .insert(key.to_string(), max_machines);
    }

    /// 吊销一个 key：之后它的 heartbeat 一律失败。演示厂商的远程「熔断」。
    pub fn revoke(&self, key: &str) {
        self.inner.lock().unwrap().revoked.insert(key.to_string());
    }

    /// 激活：校验 key 合法且未吊销、未超配额，然后发一个带租约的令牌。
    ///
    /// 幂等友好：同一台机器（machine_fp 已在该 key 名下）重复激活不占新配额，
    /// 只是再发一个新令牌并刷新租约——避免客户端重装 / 重启就把配额耗尽。
    pub fn activate(
        &self,
        license_key: &str,
        machine_fp: &str,
        now: i64,
    ) -> Result<ActivateResp, ActivationError> {
        let mut g = self.inner.lock().unwrap();

        if !g.key_quota.contains_key(license_key) {
            return Err(ActivationError::UnknownKey);
        }
        if g.revoked.contains(license_key) {
            return Err(ActivationError::Revoked);
        }

        let quota = g.key_quota[license_key];
        let machines = g.machines_of_key.entry(license_key.to_string()).or_default();
        let is_new_machine = !machines.contains(machine_fp);
        // 只有「新机器」才检查配额；老机器重复激活放行。
        if is_new_machine && machines.len() >= quota {
            return Err(ActivationError::QuotaExceeded);
        }
        machines.insert(machine_fp.to_string());

        let token = new_token();
        let lease_expires = now + LEASE_SECS;
        g.tokens.insert(
            token.clone(),
            Activation {
                license_key: license_key.to_string(),
                lease_expires,
            },
        );

        Ok(ActivateResp {
            activation_token: token,
            lease_expires,
        })
    }

    /// 心跳续租：令牌有效且其 key 未被吊销 -> 顺延租约；否则失败。
    ///
    /// 客户端应把「heartbeat 失败」当作停止服务的信号——这正是远程吊销生效的路径。
    pub fn heartbeat(
        &self,
        activation_token: &str,
        now: i64,
    ) -> Result<HeartbeatResp, ActivationError> {
        let mut g = self.inner.lock().unwrap();

        // 先看令牌在不在（克隆出 key，避免与后面的可变借用打架）。
        let key = match g.tokens.get(activation_token) {
            Some(a) => a.license_key.clone(),
            None => return Err(ActivationError::UnknownToken),
        };
        // key 被吊销：连令牌一起作废，返回 Revoked。
        if g.revoked.contains(&key) {
            g.tokens.remove(activation_token);
            return Err(ActivationError::Revoked);
        }

        let lease_expires = now + LEASE_SECS;
        if let Some(a) = g.tokens.get_mut(activation_token) {
            a.lease_expires = lease_expires;
        }
        Ok(HeartbeatResp { lease_expires })
    }
}

/// 随机激活令牌：16 字节随机数的 URL-safe base64。教学够用；
/// 生产可换成签名令牌（免每次查库）或更长的随机值。
fn new_token() -> String {
    let mut buf = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut buf);
    B64URL.encode(buf)
}

// ---------------------------------------------------------------------------
// 客户端逻辑（纯函数，方便单测；也对应真实客户端会做的事）
// ---------------------------------------------------------------------------

/// 客户端「激活」这一步的业务判断：从响应里取出令牌，记住租约到期时间。
/// 真实客户端这里会用 reqwest 发 HTTP，再把令牌落盘；教学里把网络那层留给
/// 集成测试的 oneshot，本函数只表达「拿到响应之后该记什么」。
pub fn client_store_token(resp: &ActivateResp) -> (String, i64) {
    (resp.activation_token.clone(), resp.lease_expires)
}

/// 客户端判断「现在还能不能用」：租约未过期即可用。
/// 注意这里是**乐观续租**——即使暂时连不上激活服务，只要还在租约期内就继续跑，
/// 避免网络抖动就让软件罢工；租约到期还没续上才停。
pub fn client_lease_valid(lease_expires: i64, now: i64) -> bool {
    now <= lease_expires
}

// ---------------------------------------------------------------------------
// axum 服务
// ---------------------------------------------------------------------------

async fn activate_handler(
    State(store): State<ActivationStore>,
    Json(req): Json<ActivateReq>,
) -> Result<Json<ActivateResp>, ActivationError> {
    let resp = store.activate(&req.license_key, &req.machine_fp, now_unix())?;
    Ok(Json(resp))
}

async fn heartbeat_handler(
    State(store): State<ActivationStore>,
    Json(req): Json<HeartbeatReq>,
) -> Result<Json<HeartbeatResp>, ActivationError> {
    let resp = store.heartbeat(&req.activation_token, now_unix())?;
    Ok(Json(resp))
}

/// 组装激活服务路由。State 是内存 ActivationStore，测试可预先 register_key / revoke。
pub fn router(store: ActivationStore) -> Router {
    Router::new()
        .route("/activate", post(activate_handler))
        .route("/heartbeat", post(heartbeat_handler))
        .with_state(store)
}

/// 当前 Unix 秒。抽成函数便于将来替换时钟；纯逻辑测试不走它（直接传 now）。
fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activate_then_heartbeat_ok() {
        let store = ActivationStore::new();
        store.register_key("KEY-1", 2);
        let now = 1_000;
        let act = store.activate("KEY-1", "m1", now).unwrap();
        assert_eq!(act.lease_expires, now + LEASE_SECS);
        let hb = store.heartbeat(&act.activation_token, now + 10).unwrap();
        assert_eq!(hb.lease_expires, now + 10 + LEASE_SECS);
    }

    #[test]
    fn unknown_key_rejected() {
        let store = ActivationStore::new();
        assert_eq!(
            store.activate("NOPE", "m1", 0),
            Err(ActivationError::UnknownKey)
        );
    }

    #[test]
    fn revoke_then_heartbeat_fails() {
        let store = ActivationStore::new();
        store.register_key("KEY-1", 2);
        let act = store.activate("KEY-1", "m1", 0).unwrap();
        // 吊销前 heartbeat 正常。
        assert!(store.heartbeat(&act.activation_token, 1).is_ok());
        // 厂商远程吊销。
        store.revoke("KEY-1");
        assert_eq!(
            store.heartbeat(&act.activation_token, 2),
            Err(ActivationError::Revoked)
        );
    }

    #[test]
    fn quota_enforced() {
        let store = ActivationStore::new();
        store.register_key("KEY-1", 2);
        assert!(store.activate("KEY-1", "m1", 0).is_ok());
        assert!(store.activate("KEY-1", "m2", 0).is_ok());
        // 第三台新机器超配额。
        assert_eq!(
            store.activate("KEY-1", "m3", 0),
            Err(ActivationError::QuotaExceeded)
        );
        // 老机器再次激活不占配额，仍可成功。
        assert!(store.activate("KEY-1", "m1", 0).is_ok());
    }

    #[test]
    fn revoked_key_cannot_activate() {
        let store = ActivationStore::new();
        store.register_key("KEY-1", 2);
        store.revoke("KEY-1");
        assert_eq!(
            store.activate("KEY-1", "m1", 0),
            Err(ActivationError::Revoked)
        );
    }

    #[test]
    fn client_lease_logic() {
        assert!(client_lease_valid(100, 100));
        assert!(client_lease_valid(100, 50));
        assert!(!client_lease_valid(100, 101));
    }
}
