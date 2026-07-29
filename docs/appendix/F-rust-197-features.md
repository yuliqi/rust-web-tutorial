# 附录 F：Rust 1.97 时代新特性清单（教程映射）

本附录基于本机 `rustc 1.97.1` 附带的 `releases.md` 与实测编译结果。  
目标：回答「教程有没有跟上新特性」——**版本号已对齐，下列特性现已补入正文/示例。**

## 学习优先级（Web 后端）

| 优先级 | 特性 | 稳定大致版本 | 教程位置 | 状态 |
|--------|------|--------------|----------|------|
| P0 | Edition 2024 | 1.85 | 第 0 章 | 已用 |
| P0 | `let-else` | 更早 | 第 3 章 / 示例 | 已用 |
| P0 | **let chains** | 1.88（2024 edition） | 第 1/3 章 + `ch01`/`ch03` | **已补** |
| P0 | **`if let` match guard** | 1.95 | 第 3 章 + `ch03` | **已补** |
| P0 | `Option::is_none_or` / `is_some_and` | 1.70/1.82 一带 | 第 5 章 + `ch05` | 已用并说明 |
| P1 | **async closures** | 1.85 | 第 10 章 + `ch10` | **已补** |
| P1 | **`std::sync::LazyLock`** | 1.80+（后续持续增强） | 第 9 章 + `ch09` | **已补** |
| P1 | RPIT + **`use<..>` precise capturing** | 1.82/1.87 | 第 6 章 + `ch06` | **已补** |
| P2 | exclusive range patterns `a..b` | 更早稳定完善 | 第 1 章 | **已补** |
| P2 | trait object upcasting | 1.86 | 第 6 章（概念） | **已补** |
| P2 | `offset_of!` | 1.77+（后续收紧检查） | 第 13 章/附录 | 概念提及 |
| 暂不教 | `gen` blocks / `yield` | 1.97 仍 experimental | — | 明确标注未稳 |
| 暂不教 | `try { ... }` 表达式 | 1.97 仍 experimental | — | 明确标注未稳 |

## 1.97.0/1.97.1 本身更偏「硬化」

对入门后端影响较小（多为 lint/目标特性/边界语义）。学习上继续以：

- Edition 2024 默认语义
- let chains / if-let guards
- async + tokio/axum 生态

为主即可。

## 实测（本机 1.97.1, edition 2024）

| 语法 | 结果 |
|------|------|
| `if let ... && ...` let chains | 通过 |
| `match` 臂 `if let ...` guard | 通过 |
| `async \|x\| ...` async closure | 通过 |
| `impl Trait + use<'_>` | 通过 |
| `LazyLock` | 通过 |
| `gen { yield ... }` | **失败（experimental）** |
| `try { ... }` | **失败（experimental）** |

## 与旧教程写法的对照

```rust
// 旧：嵌套 if let
if let Some(user) = find_user(id) {
    if user.active {
        // ...
    }
}

// 新（推荐，1.88+ / edition 2024）：let chains
if let Some(user) = find_user(id) && user.active {
    // ...
}
```

```rust
// 旧：guard 里只能用布尔表达式
match status {
    Some(code) if code == 200 => {}
    _ => {}
}

// 新（1.95+）：if let guard
match payload {
    Some(raw) if let Ok(code) = raw.parse::<u16>() && code == 200 => {}
    _ => {}
}
```

```rust
// 旧：lazy_static! / once_cell::sync::Lazy
// 新：标准库 LazyLock
use std::sync::LazyLock;
static CONFIG: LazyLock<String> = LazyLock::new(|| std::env::var("APP_ENV").unwrap_or_else(|_| "dev".into()));
```

## 明确不要写进生产示例的「看起来新」的东西

1. `gen` / `yield`（尚未稳定）
2. `try` 块表达式（尚未稳定）
3. 依赖 nightly only 的 async gen / yeet 等

## 回归命令

```bash
cd examples
cargo test --workspace
cargo run -p ch01_basics
cargo run -p ch03_structs_enums
cargo run -p ch06_traits
cargo run -p ch09_smart_pointers
cargo run -p ch10_async
```
