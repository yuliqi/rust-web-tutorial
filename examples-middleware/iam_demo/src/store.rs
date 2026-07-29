//! 数据库层：连接池 + 迁移 + 所有对 `iam` schema 的读写。
//!
//! 与前面章节一致——表放独立 schema（`iam`），迁移用事务级咨询锁串行化
//! （本 crate 锁 id 用 47，与 todo_api_pg 的 42、todo_api_saas 的 43 错开，
//! 同一个物理库里各示例互不阻塞对方的迁移）。
//!
//! 🔴 安全红线：**一切按 org_id 隔离**。凡是「按 id 取账号 / 授权」的读写，都必须
//! 同时带上 org_id 条件，绝不能只凭一个用户可控的 id 就跨组织取数据。org_id 的
//! 唯一可信来源是 JWT（见 auth.rs），不是请求体里的任何字段。

use anyhow::{Context, Result};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::authz;
use crate::auth::hash_password;
use crate::model::{Account, Grant, GrantSpec};
use crate::totp;

/// store 写操作可能失败于两类原因，路由据此给出不同 HTTP 状态：
/// - Forbidden：授权不变量被违反（如子账号索要超过父账号的权限）→ 403；
/// - Other：数据库等基础设施错误 → 500（细节只进日志，不外泄）。
///
/// 把「业务拒绝」和「系统故障」分开，是错误处理里一条重要的边界。
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{0}")]
    Forbidden(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// 账号能授出的权限上限。owner（组织根账号）在本组织内不受限；
/// 其余账号的上限是「父账号持有的那组 grants」。
enum Authority {
    /// 组织根账号：本组织内不设上限。
    Unlimited,
    /// 受限账号：只能在这组权限的范围内再往下授。
    Limited(Vec<Grant>),
}

impl Authority {
    /// 待授的一条权限是否落在上限内。
    fn covers(&self, spec: &GrantSpec) -> bool {
        match self {
            Authority::Unlimited => true,
            // 复用 authz::can 的通配 + 默认拒绝语义：把 spec 当成一次具体请求来问。
            Authority::Limited(grants) => {
                authz::can(grants, &spec.resource_type, &spec.resource_id, &spec.action)
            }
        }
    }
}

pub async fn connect(database_url: &str) -> Result<PgPool> {
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
    migrate(&pool).await?;
    Ok(pool)
}

/// 幂等迁移：CREATE ... IF NOT EXISTS，事务级咨询锁串行化（锁 id 47）。
pub async fn migrate(pool: &PgPool) -> Result<()> {
    let mut tx = pool.begin().await.context("begin migrate tx")?;
    sqlx::query("SELECT pg_advisory_xact_lock(47)")
        .execute(&mut *tx)
        .await
        .context("acquire migrate lock")?;

    sqlx::query("CREATE SCHEMA IF NOT EXISTS iam")
        .execute(&mut *tx)
        .await
        .context("create iam schema")?;

    // 组织：隔离的最外层边界。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS iam.orgs (
            id   BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            plan TEXT NOT NULL DEFAULT 'free'
        )
        "#,
    )
    .execute(&mut *tx)
    .await
    .context("migrate orgs table")?;

    // 账号：parent_id 自引用形成层级；owner 的 parent_id 为 NULL。
    // totp_secret_enc 列名带 _enc 提醒：生产此列应加密存（呼应第 20 章）。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS iam.accounts (
            id              BIGSERIAL PRIMARY KEY,
            org_id          BIGINT NOT NULL REFERENCES iam.orgs(id),
            parent_id       BIGINT REFERENCES iam.accounts(id),
            email           TEXT NOT NULL UNIQUE,
            password_hash   TEXT NOT NULL,
            role            TEXT NOT NULL,
            totp_secret_enc TEXT,
            totp_enabled    BOOLEAN NOT NULL DEFAULT FALSE
        )
        "#,
    )
    .execute(&mut *tx)
    .await
    .context("migrate accounts table")?;

    // 授权：细粒度资源级权限。resource_id / action = '*' 表通配（见 authz.rs）。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS iam.grants (
            id            BIGSERIAL PRIMARY KEY,
            account_id    BIGINT NOT NULL REFERENCES iam.accounts(id),
            resource_type TEXT NOT NULL,
            resource_id   TEXT NOT NULL,
            action        TEXT NOT NULL
        )
        "#,
    )
    .execute(&mut *tx)
    .await
    .context("migrate grants table")?;

    // 层级/授权表的查询都以 org_id / account_id 起手，建对应索引。
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_iam_accounts_org ON iam.accounts (org_id)")
        .execute(&mut *tx)
        .await
        .context("index accounts.org_id")?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_iam_grants_account ON iam.grants (account_id)")
        .execute(&mut *tx)
        .await
        .context("index grants.account_id")?;

    tx.commit().await.context("commit migrate tx")?;
    Ok(())
}

