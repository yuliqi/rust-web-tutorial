//! 数据库层：连接池 + 迁移 + 种子数据。
//!
//! SaaS 版与 todo_api_pg 连的是**同一个物理库**（docker-compose 的 postgres），
//! 但把自己的表全部放进独立的 `saas` schema（Postgres 的命名空间）：
//! 两个示例的 `todos` 表结构不同，共用表名会互相踩踏。顺带一个知识点——
//! schema 本身也是多租户隔离的一种粒度（schema-per-tenant 方案：每个租户一个
//! schema，隔离更硬但运维更重）；本示例用的是更常见的「共表 + tenant_id 列」
//! （row-level 隔离），见 services/todos.rs。

use anyhow::{Context, Result};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// 建立连接池并自动迁移 + 播种。池参数的讲解见 todo_api_pg/src/db.rs。
pub async fn connect(database_url: &str) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
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
    seed(&pool).await?;
    Ok(pool)
}

/// 极简迁移：`CREATE ... IF NOT EXISTS` 幂等（正式项目用 sqlx::migrate!）。
///
/// 沿用 todo_api_pg 的经验：Postgres 的 IF NOT EXISTS 检查与创建不是原子操作，
/// 多实例同时启动会竞态报 duplicate key，用事务级咨询锁把迁移串行化。
/// 锁 id 选 43（todo_api_pg 用 42）：两个示例连同一个库，各用各的锁 id，
/// 互不阻塞对方的迁移。
pub async fn migrate(pool: &PgPool) -> Result<()> {
    let mut tx = pool.begin().await.context("begin migrate tx")?;
    sqlx::query("SELECT pg_advisory_xact_lock(43)")
        .execute(&mut *tx)
        .await
        .context("acquire migrate lock")?;

    sqlx::query("CREATE SCHEMA IF NOT EXISTS saas")
        .execute(&mut *tx)
        .await
        .context("create saas schema")?;

    // 租户表：SaaS 的「客户」实体。slug 是 URL 友好的唯一标识（acme、globex），
    // plan 是套餐（free/pro）——配额检查（services/todos.rs）和计费 webhook
    // （routes/webhooks.rs）都围着这一列转。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS saas.tenants (
            id   BIGSERIAL PRIMARY KEY,
            slug TEXT NOT NULL UNIQUE,
            plan TEXT NOT NULL DEFAULT 'free'
        )
        "#,
    )
    .execute(&mut *tx)
    .await
    .context("migrate tenants table")?;

    // 用户表：每个用户从属于一个租户（tenant_id 外键），email 全局唯一——
    // 登录时只凭 email 就能定位到「哪个租户的哪个人」。
    // password_hash 存 argon2 输出（绝不存明文，为什么见 auth.rs）。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS saas.users (
            id            BIGSERIAL PRIMARY KEY,
            tenant_id     BIGINT NOT NULL REFERENCES saas.tenants(id),
            email         TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            role          TEXT NOT NULL
        )
        "#,
    )
    .execute(&mut *tx)
    .await
    .context("migrate users table")?;

    // 业务表：对比 todo_api_pg 只多一列 tenant_id，但这一列改变了一切——
    // 每条 SQL 都必须带 WHERE tenant_id = $n（安全红线，见 services/todos.rs）。
    // NOT NULL + 外键让「没有归属的 todo」在数据库层面就不可能存在。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS saas.todos (
            id        BIGSERIAL PRIMARY KEY,
            tenant_id BIGINT NOT NULL REFERENCES saas.tenants(id),
            title     TEXT NOT NULL,
            done      BOOLEAN NOT NULL DEFAULT FALSE
        )
        "#,
    )
    .execute(&mut *tx)
    .await
    .context("migrate todos table")?;

    // 多租户表的第一条索引永远是 tenant_id：所有查询都以它开头过滤，
    // 没有这条索引，每个租户的每次列表都要全表扫描所有租户的数据。
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_saas_todos_tenant ON saas.todos (tenant_id)")
        .execute(&mut *tx)
        .await
        .context("create todos tenant index")?;

    tx.commit().await.context("commit migrate tx")?;
    Ok(())
}

/// 幂等种子数据：两个租户 + 三个账号，方便一启动就能试玩（账号见 main.rs 日志）。
///
/// ON CONFLICT DO NOTHING 保证重复启动不报错、也**不覆盖**已有数据——
/// 尤其 tenants.plan：webhook 可能已把 acme 升到 pro，重启不应把它打回 free。
async fn seed(pool: &PgPool) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO saas.tenants (slug, plan)
        VALUES ('acme', 'free'), ('globex', 'pro')
        ON CONFLICT (slug) DO NOTHING
        "#,
    )
    .execute(pool)
    .await
    .context("seed tenants")?;

    let seed_users = [
        ("acme", "admin@acme.test", "admin"),
        ("acme", "member@acme.test", "member"),
        ("globex", "admin@globex.test", "admin"),
    ];

    // argon2 是故意慢的（见 auth.rs），三个账号都在时跳过哈希，重启快一点。
    let existing: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM saas.users WHERE email IN ($1, $2, $3)",
    )
    .bind(seed_users[0].1)
    .bind(seed_users[1].1)
    .bind(seed_users[2].1)
    .fetch_one(pool)
    .await
    .context("count seed users")?;
    if existing == seed_users.len() as i64 {
        return Ok(());
    }

    for (slug, email, role) in seed_users {
        // 每个账号各自哈希一次：盐是随机的，即使密码相同哈希串也互不相同，
        // 数据库泄露时看不出「这几个人用同一个密码」。
        let password_hash = crate::auth::hash_password("password123")?;
        // INSERT ... SELECT：用 slug 现查租户 id，避免对种子数据硬编码 id。
        sqlx::query(
            r#"
            INSERT INTO saas.users (tenant_id, email, password_hash, role)
            SELECT id, $2, $3, $4 FROM saas.tenants WHERE slug = $1
            ON CONFLICT (email) DO NOTHING
            "#,
        )
        .bind(slug)
        .bind(email)
        .bind(&password_hash)
        .bind(role)
        .execute(pool)
        .await
        .with_context(|| format!("seed user {email}"))?;
    }
    Ok(())
}
