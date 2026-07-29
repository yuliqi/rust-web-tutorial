# 第 8 章：测试与质量

> 示例：`examples/ch08_testing`

## 学习目标

- 写单元测试、集成测试、文档测试
- 使用 `clippy` / `rustfmt` 守门
- 为 Web 服务建立可测结构

## 8.1 单元测试

```rust
fn add(a: i32, b: i32) -> i32 { a + b }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_works() {
        assert_eq!(add(2, 2), 4);
    }

    #[test]
    #[should_panic]
    fn boom() {
        panic!("expected");
    }
}
```

## 8.2 集成测试

放在 `tests/*.rs`，只能访问 **公共 API**：

```text
ch08_testing/
├── src/lib.rs
└── tests/
    └── integration.rs
```

## 8.3 文档测试

```rust
/// 加法。
///
/// ```
/// assert_eq!(ch08_testing::add(1, 2), 3);
/// ```
pub fn add(a: i32, b: i32) -> i32 { a + b }
```

`cargo test` 会编译运行文档代码块。

## 8.4 断言工具

```rust
assert!(ok);
assert_eq!(left, right);
assert_ne!(a, b);
approx? // 浮点可用第三方
```

异步测试：

```rust
#[tokio::test]
async fn health_ok() {
    // ...
}
```

## 8.5 质量门禁

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

可放进 CI。建议本地 git hook 至少跑 `fmt`。

## 8.6 让 Web 代码可测

技巧：

1. 业务逻辑纯函数化（不直接碰 HTTP）
2. 仓库用 Trait，测试注入内存实现
3. `axum` 可用 `oneshot` 做路由级测试
4. 数据库测试用事务回滚或临时 sqlite 文件

```rust
pub fn create_todo_title(raw: &str) -> Result<String, String> {
    let title = raw.trim();
    if title.is_empty() {
        return Err("title required".into());
    }
    if title.len() > 100 {
        return Err("title too long".into());
    }
    Ok(title.to_string())
}
```

先测这种函数，再测 handler。

## 运行示例

```bash
cargo test -p ch08_testing
```

## 本章小结

- 单元测试写在 `#[cfg(test)] mod tests` 里，与被测代码同文件
- 集成测试放在 `tests/*.rs`，只能访问 crate 的公共 API
- 文档测试让示例代码随 `cargo test` 编译运行，文档不会过期
- 常用断言：`assert!` / `assert_eq!` / `assert_ne!`；异步测试用 `#[tokio::test]`
- 质量门禁三件套：`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test`
- 让 Web 代码可测的关键：业务逻辑纯函数化、仓库用 Trait 注入内存实现

**自测清单**

- [ ] 我能说出单元测试、集成测试、文档测试各放在哪里
- [ ] 我能解释为什么集成测试只能访问公共 API
- [ ] 我能写出一个 `#[should_panic]` 测试和一个异步测试
- [ ] 我能列出适合放进 CI 的质量门禁命令
- [ ] 我能说出至少两种让 axum 服务可测的结构技巧

## 练习

1. 给标题校验补齐边界测试。
2. 为内存仓库写 get/create/delete 测试。
3. 故意触发 clippy 警告并修复。

改完后可用示例 crate 自带的测试验证：`cargo test -p ch08_testing`。

---

导航：[上一章](07-modules-workspace.md) | [返回目录](../README.md) | [下一章](09-smart-pointers.md)
