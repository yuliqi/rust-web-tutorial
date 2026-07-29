# 现代 Rust 实战教程（Web 后端主线）

面向 **Rust 1.97.1 / edition 2024** 的完整学习路径：从语言基础到可上线的 Web API。

- **难度**：入门 → 进阶
- **主线**：Web 后端（`tokio` + `axum` + `serde` + `sqlx`）
- **形式**：分章 Markdown + Cargo workspace 示例工程
- **基准工具链**（本教程验证目标）

```text
rustc 1.97.1 (8bab26f4f 2026-07-14)
cargo 1.97.1 (c980f4866 2026-06-30)
rustup 1.29.0 (28d1352db 2026-03-05)
```

仓库根目录提供 `rust-toolchain.toml`，进入目录后 `rustup` 会自动选用 **1.97.1**。

## 你将收获

1. 扎实掌握所有权、类型系统、错误处理与工程化
2. 能独立搭建异步 Web API（路由、中间件、数据库、鉴权、可观测）
3. 形成可维护的项目结构与测试习惯
4. 熟悉 2024 edition 与 1.97 工具链日常工作流
5. 掌握 let chains / if-let guard / async closure / LazyLock 等现代写法

## 目录结构

```text
rust-web-tutorial/
├── .vitepress/               # VitePress 文档构建配置
├── README.md                 # 仓库总说明
├── rust-toolchain.toml       # 钉死 1.97.1 工具链
├── docs/                     # 教程文档
│   ├── README.md             # 本文件
│   ├── chapters/             # 教程正文
│   ├── appendix/             # 附录
│   └── index.md              # VitePress 首页
├── examples-middleware/      # 第 14-29 章：中间件示例（独立 workspace，需 Docker）
│   ├── docker-compose.yml    # Postgres/MySQL/Redis/RabbitMQ/etcd 一键启动
│   ├── todo_api_pg/          # PostgreSQL 版 Todo API
│   ├── mysql_demo/           # MySQL 方言差异
│   ├── redis_cache/          # cache-aside 缓存模式
│   ├── mq_rabbit/            # RabbitMQ 生产者/消费者
│   ├── cluster_demo/         # etcd 服务发现 + Redis 分布式锁
│   ├── todo_api_saas/        # SaaS 版：多租户 + JWT + 配额 + 计费 webhook
│   ├── api_crypto/           # 接口报文加密：混合信封 + Web Crypto 前端
│   ├── db_crypto/            # 数据库字段加密 + 盲索引 + 密钥轮换
│   ├── scheduler_demo/       # 定时任务：cron 调度 + 集群去重（锁+唯一约束）
│   ├── cloud_sync/           # 多云资产同步：provider 抽象 + 加密凭证 + 同步流水线
│   ├── iam_demo/             # 2FA + 层级子账号 + 资源级授权
│   ├── realtime_demo/        # WebSocket + SSE 实时推送 + 广播
│   ├── license_demo/         # 软件许可：离线 ed25519 签名 + 在线激活/吊销
│   ├── webterm_demo/         # Web 终端与堡垒机：PTY over WebSocket + 会话录制审计
│   ├── cert_demo/            # 证书管理：ACME 自动签发 + 续期判定 + HTTP-01 挑战
│   ├── monitor_demo/         # 实时监控：sysinfo 采集 + Prometheus + SSE 仪表盘 + 告警
│   └── plugin_demo/          # 应用商店 + 插件机制：compose 模板渲染 + 子进程 JSON-RPC
└── examples/                 # 可运行示例（Cargo workspace）
    ├── Cargo.toml
    ├── ch01_basics/
    ├── ch02_ownership/
    ├── ch03_structs_enums/
    ├── ch04_errors/
    ├── ch05_collections/
    ├── ch06_traits/
    ├── ch07_modules_lib/     # library crate
    ├── ch07_modules_bin/     # binary 依赖上面的 lib
    ├── ch08_testing/
    ├── ch09_smart_pointers/
    ├── ch10_async/
    ├── ch11_http_client/
    └── todo_api/             # 综合项目：待办 REST API
```

## 环境要求

最低建议：

```bash
rustc --version   # rustc 1.97.1
cargo --version   # cargo 1.97.1
rustup --version  # rustup 1.29.0
```

安装 / 切换到本教程版本：

```bash
rustup toolchain install 1.97.1
rustup component add rustfmt clippy rust-analyzer --toolchain 1.97.1
# 进入本仓库后，rust-toolchain.toml 会自动生效
rustc --version
```

