# 附录 B：与其他语言对照

| 概念 | Rust | Go | Java | Python |
|------|------|----|------|--------|
| 空值 | `Option<T>` | 多返回/`nil` | `Optional`/nullable | `None` |
| 错误 | `Result<T,E>` | `error` 多返回 | 异常 | 异常 |
| 并发 | 线程 + async | goroutine | 线程/虚拟线程 | asyncio/线程 |
| 包管理 | cargo | go mod | maven/gradle | pip/uv/poetry |
| 接口 | Trait | interface | interface | Protocol |
| 泛型 | 是 | 是 | 是 | 类型提示 |
| GC | 无（所有权） | 有 | 有 | 有 |
| 默认不可变 | 是 | 否 | 否 | 否 |

## 从其他语言迁移提示

- **从 Go 来**：少用「到处共享指针」思维；错误更枚举化；async 不是 goroutine。
- **从 Java 来**：没有传统继承树；组合 + Trait；没有隐式 null。
- **从 Python 来**：类型与所有权是强制的；先过编译器再谈动态灵活。
- **从 C++ 来**：默认更安全；RAII 很像；模板错误信息通常更友好（但仍可能长）。
