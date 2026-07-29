//! 加密存云凭证：租户填的各家云 AK/SK 落库前先加密，另存一列盲索引防重复添加。
//!
//! ## 安全红线（这段比代码重要）
//!
//! 云 AK/SK 泄露 = **整个云账号失守**：攻击者能开机器挖矿、删数据、拖对象存储、
//! 甚至用你的额度打别人——比第 20 章那个手机号严重得多（手机号泄露是隐私事故，
//! AK/SK 泄露是直接的资金与数据灾难）。所以存云凭证必须同时做到：
//! - **加密存**：secret 列存 AES-256-GCM 密文（本文件做的），拖库拿到的是密文；
//! - **最小权限**：让租户给的是只读子账号 AK（只需 `Describe*` 权限就能盘点资产），
//!   而不是主账号 AK——即便泄露，能造成的破坏也被 IAM 策略框住；
//! - **定期轮换**：AK 支持轮换，密文带 `v{n}:` 版本前缀就是为轮换留的口子；
//! - **绝不出网/出日志**：解密后的明文只在拉取资产那一瞬存在于内存，绝不写日志、
//!   绝不进 URL query（呼应系统级隐私红线）。
//!
//! 表设计（独立 schema `cloudsync`，与本 workspace 其他示例的表错开）：
//!
//! ```sql
//! cloudsync.credentials(
//!     id         BIGSERIAL PRIMARY KEY,
//!     tenant_id  BIGINT NOT NULL,        -- 多租户隔离键（第 17 章）
//!     provider   TEXT   NOT NULL,        -- aliyun / aws / private
//!     secret_enc TEXT   NOT NULL,        -- 加密后的 {access_key, secret_key, endpoint} JSON
//!     secret_idx TEXT   NOT NULL,        -- 盲索引 HMAC(provider:access_key)，防重复添加
//!     created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
//!     UNIQUE (tenant_id, secret_idx)     -- 同租户同一把 AK 只存一份
//! )
//! ```

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use crate::crypto::Keyring;

/// 一份云凭证——业务代码眼里的样子（明文），感知不到底层加密。
///
/// `endpoint` 只有私有云用得上（公有云 endpoint 固定），所以是 `Option`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credential {
    pub tenant_id: i64,
    pub provider: String,
    pub access_key: String,
    pub secret_key: String,
    pub endpoint: Option<String>,
}

/// secret 列里加密存的实际内容。把三个字段打包成一个 JSON 一起加密，
/// 比「每个字段一列密文」省列、也省得同一把凭证的几段密文对不上号。
#[derive(Debug, Serialize, Deserialize)]
struct SecretPayload {
    access_key: String,
    secret_key: String,
    endpoint: Option<String>,
}

/// 盲索引的明文原料：`provider:access_key`。
///
/// 为什么用它做去重键而不是整份 secret：判断「是不是同一把凭证」看 provider + AK 就够了
/// （AK 在一朵云内唯一标识一个子账号），SK 是配套的私密部分、不参与「相不相同」的判断。
/// 盲索引可逆不了，所以去重不需要解密——直接比 HMAC 值即可。
fn credential_index_input(provider: &str, access_key: &str) -> String {
    format!("{provider}:{access_key}")
}

/// 建连接池并自动迁移。参数与 todo_api_pg 一致（快速失败的 acquire_timeout 讨论见那边）。
pub async fn connect(database_url: &str) -> Result<PgPool> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(std::time::Duration::from_secs(3))
        .connect(database_url)
        .await
        .with_context(|| {
            format!(
                "connect db failed: {database_url}\n\
                 提示：请先在 examples-middleware/ 下执行 `docker compose up -d postgres redis`"
            )
        })?;
    migrate(&pool).await?;
    Ok(pool)
}

/// 幂等迁移。沿用本 workspace 的事务级咨询锁模式串行化并发建表
/// （`CREATE ... IF NOT EXISTS` 的检查与创建不是原子操作，多连接同时执行会竞态）。
/// 锁 id 46 是本 crate 自选的，与其他示例（42/44/45）错开。
pub async fn migrate(pool: &PgPool) -> Result<()> {
    let mut tx = pool.begin().await.context("begin migrate tx")?;
    sqlx::query("SELECT pg_advisory_xact_lock(46)")
        .execute(&mut *tx)
        .await
        .context("acquire migrate lock")?;
    sqlx::query("CREATE SCHEMA IF NOT EXISTS cloudsync")
        .execute(&mut *tx)
        .await
        .context("create schema cloudsync")?;
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS cloudsync.credentials (
            id         BIGSERIAL PRIMARY KEY,
            tenant_id  BIGINT NOT NULL,
            provider   TEXT   NOT NULL,
            secret_enc TEXT   NOT NULL,
            secret_idx TEXT   NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            -- 同一租户同一把 AK 只允许存一份（盲索引去重）
            UNIQUE (tenant_id, secret_idx)
        )
        "#,
    )
    .execute(&mut *tx)
    .await
    .context("create credentials table")?;
    // 归一化后的统一资产表：跨云资产混在一张表里，靠 (tenant_id, provider, external_id) 唯一。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS cloudsync.assets (
            id          BIGSERIAL PRIMARY KEY,
            tenant_id   BIGINT NOT NULL,
            provider    TEXT   NOT NULL,
            external_id TEXT   NOT NULL,
            asset_type  TEXT   NOT NULL,
            name        TEXT   NOT NULL,
            region      TEXT   NOT NULL,
            raw         JSONB  NOT NULL,
            synced_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
            -- upsert 的冲突键：同一租户、同一朵云、同一个云侧 id 视为同一份资产
            UNIQUE (tenant_id, provider, external_id)
        )
        "#,
    )
    .execute(&mut *tx)
    .await
    .context("create assets table")?;
    tx.commit().await.context("commit migrate tx")?;
    Ok(())
}

