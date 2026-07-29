//! Todo 业务逻辑：校验规则 + SQL 集中在这里。本文件是整个 crate 的教学核心——
//! 与 SQLite 版（examples/todo_api/src/services/todos.rs）的方言差异全在此处：
//! 1. 占位符 `?` → `$1, $2 …`（编号占位符，可复用同一个参数）；
//! 2. INSERT/UPDATE 用 `RETURNING` 一条语句拿回整行，取代「写 + 回查」两步。

use crate::error::{AppError, AppResult};
use crate::models::{CreateTodo, Todo, UpdateTodo};
use sqlx::PgPool;

/// 校验独立成纯函数（无 IO、无 async）：单测不需要数据库，见文件底部。
/// 用 chars().count() 而非 len()：按字符数限长，中文等多字节字符不被高估。
pub fn validate_title(raw: &str) -> AppResult<String> {
    let title = raw.trim();
    if title.is_empty() {
        return Err(AppError::bad_request("title required"));
    }
    if title.chars().count() > 100 {
        return Err(AppError::bad_request("title too long (max 100)"));
    }
    Ok(title.to_string())
}

/// 差异点：占位符。SQLite 版是匿名的 `?`，按 bind 顺序对号；
/// Postgres 用编号的 `$1, $2 …`——bind 顺序仍决定编号，但同一参数
/// 可以在 SQL 里出现多次（`$1 … $1`）而只 bind 一次，`?` 做不到。
pub async fn list(pool: &PgPool, done: Option<bool>) -> AppResult<Vec<Todo>> {
    let todos = if let Some(done) = done {
        sqlx::query_as::<_, Todo>(
            "SELECT id, title, done FROM todos WHERE done = $1 ORDER BY id ASC",
        )
        .bind(done)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, Todo>("SELECT id, title, done FROM todos ORDER BY id ASC")
            .fetch_all(pool)
            .await?
    };
    Ok(todos)
}

/// fetch_optional 让「查不到」走 None，再转成带具体 id 的 404，与 SQLite 版同。
pub async fn get(pool: &PgPool, id: i64) -> AppResult<Todo> {
    let todo = sqlx::query_as::<_, Todo>("SELECT id, title, done FROM todos WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::not_found(format!("todo {id} not found")))?;
    Ok(todo)
}

/// 差异点：插入拿回整行。SQLite 版是两步——execute 后取 last_insert_rowid()
/// 再回查一次；Postgres 用 `INSERT ... RETURNING` 让数据库在同一条语句里
/// 把新行（含自增 id 和 DEFAULT 填充的列）直接吐回来：少一次往返，
/// 且天然原子——不存在「插入和回查之间行被人改了」的窗口。
/// done 不出现在列清单里，交给建表时的 DEFAULT FALSE（见 db.rs）。
pub async fn create(pool: &PgPool, input: CreateTodo) -> AppResult<Todo> {
    let title = validate_title(&input.title)?;
    let todo = sqlx::query_as::<_, Todo>(
        "INSERT INTO todos (title) VALUES ($1) RETURNING id, title, done",
    )
    .bind(&title)
    .fetch_one(pool)
    .await?;
    Ok(todo)
}

/// 差异点：部分更新一步完成。SQLite 版是「先 get 旧值、内存合并、再 UPDATE、
/// 再 get」——它自己的注释就承认这种先读后写存在「丢失更新」竞态
/// （两个并发 PATCH 各自读到旧值，后写的会覆盖先写的字段）。
/// 这里演示正确做法：把「没传就沿用旧值」的合并交给数据库的 COALESCE——
/// bind 进去的 Option 为 None 时落成 SQL 的 NULL，COALESCE(NULL, title) 即保留原值。
/// 整个读-改-写压缩进单条 `UPDATE ... RETURNING`，语句级原子，竞态消失；
/// 顺带把 4 次数据库往返压成 1 次。行不存在时 RETURNING 无行可返，
/// fetch_optional 得到 None → 404，存在性检查也一并覆盖。
pub async fn update(pool: &PgPool, id: i64, input: UpdateTodo) -> AppResult<Todo> {
    // 空 PATCH 视为客户端错误，与 SQLite 版语义保持一致。
    if input.title.is_none() && input.done.is_none() {
        return Err(AppError::bad_request(
            "at least one of title/done is required",
        ));
    }

    // 校验依然发生在 SQL 之前：坏数据不该有机会走到数据库。
    let title = input.title.map(|t| validate_title(&t)).transpose()?;

    let todo = sqlx::query_as::<_, Todo>(
        r#"
        UPDATE todos
        SET title = COALESCE($1, title),
            done  = COALESCE($2, done)
        WHERE id = $3
        RETURNING id, title, done
        "#,
    )
    .bind(title) // Option<String>：None → NULL → COALESCE 保留旧值
    .bind(input.done) // Option<bool> 同理
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found(format!("todo {id} not found")))?;
    Ok(todo)
}

/// DELETE 不因行不存在而报错，靠 rows_affected 判断 404，与 SQLite 版同（仅占位符不同）。
pub async fn delete(pool: &PgPool, id: i64) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM todos WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found(format!("todo {id} not found")));
    }
    Ok(())
}

// 分层的回报：校验是纯函数，这些单测不需要 Postgres，`cargo test` 离线即可跑。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_title_trims() {
        assert_eq!(validate_title("  hi  ").unwrap(), "hi");
    }

    #[test]
    fn validate_title_empty() {
        assert!(matches!(validate_title("  "), Err(AppError::BadRequest(_))));
    }

    #[test]
    fn validate_title_too_long() {
        let long = "字".repeat(101);
        assert!(matches!(
            validate_title(&long),
            Err(AppError::BadRequest(_))
        ));
        // 恰好 100 个字符应通过：按字符数而非字节数计（"字" 是 3 字节）。
        assert!(validate_title(&"字".repeat(100)).is_ok());
    }
}
