//! 数据模型：定义 API 边界（JSON）与数据库边界（行）上流动的类型。
//! 两套 derive 各司其职：serde 负责 JSON 的进出，sqlx 的 FromRow 负责查询结果 → 结构体。
//! 一个类型可以同时挂两者（如 Todo），也可以只挂一边（请求体只需 Deserialize）。

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// 核心实体，横跨两个边界：
/// - `FromRow`：query_as 按列名把数据库行填进字段（done 的 INTEGER 自动转 bool）；
/// - `Serialize`：作为响应体输出 JSON；`Deserialize` 则让测试可以反解校验。
#[derive(Debug, Clone, Serialize, Deserialize, FromRow, PartialEq, Eq)]
pub struct Todo {
    pub id: i64,
    pub title: String,
    pub done: bool,
}

/// 创建请求的 DTO。与 Todo 分开定义是刻意的：
/// id/done 由服务端决定，不给客户端伪造的机会。
#[derive(Debug, Deserialize)]
pub struct CreateTodo {
    pub title: String,
}

/// PATCH 语义：字段全是 Option，「没传」= 不改。
/// serde 反序列化时缺失的字段自然落成 None，正好匹配部分更新的需求。
/// 注意：显式传 `"title": null` 也会落成 None——这个设计里「没传」和
/// 「传 null」不可区分，要区分就得引入双层 Option 或自定义反序列化。
#[derive(Debug, Deserialize)]
pub struct UpdateTodo {
    pub title: Option<String>,
    pub done: Option<bool>,
}

/// 列表的查询串参数（?done=true）。Option 表示过滤条件可有可无。
#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub done: Option<bool>,
}
