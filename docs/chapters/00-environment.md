# 第 0 章：环境与工具链

## 学习目标

- 安装并验证 **Rust 1.97.1** 工具链
- 理解 `rustc` / `cargo` / `rustup` 的分工
- 创建并运行第一个 Cargo 项目
- 认识 Edition 2024、格式化与静态检查
- 用 `rust-toolchain.toml` 固定团队版本

## 0.1 工具链角色

| 工具 | 本教程版本 | 作用 |
|------|------------|------|
| `rustup` | **1.29.0** | 安装/切换工具链、组件、目标平台 |
| `rustc` | **1.97.1** | 编译器 |
| `cargo` | **1.97.1** | 包管理 + 构建 + 测试 + 运行入口 |
| `rustfmt` | 随工具链 | 统一格式 |
| `clippy` | 随工具链 | 额外 lint，接近「资深同事 code review」 |
| `rust-analyzer` | 组件/编辑器 | IDE 语义引擎 |

日常你几乎只直接碰 `cargo`。

完整版本指纹：

```text
rustc 1.97.1 (8bab26f4f 2026-07-14)
cargo 1.97.1 (c980f4866 2026-06-30)
rustup 1.29.0 (28d1352db 2026-03-05)
```

## 0.2 安装与验证

官网安装（若尚未安装）：

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

安装本教程指定工具链：

```bash
rustup toolchain install 1.97.1
rustup component add rustfmt clippy rust-analyzer --toolchain 1.97.1
```

本仓库根目录已提供：

```toml
# rust-toolchain.toml
[toolchain]
channel = "1.97.1"
components = ["rustfmt", "clippy", "rust-analyzer"]
profile = "default"
```

进入仓库后验证：

```bash
rustc --version
cargo --version
rustup --version
rustup show
```

期望看到 `1.97.1` / `1.29.0`。若版本不对：

```bash
rustup toolchain install 1.97.1
cd /path/to/rust-web-tutorial
rustc -vV   # 应显示 release: 1.97.1
```

## 0.3 第一个项目

```bash
cargo new hello_cargo
cd hello_cargo
cargo run
```

生成结构：

```text
hello_cargo/
├── Cargo.toml      # 清单：包名、版本、依赖、edition、rust-version
└── src/main.rs     # 二进制入口
```

`Cargo.toml` 关键字段（1.97 推荐写法）：

```toml
[package]
name = "hello_cargo"
version = "0.1.0"
edition = "2024"
rust-version = "1.97"

[dependencies]
# 之后在这里加 crates.io 依赖
```

说明：

- `edition`：语言表面语法版本
- `rust-version`：MSRV（最低支持的编译器），Cargo 会在版本过低时给出更明确提示

## 0.4 Edition 2024（本教程默认）

Edition 是**语言表面语法/默认语义**的版本通道，不是编译器大版本。

- 同一 `rustc 1.97.1` 可编译不同 edition 的 crate
- 升级 edition 可用 `cargo fix --edition` 辅助
- 本教程统一 `edition = "2024"`

对 Web 后端学习影响最大的几点：

1. 更现代的模块/prelude 默认行为
2. RPIT（返回位置 `impl Trait`）生命周期捕获规则更直观，可用 `use<..>` 精确捕获
3. **let chains**（`if let ... && ...`）成为日常写法
4. `if let` 临时值作用域更安全（Edition 2024）
5. 与 1.97 工具链、最新 `rust-analyzer` 体验一致

完整清单见 [附录 F](../appendix/F-rust-197-features.md)。

## 0.5 日常命令速记（Cargo 1.97）

```bash
cargo new app            # 新建二进制
cargo new --lib mylib    # 新建库
cargo build              # 调试构建
cargo build --release    # 优化构建
cargo run                # 构建并运行
cargo test               # 测试
cargo fmt                # 格式化
cargo clippy             # lint
cargo doc --open         # 生成并打开文档
cargo tree               # 依赖树
cargo info serde         # 查看 crate 信息（本地/索引）
cargo update             # 在兼容范围内更新依赖
```

## 0.6 推荐编辑器设置

最少开启：

1. `rust-analyzer`（补全、跳转、内联错误）
2. 保存时 `rustfmt`
3. 工作区打开 `rust-web-tutorial` 根或 `examples` 根（能识别 `Cargo.toml` / `rust-toolchain.toml`）

可选：`CodeLLDB` 断点调试。

## 0.7 Web 后端学习视角下的工具

后面章节会反复用到：

- `tokio`：异步运行时
- `axum`：HTTP 框架
- `serde`：序列化
- `sqlx`：异步 SQL
- `tracing`：结构化日志
- `clap`：CLI（本地工具/管理脚本）

先把 1.97.1 工具链跑顺，再学语言，会轻松很多。

## 0.8 版本策略（团队项目建议）

| 文件 | 作用 |
|------|------|
| `rust-toolchain.toml` | 钉死编译器版本（本教程：1.97.1） |
| `Cargo.toml` 的 `rust-version` | 声明 MSRV |
| `Cargo.lock` | 锁定依赖精确版本（应用项目应提交） |

## 本章小结

- `rustup` 装/切工具链，`rustc` 编译，`cargo` 是日常唯一入口（构建、测试、运行、fmt、clippy）
- 本教程钉死 rustc/cargo **1.97.1**、rustup **1.29.0**，用 `rust-toolchain.toml` 固定团队版本
- `cargo new` 生成 `Cargo.toml` + `src/main.rs`；关键字段 `edition = "2024"`、`rust-version`（MSRV）
- Edition 是语言表面语法的版本通道，不是编译器大版本；同一 rustc 可编译不同 edition
- 日常命令：`cargo build / run / test / fmt / clippy / doc / tree`
- 编辑器最少配置：`rust-analyzer` + 保存时 `rustfmt`
- 应用项目应提交 `Cargo.lock`，锁定依赖精确版本

**自测清单**

- [ ] 我能说出 `rustup` / `rustc` / `cargo` 各自的职责
- [ ] 我能用 `cargo new` 创建项目并 `cargo run` 跑起来
- [ ] 我能解释 edition 与 `rust-version`（MSRV）分别声明什么
- [ ] 我能说出 `rust-toolchain.toml` 与 `Cargo.lock` 各锁定什么
- [ ] 我能对项目执行 `cargo fmt` 与 `cargo clippy` 并读懂输出

## 练习

1. 确认本机输出与基准工具链一致（`rustc`/`cargo`/`rustup`）。
2. 用 `cargo new` 创建项目，写入 `edition = "2024"` 与 `rust-version = "1.97"`。
3. 对项目执行 `cargo fmt`、`cargo clippy`、`cargo doc --open`。

---

导航：[返回目录](../README.md) | [下一章](01-language-basics.md)