兼容说明：

- **推荐**：1.97.1（与文档/示例一致）
- **通常可跟**：1.97.x 同系列
- **edition**：统一 `2024`
- 示例声明 `rust-version = "1.97"`（Cargo MSRV 提示）

推荐工具：

- 编辑器：VS Code / Zed / RustRover + `rust-analyzer`
- 质量：`clippy`、`rustfmt`
- 可选：`cargo-nextest`、`sqlx-cli`、`cargo-audit`

## 如何学习

全书按「五篇」循序渐进，从语言地基一路到用 Rust 造一个多租户 SaaS / 运维面板：

1. **按篇推进**：每一篇有明确的学习目标和前置依赖（见下方「学习路线」），建议顺序读。
2. **边看边跑**：每章对应一个可运行示例 crate。语言章（第 1–11 章）在 `examples/`，本地即可跑；
   生产/平台章（第 14 章起）在 `examples-middleware/`，多数示例要先 `docker compose up`（少数无需 Docker，已在目录标注）。
3. **做练习**：语言章的 crate 带 `src/exercises.rs`（把 `todo!()` 换成实现），
   `cargo test -p chXX -- --ignored` 自动判题，完成后对照 `src/solutions.rs`。
4. **每章有小结 + 自测清单**：读完能自查掌握度；卡壳查附录 A（常见编译错误）。

两个示例工程分别这样跑：

```bash
# 语言主线（无需 Docker）
cd examples && cargo test --workspace && cargo run -p ch01_basics

# 生产/平台示例（多数需 Docker）
cd examples-middleware && docker compose up -d && cargo test --workspace
```

> **依赖提示**：第 24 章（实时通信）是第 26、28 章的基础；第 22 章（多云同步）综合运用第 17/18/20/21 章；
> 想直奔某个主题也可以，但建议至少先过完第一、二篇打好语言与 Web 基础。

## 学习路线（五篇）

> 章节按学习顺序分五篇，每篇一个阶段目标。章号是稳定编号，也是推荐阅读顺序。

### 第一篇 · Rust 语言基础（第 0–10 章）

> **目标**：掌握所有权、类型系统、错误处理、并发异步——写出地道且能编译过的 Rust。本地即可跑，带 `todo!()` 练习。

| 章 | 标题 | 示例 crate |
|----|------|------------|
| 00 | [环境与工具链](chapters/00-environment.md) | — |
| 01 | [语言基础](chapters/01-language-basics.md) | `ch01_basics` |
| 02 | [所有权](chapters/02-ownership.md) | `ch02_ownership` |
| 03 | [结构体、枚举与模式匹配](chapters/03-structs-enums.md) | `ch03_structs_enums` |
| 04 | [错误处理](chapters/04-error-handling.md) | `ch04_errors` |
| 05 | [集合与迭代器](chapters/05-collections-iterators.md) | `ch05_collections` |
| 06 | [泛型与 Trait](chapters/06-generics-traits.md) | `ch06_traits` |
| 07 | [模块与工程组织](chapters/07-modules-workspace.md) | `ch07_modules_*` |
| 08 | [测试与质量](chapters/08-testing-quality.md) | `ch08_testing` |
| 09 | [智能指针与内存模型](chapters/09-smart-pointers.md) | `ch09_smart_pointers` |
| 10 | [并发与异步](chapters/10-concurrency-async.md) | `ch10_async` |

### 第二篇 · Web 后端主线（第 11–13 章）

> **目标**：从语言走到能上线的异步 Web API，完成综合项目 `todo_api`。前置：第一篇。

| 章 | 标题 | 示例 crate |
|----|------|------------|
| 11 | [Web 后端生态](chapters/11-web-ecosystem.md) | `ch11_http_client` |
| 12 | [综合项目：Todo API](chapters/12-todo-api-project.md) | `todo_api` |
| 13 | [进阶路线图](chapters/13-advanced-roadmap.md) | — |

### 第三篇 · 生产基建与部署（第 14–16 章）

> **目标**：让服务扛得住生产——数据库/缓存/消息队列、水平扩展与分布式、容器化上线。多数示例需 Docker（`examples-middleware/`）。

