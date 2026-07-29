//! services 层：业务逻辑所在地，处于 routes（HTTP）与 db（存储）之间。
//! 本 crate 的方言差异几乎全部集中在 services/todos.rs 的 SQL 里——对照着读收获最大。

pub mod todos;
