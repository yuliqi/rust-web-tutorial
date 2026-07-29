//! services 层：业务逻辑所在地，处于 routes（HTTP）与 db（存储）之间。
//! 与 routes 分离的核心理由是可测试性——这里的函数只依赖 Pool 和普通参数，
//! 不需要构造 HTTP 请求就能单测（见 services/todos.rs 底部的 tests）。

pub mod todos;
