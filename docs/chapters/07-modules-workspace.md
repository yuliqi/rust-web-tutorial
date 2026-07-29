# 第 7 章：模块与工程组织

> 示例：`examples/ch07_modules_lib` + `examples/ch07_modules_bin`

## 学习目标

- 组织模块树与可见性
- 区分 package / crate / module
- 使用 workspace 管理多 crate

## 7.1 模块树

```rust
// src/lib.rs
pub mod models;
pub mod services;
mod internal; // 私有模块

pub use models::Todo; // 重导出，简化外部路径
```

文件映射：

```text
src/
├── lib.rs
├── models.rs
├── models/
│   └── todo.rs      # 若 models 是目录
├── services.rs
└── internal.rs
```

## 7.2 可见性

- 默认私有
- `pub`：对外可见
- `pub(crate)`：crate 内可见
- `pub(super)`：父模块可见

后端建议：

- `models` / `dto` 公开
- 内部工具 `pub(crate)`
- 真正私有细节不 `pub`

## 7.3 Package 与 Crate

- **Package**：一个 `Cargo.toml` 管理单元
- **Crate**：编译单元（`lib` 或 `bin`）
- 一个 package 可有 1 个 lib + 多个 bin

## 7.4 Workspace

本教程 `examples/Cargo.toml`：

```toml
[workspace]
members = [
  "ch01_basics",
  # ...
  "todo_api",
]
resolver = "2"
```

好处：

- 统一依赖版本（可用 `[workspace.dependencies]`）
- 一次 `cargo test` 全跑
- 库/服务拆分清晰

## 7.5 推荐 Web 项目布局

```text
todo_api/
├── Cargo.toml
└── src/
    ├── main.rs          # 启动、注入、监听
    ├── lib.rs           # 便于测试
    ├── config.rs
    ├── error.rs
    ├── routes/
    │   ├── mod.rs
    │   └── todos.rs
    ├── services/
    ├── repositories/
    └── models/
```

原则：**main 很瘦，业务在 lib**。

## 7.6 Feature flags

```toml
[features]
default = ["sqlite"]
sqlite = ["sqlx/sqlite"]
postgres = ["sqlx/postgres"]
```

可用于可选能力、降依赖、分环境。

## 运行示例

```bash
cargo run -p ch07_modules_bin
cargo test -p ch07_modules_lib
```

## 本章小结

- 模块树在 `lib.rs`/`main.rs` 中用 `mod` 声明，文件与目录一一映射
- 可见性默认私有，按需用 `pub` / `pub(crate)` / `pub(super)` 收紧暴露面
- `pub use` 重导出可以简化外部调用路径
- Package 是 `Cargo.toml` 管理单元，Crate 是编译单元；一个 package 可含 1 个 lib + 多个 bin
- Workspace 统一依赖版本、一次跑全部测试，适合库/服务拆分
- Web 项目遵循「main 很瘦，业务在 lib」，routes/services/repositories/models 分层
- Feature flags 用于可选能力与分环境构建

**自测清单**

- [ ] 我能说出 package、crate、module 三者的区别
- [ ] 我能解释 `pub(crate)` 和 `pub` 的适用场景差异
- [ ] 我能用 `pub use` 为外部用户提供简短路径
- [ ] 我能画出一个 axum 项目的推荐目录布局
- [ ] 我能说出 workspace 相比多个独立仓库的好处

## 练习

1. 把 `Todo` 模型与 `MemoryRepo` 分到不同模块。
2. 在 bin 中只依赖 lib 的公开 API。
3. 增加 `pub use` 重导出，让外部写 `use ch07_modules_lib::Todo`。

改完后可用示例 crate 自带的测试验证：`cargo test -p ch07_modules_lib`。

---

导航：[上一章](06-generics-traits.md) | [返回目录](../README.md) | [下一章](08-testing-quality.md)
