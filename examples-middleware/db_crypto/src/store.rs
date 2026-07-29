//! 数据访问层：密文列 + 盲索引列的表结构与增查改。
//!
//! 表设计（独立 schema `dbcrypto`，避免与本 workspace 其他示例撞表）：
//!
//! ```sql
//! dbcrypto.contacts(
//!     id          BIGSERIAL PRIMARY KEY,
//!     name        TEXT,   -- 非敏感字段照常明文
//!     phone_enc   TEXT,   -- 手机号密文  v{n}:nonce:ct
//!     phone_idx   TEXT,   -- 手机号盲索引 HMAC-SHA256 的 hex，建了 B-tree 索引
//!     id_card_enc TEXT    -- 身份证密文；没有查询需求，所以不配盲索引列
//! )
//! ```
//!
//! 一条敏感字段是否需要「_idx 伴生列」取决于业务要不要按它查——
//! 身份证这里只存不查，就省掉一列（每加一列盲索引都多一份被撞库验证的暴露面）。

use anyhow::{Context, Result};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};

use crate::crypto::{KeyRing, blind_index, decrypt_field, encrypt_field, rotate_field};

/// 解密后的联系人——业务代码眼里的样子，感知不到底层加密。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contact {
    pub id: i64,
    pub name: String,
    pub phone: String,
    pub id_card: String,
}

/// 原样读出的一行——数据库里**实际存储**的样子。
/// main 用它演示「拖库的人能看到什么」：全是 v2:xxx 密文和 hex 盲索引。
#[derive(Debug, Clone)]
pub struct RawContactRow {
    pub id: i64,
    pub name: String,
    pub phone_enc: String,
    pub phone_idx: String,
    pub id_card_enc: String,
}

/// 建连接池并自动迁移。参数与 todo_api_pg 一致（快速失败的 acquire_timeout 讨论见那边）。
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

/// 幂等迁移。沿用 todo_api_pg 的事务级咨询锁模式串行化并发建表
/// （`CREATE ... IF NOT EXISTS` 的检查与创建不是原子操作，多连接同时执行会竞态）。
/// 锁 id 44 是本 crate 自选的，与 workspace 其他示例（42 等）错开。
pub async fn migrate(pool: &PgPool) -> Result<()> {
    let mut tx = pool.begin().await.context("begin migrate tx")?;
    sqlx::query("SELECT pg_advisory_xact_lock(44)")
        .execute(&mut *tx)
        .await
        .context("acquire migrate lock")?;
    // 独立 schema：多个教学 crate 共用一个 todos 库，各自圈地互不干扰。
    sqlx::query("CREATE SCHEMA IF NOT EXISTS dbcrypto")
        .execute(&mut *tx)
        .await
        .context("create schema")?;
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS dbcrypto.contacts (
            id          BIGSERIAL PRIMARY KEY,
            name        TEXT NOT NULL,
            phone_enc   TEXT NOT NULL,
            phone_idx   TEXT NOT NULL,
            id_card_enc TEXT NOT NULL
        )
        "#,
    )
    .execute(&mut *tx)
    .await
    .context("create contacts table")?;
    // 盲索引列上建的是**普通 B-tree 索引**——对数据库来说它就是一列 64 字符文本，
    // 等值查询走索引，性能与明文列查询同一量级。加密的代价没有落在查询路径上。
    sqlx::query("CREATE INDEX IF NOT EXISTS contacts_phone_idx ON dbcrypto.contacts (phone_idx)")
        .execute(&mut *tx)
        .await
        .context("create blind index")?;
    tx.commit().await.context("commit migrate tx")?;
    Ok(())
}