/// 加密入库一份凭证。返回是否**新插入**（false = 该租户已有这把 AK，被去重跳过）。
///
/// secret 在**进库前**完成加密与盲索引计算，SQL 里 bind 的全是密文/HMAC——
/// 明文 AK/SK 从不出现在 SQL 语句与数据库日志里。
pub async fn save_credential(pool: &PgPool, keyring: &Keyring, cred: &Credential) -> Result<bool> {
    let payload = SecretPayload {
        access_key: cred.access_key.clone(),
        secret_key: cred.secret_key.clone(),
        endpoint: cred.endpoint.clone(),
    };
    let secret_json = serde_json::to_string(&payload).context("serialize secret payload")?;
    let secret_enc = keyring.encrypt(&secret_json);
    let secret_idx = keyring.blind_index(&credential_index_input(&cred.provider, &cred.access_key));

    let res = sqlx::query(
        r#"
        INSERT INTO cloudsync.credentials (tenant_id, provider, secret_enc, secret_idx)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (tenant_id, secret_idx) DO NOTHING
        "#,
    )
    .bind(cred.tenant_id)
    .bind(&cred.provider)
    .bind(&secret_enc)
    .bind(&secret_idx)
    .execute(pool)
    .await
    .context("insert credential")?;
    // ON CONFLICT DO NOTHING 未插入时 rows_affected 为 0，用它区分「新增」与「已存在被去重」。
    Ok(res.rows_affected() == 1)
}

/// 加载并解密某租户的全部凭证。
///
/// **多租户隔离红线（第 17 章）**：`WHERE tenant_id = $1` 一个都不能少——少了这个条件，
/// 一个租户就能同步到别人的云账号，是灾难级越权。解密只发生在这里、结果只回给本租户的
/// 同步流程，明文 AK/SK 不落任何持久化介质。
pub async fn load_credentials(
    pool: &PgPool,
    keyring: &Keyring,
    tenant_id: i64,
) -> Result<Vec<Credential>> {
    let rows = sqlx::query(
        "SELECT provider, secret_enc FROM cloudsync.credentials WHERE tenant_id = $1 ORDER BY id",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await
    .context("load credentials")?;

    let mut creds = Vec::with_capacity(rows.len());
    for row in rows {
        let provider: String = row.get("provider");
        let secret_enc: &str = row.get("secret_enc");
        let secret_json = keyring
            .decrypt(secret_enc)
            .with_context(|| format!("decrypt credential of provider {provider}"))?;
        let payload: SecretPayload =
            serde_json::from_str(&secret_json).context("deserialize secret payload")?;
        creds.push(Credential {
            tenant_id,
            provider,
            access_key: payload.access_key,
            secret_key: payload.secret_key,
            endpoint: payload.endpoint,
        });
    }
    Ok(creds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{KeyRing, Keyring};
    use std::collections::HashMap;

    fn test_keyring() -> Keyring {
        let mut keys = HashMap::new();
        keys.insert(2u8, [0x22; 32]);
        Keyring::new(KeyRing::new(keys, 2).unwrap(), [0x33; 32])
    }

    #[test]
    fn secret_payload_roundtrip_through_encryption() {
        // 凭证的加解密 roundtrip：加密后是带 v2 前缀的密文，解密还原回原 JSON。
        let kr = test_keyring();
        let payload = SecretPayload {
            access_key: "LTAI_ak".to_string(),
            secret_key: "super_secret_sk".to_string(),
            endpoint: Some("https://idc.example".to_string()),
        };
        let json = serde_json::to_string(&payload).unwrap();
        let enc = kr.encrypt(&json);
        assert!(enc.starts_with("v2:"));
        // 密文里不含明文 AK/SK。
        assert!(!enc.contains("LTAI_ak"));
        assert!(!enc.contains("super_secret_sk"));
        let back: SecretPayload = serde_json::from_str(&kr.decrypt(&enc).unwrap()).unwrap();
        assert_eq!(back.access_key, "LTAI_ak");
        assert_eq!(back.secret_key, "super_secret_sk");
        assert_eq!(back.endpoint.as_deref(), Some("https://idc.example"));
    }

    #[test]
    fn index_input_distinguishes_provider_and_ak() {
        // 去重键：provider 或 AK 任一不同 ⇒ 不同凭证。
        assert_eq!(credential_index_input("aws", "AK1"), "aws:AK1");
        assert_ne!(
            credential_index_input("aws", "AK1"),
            credential_index_input("aliyun", "AK1")
        );
        assert_ne!(
            credential_index_input("aws", "AK1"),
            credential_index_input("aws", "AK2")
        );
    }
}
