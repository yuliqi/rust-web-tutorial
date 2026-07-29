//! 层级账号与授权模型（全部落在独立的 `iam` schema）。
//!
//! 相比第 17 章 todo_api_saas 的扁平「租户 + admin/member」，本章把账号做成三层能力：
//!
//! 1. **层级子账号**（accounts.parent_id 自引用）：组织下的账号能再开子账号，
//!    形成树。核心不变量——「子账号权限不超过父账号」（见 authz.rs 的 effective_grants）。
//! 2. **资源级授权**（grants 表）：不再只有粗粒度的 role，而是能精确到
//!    「这个子账号只能访问 cloud-account:A 这一个云账号的资产」。
//! 3. 两者结合：真实系统里 role 定「大方向」（粗粒度、够用就好），grants 定
//!    「具体能碰哪些资源」（细粒度、按需授予）——本章新增的正是后者。
//!
//! 这里的 struct 是 schema 的「Rust 侧真相」；建表 DDL 见 store.rs 的 migrate，
//! 字段一一对应。

use serde::{Deserialize, Serialize};

/// 组织：SaaS 的「客户/团队」实体，是一切隔离的最外层边界。
/// 所有账号、授权都挂在某个 org_id 下，跨 org 一律不可见（store.rs 的红线）。
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Org {
    pub id: i64,
    pub name: String,
    /// 套餐（free/pro…），本章不展开计费，保留字段对齐第 17 章。
    pub plan: String,
}

/// 账号：组织下的一个可登录主体。
/// - parent_id 自引用形成层级：主账号 parent_id 为 NULL，子账号指向其父。
/// - role 是粗粒度角色（沿用第 17 章）；细粒度能力在 grants 表。
/// - totp_secret_enc / totp_enabled 承载 2FA 状态（列名 `_enc` 标注：生产应加密存）。
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Account {
    pub id: i64,
    pub org_id: i64,
    pub parent_id: Option<i64>,
    pub email: String,
    #[serde(skip_serializing)] // 哈希也别往响应里漏
    pub password_hash: String,
    pub role: String,
    #[serde(skip_serializing)] // 敏感：TOTP 密钥绝不出现在任何 API 响应里
    pub totp_secret_enc: Option<String>,
    pub totp_enabled: bool,
}

/// 一条资源级授权：账号 account_id 可以对 `resource_type` 类型下的
/// `resource_id` 这个资源执行 `action`。
///
/// 约定 `resource_id = "*"` 表示「该类型下的全部资源」，`action = "*"` 表示
/// 「全部动作」——通配的判定逻辑集中在 authz.rs 的 can()。
///
/// 例：`{resource_type:"cloud-account", resource_id:"A", action:"read"}`
/// = 「可以读取云账号 A 的资产」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct Grant {
    pub id: i64,
    pub account_id: i64,
    pub resource_type: String,
    pub resource_id: String,
    pub action: String,
}

/// 授予授权时的入参（还没有 id / account_id，由 store 落库时补齐）。
/// authz.rs 的纯逻辑也复用它来表达「一条待判定的权限」。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantSpec {
    pub resource_type: String,
    pub resource_id: String,
    pub action: String,
}

impl GrantSpec {
    pub fn new(
        resource_type: impl Into<String>,
        resource_id: impl Into<String>,
        action: impl Into<String>,
    ) -> Self {
        Self {
            resource_type: resource_type.into(),
            resource_id: resource_id.into(),
            action: action.into(),
        }
    }
}

impl Grant {
    /// 丢掉 id/account_id，只留「权限本身」的三元组，方便和 GrantSpec 比较、判定。
    pub fn spec(&self) -> GrantSpec {
        GrantSpec {
            resource_type: self.resource_type.clone(),
            resource_id: self.resource_id.clone(),
            action: self.action.clone(),
        }
    }
}
