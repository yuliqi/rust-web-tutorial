//! 数据库层：建连接池 + 建表。分层位置与 SQLite 版一致（routes → services → 这里的 pool），
//! 本文件集中了「换数据库」时最先要动的两处：连接方式和建表 DDL。

use anyhow::{Context, Result};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// 建立连接池并自动建表。
///
/// 差异点：SQLite 版有 `create_if_missing`（文件不存在就创建）和「内存库收紧为
/// 单连接」的特判；Postgres 是独立的服务端进程，库由 docker-compose 的
/// POSTGRES_DB 在容器首次启动时创建，连不上就直接报错——没有那些特判要操心。
///
/// 差异点：连接池大小。SQLite 是单文件库，写并发本来就有限，5 条意思一下；
/// Postgres 每条连接对应服务端一个进程，max_connections 才真正值得斟酌：
/// 生产上限一般这样倒推——数据库侧 max_connections（默认 100）预留一部分给
/// 运维/迁移，剩下的除以应用实例数。比如 100 连接、留 20、跑 8 个实例，
/// 每实例就是 (100-20)/8 = 10。教学项目单实例本地库，10 足够演示。
pub async fn connect(database_url: &str) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        // 借连接的等待上限。默认 30 秒在生产是灾难：数据库故障时每个请求
        // 都卡满 30s 才失败，请求越积越多；就绪探针也会因此超时被 k8s 判死。
        // 快速失败（3s）让故障立刻显形，由上游重试/摘流量去兜底。
        .acquire_timeout(std::time::Duration::from_secs(3))
        .connect(database_url)
        .await
        .with_context(|| {
            format!(
                "connect db failed: {database_url}\n\
                 提示：请先在 examples-middleware/ 下执行 `docker compose up -d postgres`"
            )
        })?;

    // 与 SQLite 版一致：启动时立刻迁移，拿到 pool 的代码都能假设表已存在。
    migrate(&pool).await?;
    Ok(pool)
}

/// 极简迁移：`CREATE TABLE IF NOT EXISTS` 幂等，重复启动不出错（正式项目用 sqlx::migrate!）。
///
/// 差异点：主键。SQLite 版是 `INTEGER PRIMARY KEY AUTOINCREMENT`；
/// Postgres 用 `BIGSERIAL`——它是「BIGINT + 自动挂一个序列做默认值」的语法糖，
/// 对 Rust 侧仍映射为 i64，models.rs 里的 `id: i64` 一个字都不用改。
///
/// 差异点：done 的类型。SQLite 没有布尔类型，只能用 INTEGER 存 0/1 靠 sqlx 转换；
/// Postgres 有真正的 `BOOLEAN`，DEFAULT 也直接写 FALSE，类型系统在数据库侧就闭环了。
///
/// 差异点（并发坑）：SQLite 版是单进程访问，建表天然串行；Postgres 的
/// `CREATE TABLE IF NOT EXISTS` 在**多个连接同时执行**时会竞态报
/// duplicate key（IF NOT EXISTS 的检查和创建不是一个原子操作）——
/// 多实例同时启动、或集成测试并行跑时就会撞上。解法：用事务级咨询锁
/// （pg_advisory_xact_lock）把迁移串行化，锁随事务提交自动释放。
/// sqlx::migrate! 内部做的也是同一件事。
pub async fn migrate(pool: &PgPool) -> Result<()> {
    let mut tx = pool.begin().await.context("begin migrate tx")?;
    // 42 是本应用自选的锁 id：同一时刻只有一个连接能过这一行，其余排队
    sqlx::query("SELECT pg_advisory_xact_lock(42)")
        .execute(&mut *tx)
        .await
        .context("acquire migrate lock")?;
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS todos (
            id BIGSERIAL PRIMARY KEY,
            title TEXT NOT NULL,
            done BOOLEAN NOT NULL DEFAULT FALSE
        )
        "#,
    )
    .execute(&mut *tx)
    .await
    .context("migrate todos table")?;
    tx.commit().await.context("commit migrate tx")?;
    Ok(())
}
