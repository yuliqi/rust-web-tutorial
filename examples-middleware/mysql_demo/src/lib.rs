//! sqlx 连 MySQL 的最小可用示例：连接池 + 建表 + CRUD。
//! 重点不是「又写一遍 CRUD」，而是看清换数据库时哪些地方要改（方言差异）、
//! 哪些地方一行不用动（sqlx 的 API 本身）。
//! 逻辑放 lib.rs 而不是全塞进 main.rs，是为了让 tests/ 目录能直接调用这些函数。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;
use sqlx::mysql::MySqlPoolOptions;

/// 一条待办事项。字段类型对着 MySQL 的列类型看：
/// - id: BIGINT → i64（MySQL 的 AUTO_INCREMENT 通常配 BIGINT，给足增长空间）
/// - done: MySQL 的 BOOLEAN 其实是 TINYINT(1) 的别名，存 0/1——
///   和 SQLite 用 INTEGER 存布尔如出一辙，sqlx 都会自动与 Rust 的 bool 互转。
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Todo {
    pub id: i64,
    pub title: String,
    pub done: bool,
}

/// 标题的最大长度。表里 title 是 VARCHAR(255)：
/// 与 SQLite 的 TEXT「来者不拒」不同，MySQL 的 VARCHAR 必须声明上限，
/// 超长默认直接报错（严格模式）。所以插库前先在应用层校验，错误信息更友好。
pub const MAX_TITLE_LEN: usize = 255;

/// 校验标题：非空、不超长。返回修剪过首尾空白的标题。
/// 注意 VARCHAR(255) 的 255 数的是「字符」，而这里为了教学从简按字符计数即可
/// （chars().count() 数的是 Unicode 标量值，与 MySQL utf8mb4 的字符数一致）。
pub fn validate_title(raw: &str) -> Result<String> {
    let title = raw.trim();
    if title.is_empty() {
        anyhow::bail!("title 不能为空");
    }
    if title.chars().count() > MAX_TITLE_LEN {
        anyhow::bail!("title 不能超过 {MAX_TITLE_LEN} 个字符");
    }
    Ok(title.to_string())
}

/// 建立连接池并建表。MySQL 是网络服务，连接握手比 SQLite 打开文件贵得多，
/// 所以「池」在这里更是刚需：连接复用，省掉每次请求都重新握手的开销。
pub async fn connect(database_url: &str) -> Result<MySqlPool> {
    let pool = MySqlPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .with_context(|| format!("connect mysql failed: {database_url}"))?;
    migrate(&pool).await?;
    Ok(pool)
}

/// 建表。对比 SQLite 版（examples/todo_api/src/db.rs）的三处方言差异：
/// - 自增主键写法：AUTO_INCREMENT（SQLite 是 AUTOINCREMENT，Postgres 用 BIGSERIAL）
/// - title 用 VARCHAR(255) 而不是 TEXT：必须给长度上限
/// - done 用 BOOLEAN：只是 TINYINT(1) 的别名，DEFAULT FALSE 实际存 0
pub async fn migrate(pool: &MySqlPool) -> Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS todos (
            id BIGINT AUTO_INCREMENT PRIMARY KEY,
            title VARCHAR(255) NOT NULL,
            done BOOLEAN NOT NULL DEFAULT FALSE
        )
        "#,
    )
    .execute(pool)
    .await
    .context("migrate todos table")?;
    Ok(())
}

/// 新增一条 todo。这里能看到最大的方言差异：
/// MySQL 没有 `RETURNING` 子句（Postgres/SQLite 3.35+ 都有，可以一条 SQL 插入并返回整行），
/// 所以只能「两步走」：先 INSERT，再从执行结果里拿 last_insert_id()。
/// 占位符倒是和 SQLite 一样用 `?`（Postgres 是 $1、$2 这种编号风格）。
pub async fn create_todo(pool: &MySqlPool, title: &str) -> Result<Todo> {
    let title = validate_title(title)?;
    let result = sqlx::query("INSERT INTO todos (title) VALUES (?)")
        .bind(&title)
        .execute(pool)
        .await
        .context("insert todo")?;
    // last_insert_id 是「本连接」上最近一次自增值——池会保证这两步在同一条连接上，
    // 因为它们同属这一次 execute 的返回值，不存在被别的插入「插队」的问题。
    let id = result.last_insert_id() as i64;
    Ok(Todo { id, title, done: false })
}

/// 按 id 查一条。fetch_optional：查不到返回 None，比 fetch_one 直接报错更好处理。
pub async fn get_todo(pool: &MySqlPool, id: i64) -> Result<Option<Todo>> {
    let todo = sqlx::query_as::<_, Todo>("SELECT id, title, done FROM todos WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .context("select todo by id")?;
    Ok(todo)
}

/// 查全部，按 id 排序保证输出稳定。
pub async fn list_todos(pool: &MySqlPool) -> Result<Vec<Todo>> {
    let todos = sqlx::query_as::<_, Todo>("SELECT id, title, done FROM todos ORDER BY id")
        .fetch_all(pool)
        .await
        .context("list todos")?;
    Ok(todos)
}

/// 更新完成状态。返回是否真的更新到了行（rows_affected == 0 说明 id 不存在）。
/// 注意：MySQL 的 rows_affected 默认只统计「值真的变了」的行，
/// 把 done 从 true 改成 true 会返回 0——判断存在性时别被这个坑到。
pub async fn set_done(pool: &MySqlPool, id: i64, done: bool) -> Result<bool> {
    let result = sqlx::query("UPDATE todos SET done = ? WHERE id = ?")
        .bind(done)
        .bind(id)
        .execute(pool)
        .await
        .context("update todo")?;
    Ok(result.rows_affected() > 0)
}

/// 删除。同样用 rows_affected 区分「删掉了」和「本来就没有」。
pub async fn delete_todo(pool: &MySqlPool, id: i64) -> Result<bool> {
    let result = sqlx::query("DELETE FROM todos WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .context("delete todo")?;
    Ok(result.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 纯函数单测：不碰数据库,离线就能跑。
    #[test]
    fn validate_title_trims_and_accepts_normal_input() {
        let title = validate_title("  写周报  ").unwrap();
        assert_eq!(title, "写周报");
    }

    #[test]
    fn validate_title_rejects_empty_and_too_long() {
        assert!(validate_title("   ").is_err());
        // 256 个字符,刚好超过 VARCHAR(255) 的上限——用中文字符顺便验证按字符而非字节计数。
        let too_long = "长".repeat(MAX_TITLE_LEN + 1);
        assert!(validate_title(&too_long).is_err());
        // 恰好 255 个字符则合法。
        let just_fit = "长".repeat(MAX_TITLE_LEN);
        assert!(validate_title(&just_fit).is_ok());
    }

    #[test]
    fn todo_serializes_to_json() {
        // Todo 派生了 Serialize/Deserialize,确认 JSON 形状符合 API 输出预期。
        let todo = Todo { id: 1, title: "学 MySQL".to_string(), done: false };
        let json = serde_json::to_string(&todo).unwrap();
        assert_eq!(json, r#"{"id":1,"title":"学 MySQL","done":false}"#);
        let back: Todo = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, 1);
        assert!(!back.done);
    }
}
