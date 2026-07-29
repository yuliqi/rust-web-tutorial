# 附录 E：Rust 1.97.1 工具链速查

本附录对应当前教程基准环境。

## 已验证版本指纹

```text
rustc 1.97.1 (8bab26f4f 2026-07-14)
cargo 1.97.1 (c980f4866 2026-06-30)
rustup 1.29.0 (28d1352db 2026-03-05)
```

## 一键对齐

```bash
rustup toolchain install 1.97.1
rustup component add rustfmt clippy rust-analyzer --toolchain 1.97.1

# 在本仓库中：
# rust-toolchain.toml 已指定 channel = "1.97.1"
rustc -vV
cargo -vV
rustup --version
```

期望关键字段：

```bash
rustc -vV | rg 'release:|commit-hash:'
# release: 1.97.1
# commit-hash: 8bab26f4f...
```

## Edition 与 MSRV

```toml
[package]
edition = "2024"
rust-version = "1.97"
```

- 推荐稳定版 `1.97.1`（不要混用过旧 nightly 组件）
- 新项目：`edition = "2024"`
- 需要实验特性再考虑 beta/nightly，并与稳定版示例隔离

## 本教程默认依赖组合（Web）

| Crate | 版本策略 | 用途 |
|-------|----------|------|
| `tokio` | `1` | 异步运行时 |
| `axum` | `0.8` | HTTP 框架 |
| `tower` / `tower-http` | `0.5` / `0.6` | 中间件 |
| `serde` / `serde_json` | `1` | JSON |
| `sqlx` | `0.8` + `runtime-tokio,sqlite` | 数据库 |
| `reqwest` | `0.12` + `json,rustls-tls` | HTTP 客户端 |
| `tracing` / `tracing-subscriber` | `0.1` / `0.3` | 可观测 |
| `thiserror` / `anyhow` | `2` / `1` | 错误处理 |

> 升级提示：crates.io 可能已有 `reqwest 0.13`、`sqlx 0.9`、`tower-http 0.7` 等更新。  
> 教程默认优先 **可复现 + 与 axum 0.8 兼容**，升级请单独开分支并用 `cargo test --workspace` 验证。

## 高频命令（Cargo 1.97）

```bash
cargo new app
cargo add serde --features derive
cargo add axum tokio --features tokio/full
cargo add sqlx --features runtime-tokio,sqlite
cargo check
cargo test --workspace
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo build --release
cargo tree -i axum
cargo info axum
```

## 实用插件（可选）

```bash
cargo install cargo-nextest
cargo install cargo-watch
cargo install cargo-audit
cargo install sqlx-cli --features sqlite
cargo install flamegraph
```

## 调试与日志

```bash
RUST_BACKTRACE=1 cargo run -p todo_api
RUST_LOG=info,tower_http=debug,todo_api=debug cargo run -p todo_api
```

## Workspace 建议（1.97 实践）

```toml
[workspace]
resolver = "2"
members = ["todo_api", "/* more */"]

[workspace.package]
edition = "2024"
rust-version = "1.97"

[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
```

成员 crate 写：

```toml
[package]
name = "todo_api"
version.workspace = true
edition.workspace = true
# 若成员未继承 package 字段，请显式写 edition/rust-version
```

## 版本锁定建议

- **应用 / 教程示例**：提交 `Cargo.lock`
- **纯库**：按团队策略；发布到 crates.io 时 lock 不进入用户构建
- **CI**：用 `rust-toolchain.toml` 或 `dtolnay/rust-toolchain@1.97.1` 钉版本

## 常见问题

### 1) 为什么我 rustc 是 1.9x，但文档写 1.97.1？

因为本教程按 1.97.1 校验。低版本可能缺 lint/库支持。执行：

```bash
rustup toolchain install 1.97.1
```

### 2) `rust-analyzer` 报错与命令行不一致？

通常是 IDE 没用到 `rust-toolchain.toml`。重启 RA，并确认工作区根目录正确。

### 3) 依赖编译失败？

```bash
rustc --version   # 先确认 1.97.1
cargo clean
cargo test --workspace
```

## 变更记录（文档）

| 日期 | 基准工具链 | 说明 |
|------|------------|------|
| 2026-07 | rustc/cargo 1.97.1，rustup 1.29.0 | 文档与示例对齐最新稳定工具链 |

## 相关

- 语言/标准库新特性映射： [附录 F](F-rust-197-features.md)
