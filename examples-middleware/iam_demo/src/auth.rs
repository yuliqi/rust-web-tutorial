//! 鉴权：密码哈希（argon2）+ JWT 签发/校验 + AuthUser 提取器。
//! 直接沿用第 17 章 todo_api_saas/auth.rs 的机制，本章只加了一处 2FA 相关的改造——
//! Claims 里的 `mfa_pending` 标志，用来表达「密码过了、但第二步还没过」的中间态。

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::FromRequestParts;
use axum::http::header;
use axum::http::request::Parts;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::AppError;
use crate::AppState;

const TOKEN_TTL_SECS: usize = 3600;
/// 「半程」token 的寿命故意很短：它只够用户从密码步走到 TOTP 步，几分钟足矣。
const MFA_PENDING_TTL_SECS: usize = 300;

// ---------- 密码哈希（同第 17 章）----------

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hash password: {e}"))?;
    Ok(hash.to_string())
}

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

/// JWT 载荷。相比第 17 章多了一个 `mfa_pending`：
/// - false（默认）：完整凭证，能访问受保护路由；
/// - true：半程凭证——密码已验、但账号开了 2FA 且第二步尚未完成，只能用来换正式 token。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Claims {
    /// subject：账号 id。
    pub sub: i64,
    /// 组织 id——多租户隔离的唯一可信来源。
    pub org_id: i64,
    pub role: String,
    pub exp: usize,
    /// 是否处于「等待第二步验证」的中间态。序列化时默认字段，兼容老 token。
    #[serde(default)]
    pub mfa_pending: bool,
}

impl Claims {
    /// 完整凭证（两步都过 / 或账号没开 2FA）。
    pub fn full(account_id: i64, org_id: i64, role: String) -> Self {
        Self::build(account_id, org_id, role, false, TOKEN_TTL_SECS)
    }

    /// 半程凭证（密码过了、等 TOTP）。
    pub fn mfa_pending(account_id: i64, org_id: i64, role: String) -> Self {
        Self::build(account_id, org_id, role, true, MFA_PENDING_TTL_SECS)
    }

    fn build(account_id: i64, org_id: i64, role: String, mfa_pending: bool, ttl: usize) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_secs() as usize;
        Self {
            sub: account_id,
            org_id,
            role,
            exp: now + ttl,
            mfa_pending,
        }
    }
}

pub fn sign_token(secret: &str, claims: &Claims) -> anyhow::Result<String> {
    let token = encode(
        &Header::default(),
        claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;
    Ok(token)
}

pub fn decode_token(secret: &str, token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;
    Ok(data.claims)
}

// ---------- AuthUser 提取器 ----------

/// 已完成全部认证步骤的账号身份。handler 参数里写 `user: AuthUser` 即受保护。
/// 关键：**半程 token（mfa_pending=true）被这里挡在门外**——它只能去换正式 token，
/// 不能拿来访问任何业务路由。
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub account_id: i64,
    pub org_id: i64,
    pub role: String,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header_value = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError::unauthorized("missing Authorization header"))?;
        let token = header_value
            .strip_prefix("Bearer ")
            .ok_or_else(|| AppError::unauthorized("expected `Bearer <token>`"))?;

        let claims = decode_token(&state.config.jwt_secret, token)
            .map_err(|_| AppError::unauthorized("invalid or expired token"))?;

        // 半程凭证不算「已认证」：还差第二步。
        if claims.mfa_pending {
            return Err(AppError::unauthorized("MFA not completed"));
        }

        Ok(AuthUser {
            account_id: claims.sub,
            org_id: claims.org_id,
            role: claims.role,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jwt_roundtrip_and_wrong_secret() {
        let claims = Claims::full(7, 42, "owner".into());
        let token = sign_token("s3cret", &claims).unwrap();
        assert_eq!(decode_token("s3cret", &token).unwrap(), claims);
        assert!(decode_token("wrong", &token).is_err());
    }

    #[test]
    fn mfa_pending_flag_carried() {
        let half = Claims::mfa_pending(1, 1, "member".into());
        assert!(half.mfa_pending);
        let full = Claims::full(1, 1, "member".into());
        assert!(!full.mfa_pending);
    }

    #[test]
    fn password_hash_and_verify() {
        let hash = hash_password("password123").unwrap();
        assert!(hash.starts_with("$argon2"));
        assert!(verify_password(&hash, "password123"));
        assert!(!verify_password(&hash, "wrong"));
        assert!(!verify_password("not-a-hash", "password123"));
    }
}
