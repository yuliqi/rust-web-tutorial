<p align="center">
  <img src="https://rustacean.net/assets/rustacean-orig-noshadow.svg" width="120" alt="Rust">
</p>

<h1 align="center">现代 Rust 实战教程</h1>

<p align="center">
  <strong>从语言基础到可上线的 Web API — 面向 Rust 1.97.1 / edition 2024</strong>
</p>

<p align="center">
  <a href="https://rust-web-tutorial.llapp.com">📖 在线阅读</a>
  ·
  <a href="#quick-start">🚀 快速开始</a>
  ·
  <a href="#structure">📁 仓库结构</a>
  ·
  <a href="#license">📄 许可</a>
</p>

---

## 简介

一个系统化的 Rust 学习路径，以 **Web 后端**为主线，覆盖 29 章 + 8 个附录，配套完整的可运行示例代码。

| 维度 | 内容 |
|------|------|
| **难度** | 入门 → 进阶 |
| **技术栈** | `tokio` + `axum` + `serde` + `sqlx` + `tracing` |
| **形式** | 分章 Markdown 教程 + Cargo workspace 示例工程 |
| **工具链** | Rust 1.97.1（仓库自带 `rust-toolchain.toml`，自动生效） |
| **文档站** | VitePress 构建，部署于 Vercel |

## 学习路线（五篇）

| 篇 | 章节 | 目标 |
|----|------|------|
| 第一篇 · 语言基础 | 第 0–10 章 | 所有权、类型系统、错误处理、并发异步 |
| 第二篇 · Web 后端主线 | 第 11–13 章 | 从语言到可上线的异步 Web API，完成 `todo_api` |
| 第三篇 · 生产基建 | 第 14–16 章 | 数据库、缓存、消息队列、集群、容器化部署 |
| 第四篇 · SaaS 与安全 | 第 17–20 章 | 多租户、鉴权、配额计费、传输与存储加密 |
| 第五篇 · 平台化进阶 | 第 21–29 章 | 定时任务、实时通信、Web 终端、证书、监控、插件等 |

## 快速开始

```bash
# 克隆仓库
git clone https://github.com/yuliqi/rust-web-tutorial.git
cd rust-web-tutorial

# 工具链自动切换至 1.97.1（由 rust-toolchain.toml 管理）
rustc --version

# 运行语言基础示例（无需 Docker）
cd examples && cargo test --workspace

# 运行生产示例（多数需 Docker）
cd examples-middleware && docker compose up -d && cargo test --workspace
```

## 文档

- **在线阅读**：[rust-web-tutorial.llapp.com](https://rust-web-tutorial.llapp.com)
- **本地预览**：`pnpm install && pnpm docs:dev`（需 Node.js）
- **语言章练习**：每个 crate 带 `src/exercises.rs`，`cargo test -p chXX -- --ignored` 自动判题

## 项目结构

```
rust-web-tutorial/
├── docs/                    # 教程文档（VitePress 源）
│   ├── chapters/            #  29 章正文
│   ├── appendix/            #  8 个附录
│   └── README.md            #  文档站首页（含完整学习路线表）
├── examples/                # 第 1–11 章：语言基础可运行示例（Cargo workspace）
├── examples-middleware/     # 第 14–29 章：生产/平台示例（独立 workspace，多数需 Docker）
├── rust-toolchain.toml      # 钉死 Rust 1.97.1
└── .vitepress/              # VitePress 配置
```

## 贡献

欢迎提交 Issue 和 PR。请确保示例在 Rust 1.97.1 下可编译通过。

## 许可

本项目采用 [MIT](LICENSE) 许可证。
