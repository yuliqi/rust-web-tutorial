//! 业务逻辑层：SQL 与规则集中在这里，routes 只做 HTTP 翻译。
//! SaaS 版按功能分三块：auth（登录）、todos（租户内 CRUD + 配额）、billing（套餐变更）。

pub mod auth;
pub mod billing;
pub mod todos;
