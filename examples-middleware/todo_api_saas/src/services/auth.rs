//! 登录业务：查用户 → 验密码 → 签 token。这是全系统唯一碰密码的地方。

use sqlx::PgPool;

use crate::auth::{self, Claims};
use crate::error::{AppError, AppResult};

/// 查询用的内部行结构：只在本模块用，不进 models.rs——password_hash 这种字段
/// 一旦进了公共模型，就多了一条被意外序列化进响应的路径。
#[derive(sqlx::FromRow)]
struct UserRow {
    id: i64,
    tenant_id: i64,
    password_hash: String,
    role: String,
}

/// 登录：成功返回签好的 JWT。
///
/// 「用户不存在」和「密码不对」返回**同一句** 401 消息——区分开就等于提供了
/// 免费的「邮箱是否已注册」查询接口（账户枚举漏洞）。
/// 生产还会做两件事教学从简略过：1) 用户不存在时也跑一次假哈希校验，抹平
/// 两种失败的响应耗时差；2) argon2 校验是 CPU 密集操作（故意慢），高并发下
/// 应放 tokio::task::spawn_blocking，避免占住异步工作线程。
pub async fn login(
    pool: &PgPool,
    jwt_secret: &str,
    email: &str,
    password: &str,
) -> AppResult<String> {
    let user = sqlx::query_as::<_, UserRow>(
        "SELECT id, tenant_id, password_hash, role FROM saas.users WHERE email = $1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::unauthorized("invalid email or password"))?;

    if !auth::verify_password(&user.password_hash, password) {
        return Err(AppError::unauthorized("invalid email or password"));
    }

    // 密码对上了：把「你是谁、哪个租户、什么角色」封进 token。
    // 此后 1 小时内的每个请求都凭这枚 token 说话，不再查 users 表。
    let claims = Claims::new(user.id, user.tenant_id, user.role);
    let token = auth::sign_token(jwt_secret, &claims)?;
    Ok(token)
}
