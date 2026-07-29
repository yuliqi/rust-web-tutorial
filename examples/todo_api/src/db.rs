//! 数据库层：负责建连接池 + 建表，是数据流的最底端（routes → services → 这里的 pool）。
//! 对外只暴露 `connect`，调用方拿到的 Pool 已经完成迁移、随时可用。

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

/// 建立连接池并自动建表。
/// 用「池」而不是单条连接：多个请求并发执行 SQL 时各自借一条连接，用完归还。
/// 传 `sqlite::memory:` 就是纯内存库——集成测试靠这一点做隔离（见 tests/api.rs）。
pub async fn connect(database_url: &str) -> Result<SqlitePool> {
    // create_if_missing：文件不存在就创建，省去初学者手动建库这一步。
    let options = SqliteConnectOptions::from_str(database_url)
        .with_context(|| format!("invalid DATABASE_URL: {database_url}"))?
        .create_if_missing(true);

    // 经典坑：`:memory:` 下每条物理连接各自是一个独立的内存库——
    // 若池里开多条连接，建表只发生在第一条上，其余连接看不到 todos 表。
    // 所以内存库把池收紧到 1 条连接；文件库才用多连接扛并发。
    let is_memory = database_url.contains(":memory:") || database_url.contains("mode=memory");
    let max_conns = if is_memory { 1 } else { 5 };

    // SQLite 是单文件库，写并发有限，5 条连接对本项目绰绰有余。
    // with_context 给底层错误补上「出错时在做什么」的上下文（见第 4 章）。
    let pool = SqlitePoolOptions::new()
        .max_connections(max_conns)
        .connect_with(options)
        .await
        .with_context(|| format!("connect db failed: {database_url}"))?;

    // 启动时立刻迁移：保证任何拿到这个 pool 的代码都能假设表已存在
    // （内存库正因上面收紧为单连接，这个保证才成立）。
    migrate(&pool).await?;
    Ok(pool)
}

/// 极简迁移：`CREATE TABLE IF NOT EXISTS` 天然幂等，重复启动不会出错。
/// 正式项目会用 sqlx::migrate! 管理版本化的迁移脚本，这里从简。
/// done 用 INTEGER 存 0/1——SQLite 没有布尔类型，sqlx 会自动与 Rust 的 bool 互转。
pub async fn migrate(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS todos (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            done INTEGER NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(pool)
    .await
    .context("migrate todos table")?;
    Ok(())
}