// ---------- 账号读取 ----------

/// 按 email 定位账号（登录用：先凭 email 找到人，再验密码）。
pub async fn find_account_by_email(pool: &PgPool, email: &str) -> Result<Option<Account>> {
    let account = sqlx::query_as::<_, Account>(
        "SELECT id, org_id, parent_id, email, password_hash, role, totp_secret_enc, totp_enabled \
         FROM iam.accounts WHERE email = $1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await
    .context("find account by email")?;
    Ok(account)
}

/// 🔴 按 (org_id, id) 取账号：org_id 一并作为条件，跨组织取不到——隔离在 SQL 层就闭环。
pub async fn get_account(pool: &PgPool, org_id: i64, account_id: i64) -> Result<Option<Account>> {
    let account = sqlx::query_as::<_, Account>(
        "SELECT id, org_id, parent_id, email, password_hash, role, totp_secret_enc, totp_enabled \
         FROM iam.accounts WHERE id = $1 AND org_id = $2",
    )
    .bind(account_id)
    .bind(org_id)
    .fetch_optional(pool)
    .await
    .context("get account")?;
    Ok(account)
}

/// 载入某账号自身声明的全部 grants。
pub async fn load_grants(pool: &PgPool, account_id: i64) -> Result<Vec<Grant>> {
    let grants = sqlx::query_as::<_, Grant>(
        "SELECT id, account_id, resource_type, resource_id, action \
         FROM iam.grants WHERE account_id = $1 ORDER BY id",
    )
    .bind(account_id)
    .fetch_all(pool)
    .await
    .context("load grants")?;
    Ok(grants)
}

/// 一个账号能往下授的权限上限：owner / 顶层账号无上限，其余账号以自身 grants 为界。
async fn authority_of(pool: &PgPool, account: &Account) -> Result<Authority> {
    if account.role == "owner" || account.parent_id.is_none() {
        Ok(Authority::Unlimited)
    } else {
        Ok(Authority::Limited(load_grants(pool, account.id).await?))
    }
}

// ---------- 账号写入 ----------

/// 建组织 + 组织主账号（owner）。owner 是组织的根，parent_id 为 NULL，
/// 在本组织内拥有全部授权能力（不写具体 grant，权力由 role 隐含）。
/// 返回 (org_id, owner_account_id)。
pub async fn create_org_owner(
    pool: &PgPool,
    org_name: &str,
    plan: &str,
    email: &str,
    password: &str,
) -> Result<(i64, i64)> {
    let mut tx = pool.begin().await.context("begin create_org_owner")?;

    let org_id: i64 =
        sqlx::query_scalar("INSERT INTO iam.orgs (name, plan) VALUES ($1, $2) RETURNING id")
            .bind(org_name)
            .bind(plan)
            .fetch_one(&mut *tx)
            .await
            .context("insert org")?;

    let password_hash = hash_password(password)?;
    let account_id: i64 = sqlx::query_scalar(
        "INSERT INTO iam.accounts (org_id, parent_id, email, password_hash, role) \
         VALUES ($1, NULL, $2, $3, 'owner') RETURNING id",
    )
    .bind(org_id)
    .bind(email)
    .bind(&password_hash)
    .fetch_one(&mut *tx)
    .await
    .context("insert owner account")?;

    tx.commit().await.context("commit create_org_owner")?;
    Ok((org_id, account_id))
}

/// 在 parent 之下建子账号，并授予 `requested` 这组权限。
///
/// 核心校验：**子账号索要的每一条权限都必须落在父账号的授权上限内**，否则整体拒绝
/// （返回 Err）——这是「子不越父」不变量在写入侧的第一道闸（authz 的 effective_grants
/// 是运行时的第二道闸，双保险）。owner 作为组织根不受此限。
///
/// 🔴 org_id 由调用方从 JWT 传入，并校验 parent 确属该 org，杜绝跨组织建号。
pub async fn create_sub_account(
    pool: &PgPool,
    org_id: i64,
    parent_id: i64,
    email: &str,
    password: &str,
    role: &str,
    requested: &[GrantSpec],
) -> std::result::Result<i64, StoreError> {
    let parent = get_account(pool, org_id, parent_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("parent account not found in org"))?;

    let authority = authority_of(pool, &parent).await?;
    for spec in requested {
        if !authority.covers(spec) {
            return Err(StoreError::Forbidden(format!(
                "requested grant {}:{}:{} exceeds parent authority",
                spec.resource_type, spec.resource_id, spec.action
            )));
        }
    }

    let mut tx = pool.begin().await.context("begin create_sub_account")?;
    let password_hash = hash_password(password)?;
    let account_id: i64 = sqlx::query_scalar(
        "INSERT INTO iam.accounts (org_id, parent_id, email, password_hash, role) \
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(org_id)
    .bind(parent_id)
    .bind(email)
    .bind(&password_hash)
    .bind(role)
    .fetch_one(&mut *tx)
    .await
    .context("insert sub account")?;

    for spec in requested {
        sqlx::query(
            "INSERT INTO iam.grants (account_id, resource_type, resource_id, action) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(account_id)
        .bind(&spec.resource_type)
        .bind(&spec.resource_id)
        .bind(&spec.action)
        .execute(&mut *tx)
        .await
        .context("insert grant")?;
    }

    tx.commit().await.context("commit create_sub_account")?;
    Ok(account_id)
}

/// 给已有账号追加一条授权。同样校验不超过其父账号上限，杜绝「事后越权补权」。
/// 🔴 account 必须属于 org_id。
pub async fn grant(
    pool: &PgPool,
    org_id: i64,
    account_id: i64,
    spec: &GrantSpec,
) -> std::result::Result<i64, StoreError> {
    let account = get_account(pool, org_id, account_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("account not found in org"))?;

    // 上限取自「该账号的父账号」的授权范围。
    let ceiling = match account.parent_id {
        None => Authority::Unlimited, // 顶层账号
        Some(pid) => {
            let parent = get_account(pool, org_id, pid)
                .await?
                .ok_or_else(|| anyhow::anyhow!("parent account not found in org"))?;
            authority_of(pool, &parent).await?
        }
    };
    if !ceiling.covers(spec) {
        return Err(StoreError::Forbidden(format!(
            "grant {}:{}:{} exceeds parent authority",
            spec.resource_type, spec.resource_id, spec.action
        )));
    }

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO iam.grants (account_id, resource_type, resource_id, action) \
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(account_id)
    .bind(&spec.resource_type)
    .bind(&spec.resource_id)
    .bind(&spec.action)
    .fetch_one(pool)
    .await
    .context("insert grant")?;
    Ok(id)
}

// ---------- 2FA ----------

/// 开启 2FA：写入 TOTP 密钥并置 totp_enabled。⚠️ 生产此处应先加密再存（列名 _enc）。
/// 🔴 限定 org_id + account_id，不能替别的组织的账号开 2FA。
pub async fn enable_totp(
    pool: &PgPool,
    org_id: i64,
    account_id: i64,
    secret: &str,
) -> Result<()> {
    let affected = sqlx::query(
        "UPDATE iam.accounts SET totp_secret_enc = $1, totp_enabled = TRUE \
         WHERE id = $2 AND org_id = $3",
    )
    .bind(secret)
    .bind(account_id)
    .bind(org_id)
    .execute(pool)
    .await
    .context("enable totp")?
    .rows_affected();
    if affected == 0 {
        anyhow::bail!("account not found in org");
    }
    Ok(())
}

/// 登录第二步：校验该账号提交的 TOTP 码。账号没开 2FA 或没密钥时返回 false。
pub async fn verify_login_totp(
    pool: &PgPool,
    account_id: i64,
    code: &str,
    at_unix: u64,
) -> Result<bool> {
    let account = sqlx::query_as::<_, Account>(
        "SELECT id, org_id, parent_id, email, password_hash, role, totp_secret_enc, totp_enabled \
         FROM iam.accounts WHERE id = $1",
    )
    .bind(account_id)
    .fetch_optional(pool)
    .await
    .context("load account for totp")?;

    let Some(account) = account else {
        return Ok(false);
    };
    match (account.totp_enabled, account.totp_secret_enc) {
        (true, Some(secret)) => Ok(totp::verify_code(&secret, code, at_unix)),
        _ => Ok(false),
    }
}
