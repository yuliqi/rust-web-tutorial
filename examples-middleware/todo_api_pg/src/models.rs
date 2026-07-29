//! 数据模型：与 SQLite 版完全一致——这是「换数据库不动模型」的活例子。
//!
//! 差异点（发生在库里而不是这里）：done 在 SQLite 版底下是 INTEGER 0/1，
//! 靠 sqlx 转换成 bool；Postgres 里列本身就是 BOOLEAN，FromRow 直接对号入座。
//! id 也一样：BIGSERIAL 在 Rust 侧仍是 i64。所以本文件对比 SQLite 版零改动。

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// 核心实体：FromRow 负责数据库行 → 结构体，Serialize/Deserialize 负责 JSON 进出。
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, PartialEq, Eq)]
pub struct Todo {
    pub id: i64,
    pub title: String,
    pub done: bool,
}

/// 创建请求的 DTO：id/done 由服务端决定，不给客户端伪造的机会。
#[derive(Debug, Deserialize)]
pub struct CreateTodo {
    pub title: String,
}

/// PATCH 语义：字段全是 Option，「没传」= 不改。
/// 在 Postgres 版里这对 Option 还会直接绑进 SQL（COALESCE），见 services/todos.rs。
#[derive(Debug, Deserialize)]
pub struct UpdateTodo {
    pub title: Option<String>,
    pub done: Option<bool>,
}

/// 列表的查询串参数（?done=true）。
#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub done: Option<bool>,
}
