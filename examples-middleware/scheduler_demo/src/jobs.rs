//! 任务定义与执行留痕：把「每一次触发」落库，用唯一约束兜底防重。
//!
//! 表设计（独立 schema `scheduler`，与本 workspace 其他示例的表错开）：
//!
//! ```sql
//! scheduler.job_runs(
//!     id        BIGSERIAL PRIMARY KEY,
//!     job_name  TEXT NOT NULL,           -- 哪个任务
//!     run_key   TEXT NOT NULL,           -- 哪一次触发（幂等键，见下）
//!     instance  TEXT NOT NULL,           -- 哪个实例执行的
//!     ran_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
//!     UNIQUE (job_name, run_key)         -- 同一次触发只允许落一条
//! )
//! ```
//!
//! **run_key 是这一切的关键**：它不是随机值，而是「触发时刻取整」算出来的确定值——
//! 同一次触发在任何实例上算出的 run_key 都一样。配合 `UNIQUE(job_name, run_key)`，
//! 哪怕分布式锁因为某种原因失效（TTL 内没干完、Redis 主从切换丢锁……）导致
//! 两个实例都进了业务逻辑，数据库这道唯一约束也会让第二个 INSERT 撞车、
//! [`record_run`] 返回 false，业务据此跳过。锁是第一道防线、唯一约束是第二道，
//! **双保险**才敢在生产上说「这个任务不会重复执行」（呼应第 18 章幂等）。

use anyhow::{Context, Result};
use sqlx::PgPool;
use std::sync::atomic::{AtomicU64, Ordering};

/// docker compose 起的本地 Postgres（弱口令，仅供学习）。
pub const DEFAULT_DATABASE_URL: &str = "postgres://tutorial:tutorial@localhost:5432/todos";

/// 把「触发时刻」按周期取整，算出这一次触发的 run_key。
///
/// 例如 `period_secs = 60`（按分钟触发）时，同一分钟内的所有 unix 秒都落进
/// 同一个桶（`unix_secs / 60` 相同），run_key 就相同；跨到下一分钟才变。
/// 于是「同一次触发」在不同实例、哪怕系统时钟差几百毫秒，也几乎总能算出同一个
/// run_key——这正是唯一约束能识别「重复触发」的前提。
///
/// 注意周期要和真实触发间隔对齐：每 2 秒触发就传 `period_secs = 2`，
/// 否则一个桶里会盖住多次触发（后触发的被唯一约束吞掉），或反之。
pub fn bucket_run_key(unix_secs: i64, period_secs: i64) -> String {
    let p = period_secs.max(1);
    format!("{}", unix_secs / p)
}

/// 标识「哪个实例」执行了任务：主机名 + 进程 id。
///
/// 教学用途足够；生产里更稳的做法是读取编排平台注入的标识（k8s 的 `POD_NAME`
/// 环境变量、或用 `hostname` crate），这里用 `HOSTNAME` 环境变量兜底、取不到就写
/// `host`，再拼进程 id 做区分。它只用于「留痕看是谁干的」，不参与去重判定，
/// 所以不必全局唯一。
pub fn instance_id() -> String {
    let host = std::env::var("HOSTNAME").unwrap_or_else(|_| "host".to_string());
    format!("{host}-{}", std::process::id())
}

/// 记录一次执行。`INSERT ... ON CONFLICT DO NOTHING`：
/// - 返回 `true` = 这一次触发是**由本次调用真正记下的**（该由调用方执行业务）；
/// - 返回 `false` = `(job_name, run_key)` 已存在，说明这次触发已被别的实例
///   （或本实例的另一次并发）记过，调用方应跳过业务。
///
/// `rows_affected()` 在 ON CONFLICT DO NOTHING 未插入时为 0、插入成功为 1，
/// 用它区分两种情形。这就是「唯一约束兜底防重」落到代码上的样子。
pub async fn record_run(
    pool: &PgPool,
    job_name: &str,
    run_key: &str,
    instance: &str,
) -> Result<bool> {
    let res = sqlx::query(
        r#"
        INSERT INTO scheduler.job_runs (job_name, run_key, instance)
        VALUES ($1, $2, $3)
        ON CONFLICT (job_name, run_key) DO NOTHING
        "#,
    )
    .bind(job_name)
    .bind(run_key)
    .bind(instance)
    .execute(pool)
    .await
    .with_context(|| format!("记录任务执行失败：{job_name}/{run_key}"))?;
    Ok(res.rows_affected() == 1)
}

/// 统计某个任务累计落了多少条执行记录——给演示与测试断言用
/// （证明「3 次触发只有 3 条记录，不是 6 条」）。
pub async fn count_runs(pool: &PgPool, job_name: &str) -> Result<i64> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM scheduler.job_runs WHERE job_name = $1")
        .bind(job_name)
        .fetch_one(pool)
        .await
        .with_context(|| format!("统计任务执行数失败：{job_name}"))?;
    Ok(n)
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
/// 锁 id 45 是本 crate 自选的，与其他示例（42/43/44）错开。
pub async fn migrate(pool: &PgPool) -> Result<()> {
    let mut tx = pool.begin().await.context("begin migrate tx")?;
    sqlx::query("SELECT pg_advisory_xact_lock(45)")
        .execute(&mut *tx)
        .await
        .context("acquire migrate lock")?;
    // 独立 schema：多个教学 crate 共用一个 todos 库，各自圈地互不干扰。
    sqlx::query("CREATE SCHEMA IF NOT EXISTS scheduler")
        .execute(&mut *tx)
        .await
        .context("create schema scheduler")?;
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS scheduler.job_runs (
            id        BIGSERIAL PRIMARY KEY,
            job_name  TEXT NOT NULL,
            run_key   TEXT NOT NULL,
            instance  TEXT NOT NULL,
            ran_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
            -- 去重的第二道防线：同一次触发（同 job_name 同 run_key）只允许一行
            UNIQUE (job_name, run_key)
        )
        "#,
    )
    .execute(&mut *tx)
    .await
    .context("create job_runs table")?;
    tx.commit().await.context("commit migrate tx")?;
    Ok(())
}

/// 进程内单调递增序号——仅供演示里编造互不相同的 run_key（真实 run_key 来自
/// [`bucket_run_key`]）。放这里是为了让 tests 也能复用。
#[doc(hidden)]
pub fn next_seq() -> u64 {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    SEQ.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_key_same_within_bucket_differs_across() {
        // 取一个正好落在分钟边界上的基准时刻（能被 60 整除），便于推理桶归属。
        let base = 1_700_000_040; // 1_700_000_040 / 60 = 28333334，整除
        // 同一分钟内的两个时刻 -> 相同 run_key
        let a = bucket_run_key(base, 60);
        let b = bucket_run_key(base + 59, 60);
        assert_eq!(a, b, "同一分钟内的触发应算出相同 run_key");

        // 跨到下一分钟 -> 不同 run_key
        let c = bucket_run_key(base + 60, 60);
        assert_ne!(a, c, "跨分钟的触发应算出不同 run_key");
    }

    #[test]
    fn run_key_period_guards_zero() {
        // period 传 0 也不 panic（内部 max(1) 兜底），退化成「每秒一个桶」
        let a = bucket_run_key(100, 0);
        let b = bucket_run_key(101, 0);
        assert_ne!(a, b);
    }

    #[test]
    fn instance_id_contains_pid() {
        assert!(instance_id().ends_with(&std::process::id().to_string()));
    }
}
