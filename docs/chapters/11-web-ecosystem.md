# 第 11 章：Web 后端生态

> 示例：`examples/ch11_http_client`（客户端）+ 预告 `todo_api`
>
> 工具链：`rustc 1.97.1` / `cargo 1.97.1` / `edition 2024`

## 学习目标

- 认识现代 Rust Web 关键 crates
- 能发 HTTP 请求并处理 JSON
- 搭起 axum 服务的最小骨架认知

## 11.1 推荐技术栈（Rust 1.97 / 2026 实用向）

| 层 | Crate | 用途 |
|----|-------|------|
| 运行时 | `tokio` | 异步执行器 |
| HTTP 服务 | `axum` | 路由/提取器/中间件 |
| 序列化 | `serde` + `serde_json` | JSON |
| DB | `sqlx` | 编译期/运行时 SQL 检查 |
| 日志 | `tracing` + `tracing-subscriber` | 结构化日志 |
| 错误 | `thiserror` / `anyhow` | 错误模型 |
| 配置 | `serde` + env | 12-factor 配置 |
| 校验 | `validator`（可选） | 入参校验 |
| 鉴权 | `jsonwebtoken`（可选） | JWT |

默认依赖版本见 `examples/Cargo.toml` 与 [附录 E](../appendix/E-toolchain-2026.md)（围绕 axum 0.8 的可复现组合）。

## 11.2 JSON 与 serde

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct CreateTodo {
    title: String,
    #[serde(default)]
    done: bool,
}
```

## 11.3 HTTP 客户端：`reqwest`

```rust
let client = reqwest::Client::new();
let todo = client
    .post("https://httpbin.org/post")
    .json(&CreateTodo { title: "learn rust".into(), done: false })
    .send()
    .await?
    .error_for_status()?;
```

测试下游、写爬虫、服务间调用都会用到。

## 11.4 HTTP 服务：axum 最小形态

```rust
use axum::{routing::get, Router};

async fn health() -> &'static str { "ok" }

let app = Router::new().route("/health", get(health));
// axum::serve(listener, app).await
```

提取器（Extractors）是 axum 的灵魂：

- `Path` / `Query` / `Json` / `State` / `HeaderMap`

```rust
use axum::{extract::{Path, State}, Json};

// AppState / Todo / AppError 的完整定义见第 12 章 todo_api 项目
async fn get_todo(
    Path(id): Path<i64>, // 与数据库主键类型一致（sqlite 主键是 i64）
    State(state): State<AppState>,
) -> Result<Json<Todo>, AppError> {
    // ...
}
```

> **axum 0.8 破坏性变更提醒**：路径参数写法从 0.7 的 `/todos/:id` 改成了
> `/todos/{id}`。网上老教程里的冒号写法在 0.8 会直接 panic，照抄前先看版本。

## 11.5 中间件与可观测

常见需求：

- 请求 ID
- 访问日志
- 超时
- CORS
- 鉴权

`tower` / `tower-http` 与 axum 无缝协作：

```rust
use tower_http::trace::TraceLayer;
let app = Router::new().layer(TraceLayer::new_for_http());
```

## 11.6 数据库：`sqlx` 直觉

```rust
// 伪代码
let pool = SqlitePool::connect(&db_url).await?;
sqlx::query("INSERT INTO todos (title) VALUES (?)")
    .bind(title)
    .execute(&pool)
    .await?;
```

连接池放进 `AppState`，handler 不要每次新建连接。

## 11.7 分层建议

```text
routes  ->  解析 HTTP，调 service
service ->  业务规则
repo    ->  SQL/缓存
models  ->  数据结构
error   ->  统一错误到 HTTP
```

不要把 SQL 直接堆在 handler 里（demo 可，产品别）。

## 运行示例

```bash
cargo run -p ch11_http_client
```

> 该示例默认访问公共 httpbin；若网络受限，代码里也有本地 JSON 编解码演示。

## 本章小结

- 现代 Rust Web 主力栈：`tokio` + `axum` + `serde` + `sqlx` + `tracing` + `thiserror`/`anyhow`
- `serde` 的 derive 宏让 JSON 序列化/反序列化几乎零样板
- `reqwest` 是 HTTP 客户端首选，链式调用 + `error_for_status` 处理失败
- axum 的灵魂是提取器：`Path` / `Query` / `Json` / `State` / `HeaderMap`
- axum 0.8 路径参数是 `{id}`，老教程的 `:id` 写法会 panic
- 中间件走 `tower` / `tower-http`，日志、超时、CORS 都是一层 `layer`
- 分层原则：routes → service → repo → models，SQL 不进 handler

**自测清单**

- [ ] 我能说出技术栈里每个核心 crate 的职责
- [ ] 我能用 `serde` derive 定义一个带默认值字段的 DTO
- [ ] 我能写出 axum 最小路由并说出常用提取器
- [ ] 我能说出 axum 0.8 路径参数与 0.7 的写法差异
- [ ] 我能画出 routes/service/repo 的分层调用关系

## 练习

1. 定义 `Todo` 的 request/response DTO。
2. 用 `reqwest` 调一个 GET JSON API 并反序列化。
3. 手写一页 axum `Router` 设计（可先不实现 DB）。

**动手做题**：打开 `examples/ch11_http_client/src/exercises.rs`，把 `todo!()` 换成你的实现。三道题分别对应：`pending_titles`（serde 解析过滤）、`build_query_url`（URL 拼接）、`classify_status`（状态码映射）。

```bash
cd examples
cargo test -p ch11_http_client -- --ignored   # 自动判题（没做完时失败是预期的）
```

全部通过后对照参考答案 `src/solutions.rs`（默认随 `cargo test` 验证，答案保证可靠）。

---

导航：[上一章](10-concurrency-async.md) | [返回目录](../README.md) | [下一章](12-todo-api-project.md)