/// 写入联系人：敏感字段在**进库前**完成加密与盲索引计算，
/// SQL 里 bind 的全是密文/HMAC——明文从不出现在 SQL 语句与数据库日志里。
pub async fn insert_contact(
    pool: &PgPool,
    keyring: &KeyRing,
    index_key: &[u8],
    name: &str,
    phone: &str,
    id_card: &str,
) -> Result<i64> {
    let phone_enc = encrypt_field(keyring, phone);
    let phone_idx = blind_index(index_key, phone);
    let id_card_enc = encrypt_field(keyring, id_card);
    let row = sqlx::query(
        "INSERT INTO dbcrypto.contacts (name, phone_enc, phone_idx, id_card_enc)
         VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(name)
    .bind(&phone_enc)
    .bind(&phone_idx)
    .bind(&id_card_enc)
    .fetch_one(pool)
    .await
    .context("insert contact")?;
    Ok(row.get::<i64, _>("id"))
}

/// 按手机号精确查询：先把用户输入算成盲索引，再 `WHERE phone_idx = $1`。
///
/// 查询条件是 HMAC 值而不是明文——数据库端全程见不到手机号；
/// 索引列上是普通 B-tree（见 migrate 注释），性能与明文查询同级。
/// 用户输入 `"138 0000 1234"` 也能命中 `"13800001234"`：
/// blind_index 内部先规范化，写入与查询走的是同一套规则。
pub async fn find_by_phone(
    pool: &PgPool,
    keyring: &KeyRing,
    index_key: &[u8],
    phone: &str,
) -> Result<Vec<Contact>> {
    let idx = blind_index(index_key, phone);
    let rows = sqlx::query(
        "SELECT id, name, phone_enc, id_card_enc FROM dbcrypto.contacts
         WHERE phone_idx = $1 ORDER BY id",
    )
    .bind(&idx)
    .fetch_all(pool)
    .await
    .context("query by blind index")?;
    // 命中后按密文前缀选钥解密：v1 老行与 v2 新行混在结果里也各自解得开。
    rows.into_iter()
        .map(|row| {
            Ok(Contact {
                id: row.get("id"),
                name: row.get("name"),
                phone: decrypt_field(keyring, row.get::<&str, _>("phone_enc"))?,
                id_card: decrypt_field(keyring, row.get::<&str, _>("id_card_enc"))?,
            })
        })
        .collect()
}

/// 全表密钥轮换：把所有非 active 版本的密文解密后用 active 版本重加密。
/// 返回实际改写的行数。
///
/// 教学量级直接「全表捞出来逐行 UPDATE」。生产上全表可能上亿行，必须：
/// 1. **分批**：`WHERE phone_enc NOT LIKE 'v2:%' LIMIT 1000` 循环啃，
///    每批一个短事务，避免长事务膨胀与锁持有过久；
/// 2. **后台任务**：放独立 worker 低峰慢慢跑，rotate_field 的幂等性保证中断可重跑；
/// 3. 全部批次完成、确认无 v1 前缀残留后，v1 密钥才允许从密钥环下线。
pub async fn rotate_all(pool: &PgPool, keyring: &KeyRing) -> Result<usize> {
    let rows = sqlx::query("SELECT id, phone_enc, id_card_enc FROM dbcrypto.contacts")
        .fetch_all(pool)
        .await
        .context("load rows for rotation")?;
    let mut rotated = 0usize;
    for row in rows {
        let id: i64 = row.get("id");
        let phone_enc: &str = row.get("phone_enc");
        let id_card_enc: &str = row.get("id_card_enc");
        let new_phone = rotate_field(keyring, phone_enc)
            .with_context(|| format!("rotate phone_enc of contact {id}"))?;
        let new_id_card = rotate_field(keyring, id_card_enc)
            .with_context(|| format!("rotate id_card_enc of contact {id}"))?;
        // 幂等性在这里兑现：已是 active 版本的行 rotate_field 原样返回，跳过 UPDATE。
        if new_phone == phone_enc && new_id_card == id_card_enc {
            continue;
        }
        // 注意：盲索引列不用动——它取决于明文与索引密钥，与加密密钥版本无关。
        sqlx::query("UPDATE dbcrypto.contacts SET phone_enc = $1, id_card_enc = $2 WHERE id = $3")
            .bind(&new_phone)
            .bind(&new_id_card)
            .bind(id)
            .execute(pool)
            .await
            .with_context(|| format!("update rotated contact {id}"))?;
        rotated += 1;
    }
    Ok(rotated)
}

/// 原样读出一行（不解密）。演示与测试用：断言数据库里躺着的确实是密文。
pub async fn raw_row(pool: &PgPool, id: i64) -> Result<RawContactRow> {
    let row = sqlx::query(
        "SELECT id, name, phone_enc, phone_idx, id_card_enc FROM dbcrypto.contacts WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .context("load raw row")?;
    Ok(RawContactRow {
        id: row.get("id"),
        name: row.get("name"),
        phone_enc: row.get("phone_enc"),
        phone_idx: row.get("phone_idx"),
        id_card_enc: row.get("id_card_enc"),
    })
}
