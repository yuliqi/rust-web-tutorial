//! Todo 业务逻辑：校验规则 + SQL 都集中在这里，routes 只负责转发。
//! 所有函数返回 AppResult，sqlx 错误经 `From<sqlx::Error>` 自动转成 AppError（见 error.rs），
//! 所以下面每个 `.await?` 都不需要手写错误转换。
//!
//! 参数只收 `&SqlitePool` 而不是整个 AppState：依赖越窄，函数越容易被复用和测试。

use crate::error::{AppError, AppResult};
use crate::models::{CreateTodo, Todo, UpdateTodo};
use sqlx::SqlitePool;

/// 校验独立成纯函数（无 IO、无 async）：可以写最快的单元测试，见文件底部。
/// 用 chars().count() 而非 len()：按「字符数」限长，避免中文等多字节字符被 len() 高估。
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

/// query_as::<_, Todo> 靠 models.rs 里的 FromRow 把行映射成结构体。
/// `?` 占位符 + bind 是参数化查询：值不会拼进 SQL 字符串，从根上杜绝 SQL 注入。
pub async fn list(pool: &SqlitePool, done: Option<bool>) -> AppResult<Vec<Todo>> {
    let todos = if let Some(done) = done {
        sqlx::query_as::<_, Todo>(
            "SELECT id, title, done FROM todos WHERE done = ? ORDER BY id ASC",
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

/// fetch_optional 返回 Option，让「查不到」走 None 而不是错误分支，
/// 再用 ok_or_else 转成带具体 id 的 404 消息——比依赖 RowNotFound 的通用文案更友好。
pub async fn get(pool: &SqlitePool, id: i64) -> AppResult<Todo> {
    let todo = sqlx::query_as::<_, Todo>("SELECT id, title, done FROM todos WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::not_found(format!("todo {id} not found")))?;
    Ok(todo)
}

/// 先校验再写库：坏数据在进数据库之前就被 400 挡下。
/// 插入后按 last_insert_rowid 回查一次，保证响应体就是数据库里的真实状态。
pub async fn create(pool: &SqlitePool, input: CreateTodo) -> AppResult<Todo> {
    let title = validate_title(&input.title)?;
    let result = sqlx::query("INSERT INTO todos (title, done) VALUES (?, 0)")
        .bind(&title)
        .execute(pool)
        .await?;
    get(pool, result.last_insert_rowid()).await
}

/// PATCH 的「部分更新」实现：先取当前值，客户端没传的字段沿用旧值（见 models.rs 的 Option 设计）。
/// 先 get 也顺便完成了存在性检查——id 不存在时直接 404，不会执行 UPDATE。
/// 注意：先读后写是两条独立语句，并发 PATCH 同一条记录存在「丢失更新」竞态；
/// 教学从简，正式项目应把读写放进事务，或用 `UPDATE ... RETURNING` 一步完成。
pub async fn update(pool: &SqlitePool, id: i64, input: UpdateTodo) -> AppResult<Todo> {
    // 空 PATCH 视为客户端错误：什么都不改的请求多半是调用方写错了。
    if input.title.is_none() && input.done.is_none() {
        return Err(AppError::bad_request(
            "at least one of title/done is required",
        ));
    }

    let current = get(pool, id).await?;
    let title = match input.title {
        Some(t) => validate_title(&t)?,
        None => current.title,
    };
    let done = input.done.unwrap_or(current.done);

    sqlx::query("UPDATE todos SET title = ?, done = ? WHERE id = ?")
        .bind(&title)
        .bind(done)
        .bind(id)
        .execute(pool)
        .await?;

    get(pool, id).await
}

/// DELETE 不会因为行不存在而报错，所以要看 rows_affected：
/// 删了 0 行说明 id 不存在，主动返回 404，而不是假装成功。
pub async fn delete(pool: &SqlitePool, id: i64) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM todos WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found(format!("todo {id} not found")));
    }
    Ok(())
}

// 分层的回报：校验逻辑是纯函数，测试不需要数据库、不需要 async（测试写法见第 8 章）。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_title_trims() {
        assert_eq!(validate_title("  hi  ").unwrap(), "hi");
    }

    #[test]
    fn validate_title_empty() {
        assert!(matches!(
            validate_title("  "),
            Err(AppError::BadRequest(_))
        ));
    }
}
