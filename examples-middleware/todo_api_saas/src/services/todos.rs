//! Todo 业务逻辑（SaaS 版）。对比 todo_api_pg 的同名文件，变化只有一类，
//! 但它是**第 17 章核心中的核心**：
//!
//! ┌─────────────────────────── 安全红线 ───────────────────────────┐
//! │ 1. 所有 SQL 一律带 `WHERE tenant_id = $n`，没有例外；          │
//! │ 2. tenant_id 只从 JWT 的 Claims 取（AuthUser 提取器解出、     │
//! │    handler 传进来），**绝不**从请求体/路径/查询串取——          │
//! │    前者是我们自己签名过的可信数据，后者是用户可控输入，        │
//! │    信了后者，任何人改个数字就能读写别家租户的数据。            │
//! │    （真实世界的多租户数据泄露事故，大半源于漏了其中一条。）    │
//! └────────────────────────────────────────────────────────────────┘
//!
//! 约定：本层所有函数把 `tenant_id: i64` 作为第一个业务参数——签名层面就
//! 提醒调用者「先想清楚是哪个租户」，想漏传编译器都不会答应。

use crate::error::{AppError, AppResult};
use crate::models::{CreateTodo, Todo};
use sqlx::PgPool;

/// free 套餐的 todo 上限。配额是「产品定价的技术执行点」：定价页写着
/// free 版 5 条、pro 版无限，落到代码就是这一个常数加 create 里的一次 COUNT——
/// 商业规则最终都要有代码去执行它，否则只是网页上的一句话。
pub const FREE_PLAN_TODO_LIMIT: i64 = 5;

/// 校验独立成纯函数（无 IO、无 async）：单测不需要数据库，见文件底部。
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

/// 列表：只见本租户。没有「不带 tenant_id 的全量列表」这种函数存在——
/// 需要跨租户统计的是运营后台，那是另一个有独立权限体系的服务。
pub async fn list(pool: &PgPool, tenant_id: i64) -> AppResult<Vec<Todo>> {
    let todos = sqlx::query_as::<_, Todo>(
        "SELECT id, title, done FROM saas.todos WHERE tenant_id = $1 ORDER BY id ASC",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await?;
    Ok(todos)
}

/// 按 id 取单条。注意 WHERE 同时按 id **和** tenant_id 过滤：
/// 别家租户的 id 在这里查不到行 → 404。刻意不区分「id 不存在」与「id 存在
/// 但属于别的租户」——返回 403 等于告诉探测者「这个 id 是有效的，是别人的」，
/// 攻击者就能靠遍历 id 摸清你平台的数据规模与分布。对外，别人的数据「不存在」。
pub async fn get(pool: &PgPool, tenant_id: i64, id: i64) -> AppResult<Todo> {
    let todo = sqlx::query_as::<_, Todo>(
        "SELECT id, title, done FROM saas.todos WHERE tenant_id = $1 AND id = $2",
    )
    .bind(tenant_id)
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found(format!("todo {id} not found")))?;
    Ok(todo)
}

/// 创建：先过配额闸门，再插入。
///
/// 配额检查（第 18 章）：free 租户最多 FREE_PLAN_TODO_LIMIT 条。plan 从
/// tenants 表现查而不是从 JWT 取——套餐随 webhook 实时变（升级要**即刻**生效，
/// 用户刚付完钱不能还被拦着），而 JWT 里的信息要等 token 过期才刷新。
/// 对比 role 放 JWT 的决定（见 routes/todos.rs）：撤销延迟能否接受，逐字段权衡。
///
/// 已知竞态：COUNT 与 INSERT 之间没有加锁，两个并发 create 可能同时数到 4、
/// 双双通过、落成 6 条。配额差一条不是安全问题（隔离才是），教学从简；
/// 要求严格时可用 `SELECT ... FOR UPDATE` 锁租户行或咨询锁串行化，留作练习。
pub async fn create(pool: &PgPool, tenant_id: i64, input: CreateTodo) -> AppResult<Todo> {
    let title = validate_title(&input.title)?;

    let plan: String = sqlx::query_scalar("SELECT plan FROM saas.tenants WHERE id = $1")
        .bind(tenant_id)
        .fetch_one(pool)
        .await?;
    if plan == "free" {
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM saas.todos WHERE tenant_id = $1")
                .bind(tenant_id)
                .fetch_one(pool)
                .await?;
        if count >= FREE_PLAN_TODO_LIMIT {
            // 错误消息顺带做产品引导（upgrade to pro）——配额报错是转化率的触点。
            return Err(AppError::forbidden(
                "todo quota exceeded (free plan), upgrade to pro",
            ));
        }
    }

    // tenant_id 由服务端写入（来源是 JWT），INSERT ... RETURNING 一次往返拿回整行。
    let todo = sqlx::query_as::<_, Todo>(
        "INSERT INTO saas.todos (tenant_id, title) VALUES ($1, $2) RETURNING id, title, done",
    )
    .bind(tenant_id)
    .bind(&title)
    .fetch_one(pool)
    .await?;
    Ok(todo)
}

/// 删除：同样双条件过滤。rows_affected == 0 统一映射为 404——
/// 不管是「本来就没有」还是「有但属于别家」，对外都是不存在（理由见 get）。
/// 注：角色检查（仅 admin 可删）发生在 HTTP 层的 handler 里，本层只管租户边界。
pub async fn delete(pool: &PgPool, tenant_id: i64, id: i64) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM saas.todos WHERE tenant_id = $1 AND id = $2")
        .bind(tenant_id)
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found(format!("todo {id} not found")));
    }
    Ok(())
}

// PATCH /todos/{id} 留作练习：照抄 todo_api_pg 的 COALESCE 单语句版本，
// 唯一要点是 WHERE 里除了 id 别忘了 tenant_id——忘了它，练习就变事故复盘了。

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