| 章 | 标题 | 示例 crate |
|----|------|------------|
| 14 | [生产中间件实战](chapters/14-middleware-production.md) | `todo_api_pg` `mysql_demo` `redis_cache` `mq_rabbit` |
| 15 | [集群与分布式入门](chapters/15-cluster-distributed.md) | `cluster_demo` |
| 16 | [部署与上线](chapters/16-deployment.md) | Dockerfile / k8s 清单 / CI |

### 第四篇 · SaaS 业务与安全（第 17–20 章）

> **目标**：做成真正的多租户 SaaS——租户隔离、鉴权、配额计费，以及传输与存储的敏感数据加密。

| 章 | 标题 | 示例 crate |
|----|------|------------|
| 17 | [SaaS：多租户与鉴权](chapters/17-multitenancy-auth.md) | `todo_api_saas`（需 Docker） |
| 18 | [SaaS：配额、计费与 API 治理](chapters/18-quota-billing-api.md) | `todo_api_saas`（同上） |
| 19 | [接口报文加密](chapters/19-api-encryption.md) | `api_crypto`（含浏览器前端，无需 Docker） |
| 20 | [数据库敏感数据存储](chapters/20-data-at-rest.md) | `db_crypto`（字段加密+盲索引，需 Docker） |

### 第五篇 · 平台化进阶：用 Rust 造一个面板/平台（第 21–29 章）

> **目标**：定时任务、多云资产同步、实时通信、Web 终端/堡垒机、证书、监控、许可、应用商店/插件——面板类软件的核心能力。
> **依赖提示**：第 24 章（实时通信）是第 26、28 章的基础；第 22 章（多云同步）综合第 17/18/20/21 章；第 23 章是第 17 章账号体系的升级。

| 章 | 标题 | 示例 crate |
|----|------|------------|
| 21 | [定时任务与后台作业](chapters/21-scheduled-jobs.md) | `scheduler_demo`（集群去重，需 Docker） |
| 22 | [多云资产同步引擎](chapters/22-cloud-asset-sync.md) | `cloud_sync`（多 provider+同步流水线，需 Docker） |
| 23 | [两步验证与层级账号（IAM）](chapters/23-2fa-iam.md) | `iam_demo`（2FA+子账号+资源授权，需 Docker） |
| 24 | [实时通信：WebSocket 与 SSE](chapters/24-realtime-websocket.md) | `realtime_demo`（含浏览器演示页，无需 Docker） |
| 25 | [软件许可：离线签名与在线激活](chapters/25-software-license.md) | `license_demo`（无需 Docker） |
| 26 | [Web 终端与堡垒机](chapters/26-web-terminal-bastion.md) | `webterm_demo`（PTY over WebSocket+会话审计，需 Docker） |
| 27 | [证书管理：ACME 自动签发续期](chapters/27-certificate-acme.md) | `cert_demo`（离线核心可跑，无需 Docker） |
| 28 | [实时监控与告警](chapters/28-realtime-monitoring.md) | `monitor_demo`（sysinfo+Prometheus+SSE 仪表盘） |
| 29 | [应用商店与插件机制](chapters/29-appstore-plugins.md) | `plugin_demo`（compose 模板+子进程插件，无需 Docker） |

附录：

- [A. 常见编译错误速查](appendix/A-common-errors.md)
- [B. 与其他语言对照](appendix/B-language-comparison.md)
- [C. 面试高频题](appendix/C-interview.md)
- [D. 资源清单](appendix/D-resources.md)
- [E. 1.97.1 工具链速查](appendix/E-toolchain-2026.md)
- [F. 1.97 新特性清单与映射](appendix/F-rust-197-features.md)
- [G. 配置与命名约定](appendix/G-conventions.md)
- [H. 堡垒机架构与选型指引](appendix/H-bastion-architecture.md)

## 学习节奏建议

- **2 周突击**：每天 1–2 章 + 跑示例（基础压缩，重点 02/04/10/11/12）
- **4 周扎实**：隔天一章，周末做 `todo_api`
- **6 周深入**：补第 09/13 与附录 C，扩展鉴权/部署

## 约定

- 工具链默认 **Rust 1.97.1**（见 `rust-toolchain.toml`）
- 代码默认 **edition = "2024"**，`rust-version = "1.97"`
- 错误处理优先 `Result` + `?`，库用 `thiserror`，应用用 `anyhow`
- Web 栈：`axum 0.8` + `tokio 1` + `serde 1` + `sqlx 0.8` + `tracing`
- 示例以「能跑、好改、可扩展、可复现」为先

开始学习 → [第 0 章：环境与工具链](chapters/00-environment.md)
