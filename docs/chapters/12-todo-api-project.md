# 第 12 章：综合项目 — Todo REST API

> 示例工程：`examples/todo_api`
>
> 要求：Rust **1.97.1** + edition **2024**（见仓库 `rust-toolchain.toml`）

## 学习目标

实现一个可运行的待办服务：

- `GET /health`
- `GET /todos`
- `GET /todos/{id}`
- `POST /todos`
- `PATCH /todos/{id}`
- `DELETE /todos/{id}`

> 路由里的 `{id}` 是 axum 0.8 的路径参数语法（0.7 及更早是 `:id`，见第 11 章提醒）。

技术点：

- axum 路由与提取器
- 统一错误响应
- sqlite + sqlx
- tracing 日志
- 单元测试 + API 测试

## 12.1 快速启动

先确认环境并跑通测试：

```bash
rustc --version   # rustc 1.97.1
cargo --version   # cargo 1.97.1
cd examples && cargo test -p todo_api
```

启动服务：

```bash
cd examples
cargo run -p todo_api
```

默认监听：`http://127.0.0.1:3000`

试玩：

```bash
curl -s http://127.0.0.1:3000/health
curl -s -X POST http://127.0.0.1:3000/todos \
  -H 'content-type: application/json' \
  -d '{"title":"learn axum"}'
curl -s http://127.0.0.1:3000/todos
```

环境变量（可选）：

| 变量 | 默认 | 含义 |
|------|------|------|
| `HOST` | `127.0.0.1` | 绑定地址 |
| `PORT` | `3000` | 端口 |
| `DATABASE_URL` | `sqlite:todo.db` | 数据库 |

## 12.2 目录导读

```text
todo_api/src/
├── main.rs           # 入口
├── lib.rs            # 组装 app，便于测试
├── config.rs         # 配置
├── error.rs          # AppError -> HTTP
├── models.rs         # DTO / 领域模型
├── db.rs             # 连接与迁移
├── routes/
│   ├── mod.rs
│   ├── health.rs
│   └── todos.rs
└── services/
    ├── mod.rs
    └── todos.rs
```

## 12.3 核心设计

### 状态

```rust
#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::SqlitePool,
}
```

### 错误到 HTTP

业务错误映射：

- 校验失败 → 400
- 未找到 → 404
- 其他 → 500 + 日志

### 服务层

handler 只做：

1. 提取参数
2. 调 service
3. 转响应

校验与 SQL 不在路由里堆成一锅粥。

## 12.4 API 契约

### 创建

```http
POST /todos
{"title":"write docs"}
```

响应 `201`：

```json
{"id":1,"title":"write docs","done":false}
```

### 更新

```http
PATCH /todos/1
{"title":"write better docs","done":true}
```

字段可选；至少提供一个字段。

### 列表

```http
GET /todos?done=false
```

## 12.5 测试策略

```bash
cargo test -p todo_api
```

包含：

1. 标题校验单测
2. 使用临时 sqlite 的路由级测试

## 12.6 扩展作业（强烈建议做）

按优先级：

1. **分页**：`page` / `page_size`
2. **鉴权**：最简 Bearer Token 中间件
3. **用户维度**：`user_id` 列与过滤
4. **OpenAPI**：`utoipa` 生成文档
5. **Docker**：多阶段构建最小化镜像
6. **迁移工具**：`sqlx migrate`
7. **观测**：请求耗时直方图（`metrics`）

## 12.7 完成标准 checklist

- [ ] 六类接口全部可用
- [ ] 错误返回 JSON，而不是纯文本乱码
- [ ] 日志能看到 method/path/status
- [ ] `cargo test -p todo_api` 通过
- [ ] `clippy` 无新增警告

## 本章小结

- `todo_api` 把前面所有章节串起来：模块分层、错误处理、异步、sqlx、测试
- `AppState` 只装一个 `SqlitePool`，`Clone` 后随处理器传递
- 错误统一映射到 HTTP：校验失败 400、未找到 404、其他 500 + 日志
- handler 保持三步：提取参数、调 service、转响应
- API 契约先行：路由、请求体、响应码写清楚再动手
- 测试分两层：纯函数单测 + 临时 sqlite 的路由级测试
- 扩展作业（分页、鉴权、OpenAPI、Docker）是从 demo 走向产品的路径

**自测清单**

- [ ] 我能解释为什么 `lib.rs` 组装 app 而 `main.rs` 只做启动
- [ ] 我能说出 AppError 如何映射到不同的 HTTP 状态码
- [ ] 我能解释连接池为什么放 `AppState` 而不是每次新建
- [ ] 我能说出路由级测试为什么用临时 sqlite
- [ ] 我能描述 PATCH 语义下「字段可选但至少一个」怎么校验

---

导航：[上一章](11-web-ecosystem.md) | [返回目录](../README.md) | [下一章](13-advanced-roadmap.md)
