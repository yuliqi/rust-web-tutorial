# 第 4 章：错误处理

> 示例：`examples/ch04_errors`

## 学习目标

- 区分 `panic` 与可恢复错误
- 熟练 `Result` 与 `?`
- 会用 `thiserror` / `anyhow` 组织错误

## 4.1 两种失败

| 类型 | 方式 | 场景 |
|------|------|------|
| 不可恢复 | `panic!` | 程序 bug、不变量被破坏 |
| 可恢复 | `Result` | 用户输入、IO、网络、DB |

库代码尽量别乱 `panic`；边界处校验失败应返回错误。

## 4.2 `?` 运算符

```rust
use std::fs;

fn read_config(path: &str) -> Result<String, std::io::Error> {
    let content = fs::read_to_string(path)?; // 出错自动 return Err
    Ok(content)
}
```

`?` 还会尝试 `From` 转换错误类型。

## 4.3 自定义错误（库友好）：`thiserror`

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid format: {0}")]
    Format(String),
}
```

适合：可被调用方 `match` 的错误枚举。

## 4.4 应用层错误：`anyhow`

```rust
use anyhow::{Context, Result, bail};

fn load() -> Result<String> {
    let raw = std::fs::read_to_string("app.toml")
        .context("read app.toml failed")?;
    if raw.is_empty() {
        bail!("app.toml is empty");
    }
    Ok(raw)
}
```

适合：二进制、handler 顶层、快速迭代。

经验：

- **library crate** → `thiserror`
- **application crate** → `anyhow`（或统一 `AppError`）

## 4.5 映射到 HTTP

```rust
enum AppError {
    NotFound,
    BadRequest(String),
    Internal(anyhow::Error),
}

fn to_status(err: &AppError) -> u16 {
    match err {
        AppError::NotFound => 404,
        AppError::BadRequest(_) => 400,
        AppError::Internal(_) => 500,
    }
}
```

第 12 章会把它做成 `axum` 的 `IntoResponse`。

## 4.6 日志与上下文

错误不只是返回，还要可观测：

```rust
tracing::warn!(path, "config missing, fallback to default");
tracing::error!(error = %err, "db query failed");
```

原则：

1. 在边界加 context
2. 不要吞错
3. 对用户返回安全信息，对日志保留细节

## 运行示例

```bash
cargo run -p ch04_errors
```

## 本章小结

- 两种失败：程序 bug 用 `panic!`（不可恢复），用户输入/IO/网络/DB 用 `Result`（可恢复）
- `?` 出错自动提前返回 `Err`，并借 `From` 自动转换错误类型
- 库 crate 用 `thiserror` 定义可被调用方 `match` 的错误枚举，`#[from]` 接管转换
- 应用 crate 用 `anyhow` + `Context`/`bail!` 快速组织错误并附加上下文
- 错误最终映射到 HTTP 状态码：`NotFound→404`、`BadRequest→400`、`Internal→500`
- 可观测原则：边界加 context、不吞错、对用户返回安全信息而日志保留细节

**自测清单**

- [ ] 我能判断一段代码该 `panic` 还是返回 `Result`
- [ ] 我能解释 `?` 的完整行为，包括 `From` 错误转换
- [ ] 我能说出 `thiserror` 与 `anyhow` 的分工（库 vs 应用）
- [ ] 我能用 `Context` 为错误附加上下文信息
- [ ] 我能把自定义错误枚举映射到 HTTP 状态码

## 练习

1. 解析 `key=value` 配置行，失败返回自定义错误。
2. 把 IO 错误与格式错误统一进一个 `thiserror` 枚举。
3. 用 `anyhow::Context` 为错误附加文件路径。

**动手做题**：打开 `examples/ch04_errors/src/exercises.rs`，把 `todo!()` 换成你的实现（`parse_kv`、`parse_port`、`get_port`：自定义错误 + `?` 传播链）：

```bash
cd examples
cargo test -p ch04_errors -- --ignored   # 自动判题（没做完时失败是预期的）
```

全部通过后对照参考答案 `src/solutions.rs`（默认随 `cargo test` 验证，答案保证可靠）。

---

导航：[上一章](03-structs-enums.md) | [返回目录](../README.md) | [下一章](05-collections-iterators.md)
