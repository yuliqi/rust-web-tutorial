//! 数据模型：SaaS 版的核心变化不在 Todo 本身，而在「谁的 Todo」——
//! 数据库里每行 todos 都带 tenant_id，但对外的 JSON 里**不暴露**它：
//! 客户端永远只在自己租户的世界里操作，租户身份由 token 携带（见 auth.rs），
//! 响应里回显 tenant_id 只会诱导客户端拿它做文章。

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// 核心实体。注意结构体里没有 tenant_id 字段——SELECT 时也只取这三列，
/// 租户过滤发生在 WHERE 子句（services/todos.rs），模型层保持「租户无感」。
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, PartialEq, Eq)]
pub struct Todo {
    pub id: i64,
    pub title: String,
    pub done: bool,
}

/// 创建请求的 DTO：id/done/tenant_id 全由服务端决定。
/// 尤其 tenant_id——就算客户端在请求体里塞了这个字段，serde 反序列化到本结构体
/// 时会直接忽略，物理上断掉「伪造租户」的路径（安全红线之一，见 services/todos.rs）。
#[derive(Debug, Deserialize)]
pub struct CreateTodo {
    pub title: String,
}

/// 登录请求：邮箱 + 明文密码（走 HTTPS 传输，服务端只存 argon2 哈希）。
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

/// 登录响应：一枚 JWT。客户端此后每个请求带 `Authorization: Bearer <token>`。
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
}

/// 计费 webhook 的事件体（模拟支付商的回调格式，如 Stripe 的 event 对象）。
/// 真实支付商的事件种类很多，这里只演示一种：订阅变更 → 改租户套餐。
#[derive(Debug, Deserialize)]
pub struct BillingEvent {
    pub event: String,
    pub tenant_slug: String,
    pub plan: String,
}
