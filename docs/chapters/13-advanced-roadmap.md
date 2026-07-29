# 第 13 章：进阶路线图

## 学习目标

- 了解宏、FFI、WASM、性能工程等进阶方向的入口与代表工具
- 能按自身兴趣（平台工程/系统编程/全栈）规划下一阶段学习路线
- 建立后端安全与依赖升级的常备清单意识
- 用 30 天巩固计划把已学内容落到真实项目上

基础与 Web 主线完成后，按兴趣继续深挖。

> 当前教程基准：`rustc 1.97.1` / `cargo 1.97.1` / `rustup 1.29.0`。
> 跟进新版本时，先看 [Release Notes](https://github.com/rust-lang/rust/releases)，再用本仓库 `cargo test --workspace` 回归。

## 13.0 紧盯但未进主线的特性

- `gen` blocks / `yield`：1.97 仍 experimental，别写进业务代码
- `try { ... }` 表达式：同样未稳定；继续用 `?` + 函数边界
- 跟进方式：每次升级工具链后先看 [附录 F](../appendix/F-rust-197-features.md) 与官方 Release Notes

## 13.1 宏

- 声明宏 `macro_rules!`：消除样板
- 过程宏：自定义 derive、属性宏
- 先会用，再写；优先现成生态

## 13.2 FFI 与互操作

- 与 C ABI 交互：`extern "C"`
- `bindgen` / `cxx`
- PyO3（Python 扩展）
- 注意所有权与线程安全边界

## 13.3 WebAssembly

- `wasm-bindgen` + `wasm-pack`
- 前端计算下沉、同构校验逻辑
- 注意包体积与 panic 策略

## 13.4 性能工程

- `cargo flamegraph` / `perf`
- 减少分配：`bytes::Bytes`、对象池
- 异步取消与背压
- 数据库 N+1、索引、连接池参数

## 13.5 架构升级

从单体 Todo 走向：

1. 模块化单体（workspace 多 crate）
2. 读写分离与缓存（Redis，实战见[第 14 章](14-middleware-production.md)）
3. 后台任务（队列 + worker，实战见[第 14 章](14-middleware-production.md)）
4. 事件驱动（outbox 模式）
5. 多服务与网关（服务发现见[第 15 章](15-cluster-distributed.md)）

## 13.6 安全清单（后端必看）

- 输入校验与输出编码
- 最小权限 DB 账号
- 密钥不进仓库（环境变量/密钥管理）
- 依赖审计：`cargo audit`
- 限流、超时、体积限制
- 认证鉴权与 CSRF/CORS 策略

## 13.7 建议阅读源码

- `clap`：API 设计
- `serde`：零成本抽象
- `axum` / `tower`：中间件思想
- `sqlx`：宏与运行时检查
- `ripgrep`：工程化与性能

## 13.8 30 天巩固计划

| 周 | 目标 |
|----|------|
| 1 | 复盘 02/04/06，默写所有权与错误处理 |
| 2 | 给 `todo_api` 加分页 + 鉴权 |
| 3 | 引入 postgres 与 docker-compose（跟着[第 14 章](14-middleware-production.md)做） |
| 4 | 压测 + 优化 + 写 README 架构说明 |

## 结语

如果你完整跟完第 0–12 章并独立扩展 `todo_api`，你已经具备：

- 读懂大多数 Rust 后端项目
- 从 0 搭一个可维护 API 服务
- 继续向平台工程/系统编程分支发展的底座

接下来最重要的不是再囤教程，而是：**选一个真实小产品，上线它**。

## 本章小结

- `gen` blocks、`try` 表达式等特性 1.97 仍未稳定，别写进业务代码，升级工具链时看 Release Notes 回归
- 宏先会用再写，优先现成生态；过程宏是 derive/属性宏的底层机制
- FFI（bindgen/cxx/PyO3）与 WASM（wasm-bindgen）是 Rust 向外辐射的两条路
- 性能工程从测量开始：flamegraph、减少分配、异步背压、数据库索引与连接池
- 架构演进有顺序：模块化单体 → 缓存/读写分离 → 后台任务 → 事件驱动 → 多服务
- 后端安全清单：输入校验、最小权限、密钥管理、`cargo audit`、限流超时
- 读优秀源码（serde/axum/tower/ripgrep）比囤教程更涨功力

**自测清单**

- [ ] 我能说出至少三个进阶方向和各自的代表 crate
- [ ] 我能解释为什么未稳定特性不该进业务代码
- [ ] 我能列出后端上线前的安全检查项
- [ ] 我能说出从单体到多服务的演进步骤及各步解决什么问题
- [ ] 我能为自己定一个 30 天可执行的巩固计划

---

导航：[上一章](12-todo-api-project.md) | [返回目录](../README.md) | [下一章](14-middleware-production.md)
