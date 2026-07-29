# 附录 G：配置与命名约定

本附录汇总全教程 20 个 crate 共同遵守的工程约定。新增代码时照此执行，保持一致性——
一致的命名和配置方式本身就是可维护性的一部分。

## 一、配置约定

### 十二要素：配置与代码分离

每个服务一个 `config.rs`，集中把环境变量读成强类型 `Config`，其余代码只依赖 `Config`，
不再散落 `env::var`。好处：同一份编译产物/镜像，靠环境变量适配开发、测试、生产。

```rust
// examples-middleware/todo_api_saas/src/config.rs 的模式
#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    // ...
}

impl Config {
    pub fn from_env() -> Self {
        // 读环境变量 → 缺失时给「仅本地」默认值 → 强类型字段
        let database_url = env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://tutorial:tutorial@localhost:5432/todos".into());
        // ...
    }
}
```

三条铁律：

1. **默认值只兜底本地开发**。任何随环境变化的值（连接串、端口、密钥）都必须能被环境变量覆盖。
2. **密钥不进代码仓库**。`JWT_SECRET` / `WEBHOOK_SECRET` / 字段加密密钥等，环境变量是底线，
   生产走密钥管理系统（Vault / AWS Secrets Manager / k8s Secret），由它注入并支持轮换
   （见第 16.3、20.5 节）。默认值仅供本地教学，源码里用 `⚠️` 注释标出。
3. **配置读一次**。启动时 `Config::from_env()` 一次性读取，不在请求路径里反复读环境变量。

### 环境变量清单

| 变量 | 用途 | 本地默认 |
|------|------|---------|
| `HOST` | 绑定地址 | `127.0.0.1` |
| `PORT` | 端口（见下方分配表） | 各服务不同 |
| `DATABASE_URL` | 数据库连接串 | `postgres://tutorial:tutorial@localhost:5432/todos` |
| `REDIS_URL` | Redis 连接串 | `redis://127.0.0.1:6379/` |
| `JWT_SECRET` | JWT 签名密钥（SaaS） | `dev-secret-change-me` |
| `WEBHOOK_SECRET` | 计费 webhook 验签密钥 | `dev-webhook-secret` |
| `FIELD_KEY_V1/V2` | 数据库字段加密密钥（第 20 章） | 教学固定值 |
| `RUST_LOG` | 日志级别（tracing EnvFilter） | `info` |

连接串格式：`<协议>://<用户>:<密码>@<主机>:<端口>/<库名>`——
Postgres `postgres://`、MySQL `mysql://`、Redis `redis://`、RabbitMQ `amqp://`。

### 端口分配（本地可同时运行对照）

| 服务 | 端口 |
|------|------|
| `todo_api`（SQLite） | 3000 |
| `todo_api_pg` | 3001 |
| `redis_cache` 演示 | 3002 |
| `todo_api_saas` | 3003 |
| `api_crypto` | 3004 |

### 本地覆盖：.env 或 shell

```bash
# 临时覆盖单次运行
DATABASE_URL=postgres://... PORT=8080 cargo run -p todo_api_pg

# 或用 .env（记得加进 .gitignore，教学示例未引入 dotenv 依赖，保持最小）
```

## 二、命名约定

### URL 与路由

- **路径全小写**，单词间用连字符或层级 `/`，**资源用复数名词**：`/todos`、`/auth/login`
- **路径参数用 `{id}`**（axum 0.8 语法；0.7 的 `:id` 已废弃，见第 11 章）：`/todos/{id}`
- **健康检查固定两个端点**：`/health`（liveness，不碰依赖）、`/health/ready`（readiness，探依赖）
- **对外 API 加版本前缀**：`/v1/todos`；破坏性变更进 `/v2`，旧版按承诺维护（见第 18.4 节）
- **webhook 归拢到 `/webhooks/*`**：`/webhooks/billing`

HTTP 方法语义：`GET` 查询（幂等、无副作用）、`POST` 创建、`PATCH` 部分更新、
`PUT` 整体替换、`DELETE` 删除。状态码：200/201/204 成功，400 入参错，401 未认证，
403 无权限或超配额，404 不存在，409 冲突，429 限流。

### Rust 代码

| 对象 | 规范 | 示例 |
|------|------|------|
| crate 名 | `snake_case`；教学章节加 `chNN_` 前缀 | `ch01_basics`、`todo_api_pg` |
| 文件/模块名 | `snake_case`；按职责分层固定命名 | `config` `db` `error` `models` `routes` `services` |
| 目录聚合 | 用 `mod.rs` 汇总子模块 | `routes/mod.rs` |
| 类型/Trait | `UpperCamelCase` | `AppState`、`Config`、`AuthUser` |
| 函数/变量 | `snake_case` | `from_env`、`bind_addr` |
| service 层 CRUD | 动词原形 | `create` `get` `list` `update` `delete` |
| 常量/环境变量 | `SCREAMING_SNAKE_CASE` | `MAX_TITLE_LEN`、`DATABASE_URL` |

分层职责（每个服务型 crate 都照此拆分）：

```text
main.rs      仅启动：读配置、连库、监听、优雅停机
lib.rs       组装 app()（集成测试可直接调）
config.rs    环境变量 → Config
db.rs        连接池 + 迁移
error.rs     统一错误类型 + IntoResponse
models.rs    DTO / 领域模型
routes/      HTTP 层：解析参数、调 service、转响应
services/    业务逻辑（纯逻辑，不依赖 HTTP，便于单测）
```

### 数据库

- 表名 `snake_case` 复数：`todos`、`tenants`、`users`、`contacts`
- 列名 `snake_case`：`tenant_id`、`created_at`
- 加密相关列加后缀：可逆密文列 `_enc`（`phone_enc`）、盲索引列 `_idx`（`phone_idx`）（见第 20 章）
- 多服务共用一个物理库时，用**独立 schema 隔离**：`saas.todos`、`dbcrypto.contacts`
- 迁移用 `CREATE TABLE IF NOT EXISTS` + 咨询锁（`pg_advisory_xact_lock`）防并发建表竞态

## 小结

- 配置：一个 `config.rs` 收口所有环境变量，默认值仅本地，密钥走 KMS，端口错开
- URL：小写复数资源 + `{id}` 参数 + 固定健康检查端点 + 版本前缀
- 代码：`snake_case` 文件、固定分层命名、service 层 CRUD 动词
- 数据库：`snake_case` 复数表、`_enc`/`_idx` 后缀、schema 隔离

---

导航：[返回目录](../README.md)
