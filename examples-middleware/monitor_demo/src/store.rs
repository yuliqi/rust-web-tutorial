//! 时序存储（第 28 章配套）：把采集到的 [`Metric`] 落进 Postgres，支持按时间范围回查。
//!
//! ## 一句话前提：关系库存指标只适合教学 / 小量
//!
//! 生产上「存指标」是**时序数据库**（Prometheus / VictoriaMetrics / InfluxDB / TimescaleDB）
//! 的活。它们针对「海量、只追加、按时间窗聚合」的负载做了专门优化：列式压缩（同名指标的值
//! 高度相似，压缩比惊人）、按时间分区/自动过期、`rate()`/`histogram_quantile()` 这类时序函数。
//! 拿普通关系表存指标，数据量一大，索引膨胀、聚合查询慢、还得自己写降采样与过期清理——
//! 这里用一张表 + 一个索引，只是为了让「落库→回查」这条链路能在本仓库已有的 Postgres 上
//! 完整跑通、可测，**别照搬进生产**。真实链路是：本服务按 Prometheus 文本格式暴露 `/metrics`
//! （见 [`crate::metrics::render_prometheus`]），由 Prometheus server 抓取并存进它自己的时序库。
//!
//! 表结构：`monitor.metric_samples(id, name, value, labels JSONB, ts_ms)`，
//! 建 `(name, ts_ms)` 复合索引——回查永远是「按指标名 + 时间范围」，这正是它的最左前缀。

use anyhow::{Context, Result};
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};

use crate::metrics::Metric;

/// 默认连接串，对齐 examples-middleware/docker-compose.yml（与 todo_api_pg 一致）。
pub const DEFAULT_DATABASE_URL: &str = "postgres://tutorial:tutorial@localhost:5432/todos";

/// 建连接池并自动建表（迁移）。做法与 todo_api_pg/src/db.rs 一致：启动即迁移，
/// 拿到 pool 的代码都能假设 schema/表/索引已就位。
pub async fn connect(database_url: &str) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        // 快速失败（3s）：数据库故障时立刻显形，而不是每个请求都卡满默认 30s。
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

/// 极简幂等迁移：建独立 schema、建表、建索引。
///
/// 用**独立 schema** `monitor`（而非塞进 public）：本示例与 todo_api_pg 等共用同一个
/// `todos` 库，各自占一个 schema 才不会互相踩表名——这也是多模块共库时的常规隔离手法。
///
/// 并发坑同 todo_api_pg：多连接同时 `CREATE ... IF NOT EXISTS` 会竞态报 duplicate key
/// （检查与创建不是一个原子操作）。解法一样——事务级咨询锁把迁移串行化，锁随事务提交自动释放。
/// **锁 id 取 49**，与其他示例（todo_api_pg 用 42）错开，避免不同模块的迁移互相阻塞。
pub async fn migrate(pool: &PgPool) -> Result<()> {
    let mut tx = pool.begin().await.context("begin migrate tx")?;
    sqlx::query("SELECT pg_advisory_xact_lock(49)")
        .execute(&mut *tx)
        .await
        .context("acquire migrate lock")?;

    sqlx::query("CREATE SCHEMA IF NOT EXISTS monitor")
        .execute(&mut *tx)
        .await
        .context("create schema monitor")?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS monitor.metric_samples (
            id     BIGSERIAL PRIMARY KEY,
            name   TEXT NOT NULL,
            value  DOUBLE PRECISION NOT NULL,
            labels JSONB NOT NULL DEFAULT '[]'::jsonb,
            ts_ms  BIGINT NOT NULL
        )
        "#,
    )
    .execute(&mut *tx)
    .await
    .context("create table metric_samples")?;

    // 回查恒为「WHERE name = ? AND ts_ms BETWEEN ? AND ?」，(name, ts_ms) 复合索引正好命中。
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_metric_samples_name_ts \
         ON monitor.metric_samples (name, ts_ms)",
    )
    .execute(&mut *tx)
    .await
    .context("create index")?;

    tx.commit().await.context("commit migrate tx")?;
    Ok(())
}

/// 批量落库：把一批样本写进 `monitor.metric_samples`，返回写入行数。
///
/// 用一个事务包住整批：要么全成、要么全败，不会出现「一次采集的指标只落了一半」的割裂状态。
/// `labels` 是 `Vec<(String, String)>`，序列化成 JSON 数组（形如 `[["mount","/"]]`）存进 JSONB，
/// 回查时再反序列化回来——[`query_range`] 与此对称。
///
/// 教学从简用「逐行 INSERT」，可读性最好；样本量大时应改成多值 `INSERT ... VALUES (...),(...)`
/// 或 `UNNEST` 批量插，减少往返。
pub async fn save_samples(pool: &PgPool, samples: &[Metric]) -> Result<u64> {
    if samples.is_empty() {
        return Ok(0);
    }
    let mut tx = pool.begin().await.context("begin save tx")?;
    let mut n = 0u64;
    for m in samples {
        let labels: Value = serde_json::to_value(&m.labels).unwrap_or_else(|_| Value::Array(vec![]));
        let res = sqlx::query(
            "INSERT INTO monitor.metric_samples (name, value, labels, ts_ms) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(&m.name)
        .bind(m.value)
        .bind(labels)
        .bind(m.ts_ms)
        .execute(&mut *tx)
        .await
        .context("insert metric sample")?;
        n += res.rows_affected();
    }
    tx.commit().await.context("commit save tx")?;
    Ok(n)
}

/// 按「指标名 + 时间范围 [from_ms, to_ms]」回查样本，按时间升序返回（画趋势图正是这个查法）。
///
/// 区间是**闭区间**（`BETWEEN` 含两端）；命中前面建的 `(name, ts_ms)` 索引。
/// 二级排序用 `id`，保证同一毫秒内多条样本的顺序也稳定、结果可复现。
pub async fn query_range(
    pool: &PgPool,
    name: &str,
    from_ms: i64,
    to_ms: i64,
) -> Result<Vec<Metric>> {
    let rows = sqlx::query(
        "SELECT name, value, labels, ts_ms \
         FROM monitor.metric_samples \
         WHERE name = $1 AND ts_ms BETWEEN $2 AND $3 \
         ORDER BY ts_ms, id",
    )
    .bind(name)
    .bind(from_ms)
    .bind(to_ms)
    .fetch_all(pool)
    .await
    .context("query_range")?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let labels_json: Value = row.try_get("labels").context("read labels")?;
        let labels: Vec<(String, String)> = serde_json::from_value(labels_json).unwrap_or_default();
        out.push(Metric {
            name: row.try_get("name").context("read name")?,
            value: row.try_get("value").context("read value")?,
            labels,
            ts_ms: row.try_get("ts_ms").context("read ts_ms")?,
        });
    }
    Ok(out)
}
