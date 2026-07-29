//! 鉴权模块（第 17 章核心）：密码哈希、JWT 签发/校验、`AuthUser` 提取器。
//!
//! 数据流：POST /auth/login 验证密码 → 签发 JWT（含 user_id/tenant_id/role/exp）
//! → 客户端每个请求带 `Authorization: Bearer <token>` → `AuthUser` 提取器解出
//! Claims → handler 拿到「已认证的租户身份」。
//!
//! 与单租户后端的差异：token 里多了 tenant_id。此后所有数据访问都以它为界——
//! 它来自我们自己签名的 token（可信），而不是任何用户可控的输入（见 services/todos.rs）。

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::header;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::AppError;
use crate::AppState;

/// token 有效期：1 小时。取舍：越短越安全（被盗 token 的可用窗口小），
/// 但用户被迫频繁重登。生产常配「短 access token + 长 refresh token」两级，
/// 教学从简只用单枚短期 token。
const TOKEN_TTL_SECS: usize = 3600;

// ---------- 密码哈希 ----------

/// 为什么绝不存明文（甚至不存 MD5/SHA256）：数据库泄露是常态化风险，明文泄露
/// 等于用户在所有网站的密码一起泄露（人们复用密码）；而 SHA256 这类通用哈希
/// 算得太快，泄露后可被 GPU 每秒数十亿次地暴力碰撞。
///
/// argon2 是当前的推荐算法（2015 年密码哈希竞赛冠军，OWASP 首选）：故意做得
/// 又慢又吃内存，让暴力破解在经济上不划算。每次调用自动生成随机盐，输出串里
/// 自带算法参数和盐（`$argon2id$v=19$m=19456,t=2,p=1$盐$哈希`），校验时无需另存。
pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hash password: {e}"))?;
    Ok(hash.to_string())
}

/// 校验：从哈希串里解析出参数和盐，用同样参数把待验密码哈希一遍再比对。
/// 解析失败（库里存了坏数据）也按「不匹配」处理，不给调用方区分的机会。
pub fn verify_password(stored_hash: &str, password: &str) -> bool {
    PasswordHash::new(stored_hash)
        .map(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}

// ---------- JWT ----------

/// JWT 的载荷（Claims）。设计取舍——把 tenant_id 和 role 放进 token：
/// 好处是每个请求省一次查库（无状态鉴权，天然水平扩展）；
/// 代价是「撤销延迟」：改了用户角色/踢出用户后，已签发的 token 在过期前仍然有效。
/// 1 小时的 exp 把这个延迟窗口封在可接受范围；要即时撤销就得引入服务端状态
/// （token 黑名单、版本号查库），那就部分放弃了无状态的好处——没有免费午餐。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Claims {
    /// subject：用户 id（JWT 标准字段名）。
    pub sub: i64,
    /// 租户 id——多租户隔离的唯一可信来源（安全红线，见 services/todos.rs）。
    pub tenant_id: i64,
    /// 角色：admin / member，RBAC 用（见 routes/todos.rs 的 delete）。
    pub role: String,
    /// 过期时间（unix 秒）。jsonwebtoken 校验时自动检查，过期即拒。
    pub exp: usize,
}

impl Claims {
    pub fn new(user_id: i64, tenant_id: i64, role: String) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_secs() as usize;
        Self {
            sub: user_id,
            tenant_id,
            role,
            exp: now + TOKEN_TTL_SECS,
        }
    }
}

/// 签发：HS256 对称签名。签名保证的是**防篡改**而不是保密——JWT 的载荷只是
/// base64，任何人都能解开看，所以绝不能往里放密码、密钥等敏感信息。
pub fn sign_token(secret: &str, claims: &Claims) -> anyhow::Result<String> {
    let token = encode(
        &Header::default(), // 默认即 HS256
        claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;
    Ok(token)
}

/// 校验：验签 + 检查 exp（Validation::default() 自带，含 60 秒时钟偏差容忍）。
/// 任何失败（签名不对、过期、格式坏）都返回 Err，调用方统一转 401。
pub fn decode_token(secret: &str, token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;
    Ok(data.claims)
}

// ---------- AuthUser 提取器 ----------

/// 已认证的用户身份。handler 只要在参数里写 `user: AuthUser`，这个 handler
/// 就自动受保护——没带 token 或 token 无效的请求根本进不了函数体，直接 401。
///
/// 这就是「中间件式鉴权」在 axum 的惯用形态：传统框架把鉴权写成 middleware，
/// 在请求链上拦截并把用户信息塞进某个 context；axum 用提取器（extractor）
/// 达到同样效果，且更进一步——**类型签名即访问控制声明**：看一眼 handler 的
/// 参数列表就知道它要不要登录，忘了写就拿不到用户信息，编译器逼你显式选择。
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: i64,
    pub tenant_id: i64,
    pub role: String,
}

impl AuthUser {
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }
}

/// FromRequestParts 意味着只读请求头、不消费请求体，所以 AuthUser 可以和
/// `Json<T>` 共存于同一个 handler（Json 必须放参数列表最后，它消费 body）。
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // 1. 取 Authorization 头，格式必须是 `Bearer <token>`（RFC 6750）。
        let header_value = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError::unauthorized("missing Authorization header"))?;
        let token = header_value
            .strip_prefix("Bearer ")
            .ok_or_else(|| AppError::unauthorized("expected `Bearer <token>`"))?;

        // 2. 验签 + 查 exp。对外只说「无效或过期」，不区分具体原因——
        //    细分的错误信息只会帮助攻击者调试他伪造的 token。
        let claims = decode_token(&state.config.jwt_secret, token)
            .map_err(|_| AppError::unauthorized("invalid or expired token"))?;

        Ok(AuthUser {
            user_id: claims.sub,
            tenant_id: claims.tenant_id,
            role: claims.role,
        })
    }
}

// 离线单测：JWT 与 argon2 都是纯计算，不需要数据库/Redis。
#[cfg(test)]
mod tests {
    use super::*;

    fn now_secs() -> usize {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as usize
    }

    #[test]
    fn jwt_roundtrip() {
        let claims = Claims::new(7, 42, "admin".into());
        let token = sign_token("s3cret", &claims).unwrap();
        let decoded = decode_token("s3cret", &token).unwrap();
        assert_eq!(decoded, claims);
        // 换个密钥验签必须失败：这正是「拿不到密钥就伪造不了 token」的根据。
        assert!(decode_token("wrong-secret", &token).is_err());
    }

    #[test]
    fn jwt_expired_rejected() {
        // exp 放到 1 小时前——超出默认 60 秒的时钟偏差容忍，必须被拒。
        let claims = Claims {
            sub: 1,
            tenant_id: 1,
            role: "member".into(),
            exp: now_secs() - 3600,
        };
        let token = sign_token("s3cret", &claims).unwrap();
        assert!(decode_token("s3cret", &token).is_err());
    }

    #[test]
    fn password_hash_and_verify() {
        let hash = hash_password("password123").unwrap();
        // 输出是 PHC 格式串，自带算法与盐，绝不等于明文。
        assert!(hash.starts_with("$argon2"));
        assert!(verify_password(&hash, "password123"));
        assert!(!verify_password(&hash, "password124"));
        // 坏哈希串按「不匹配」处理，不 panic。
        assert!(!verify_password("not-a-hash", "password123"));
    }

    #[test]
    fn password_hash_salted() {
        // 同一密码两次哈希结果不同（随机盐）：撞库者无法用彩虹表批量反查。
        let h1 = hash_password("password123").unwrap();
        let h2 = hash_password("password123").unwrap();
        assert_ne!(h1, h2);
    }
}
