# 第 1 章：语言基础

> 示例：`examples/ch01_basics`

## 学习目标

- 掌握变量、类型、函数与控制流
- 理解「表达式优先」的语法风格
- 能写小型纯逻辑程序

## 1.1 变量与可变性

```rust
let x = 5;          // 不可变
let mut y = 5;      // 可变
y += 1;

const MAX_POINTS: u32 = 100_000; // 常量：必须标注类型，编译期可知
```

**Shadowing**（重影）允许复用名字并改变类型：

```rust
let spaces = "   ";
let spaces = spaces.len(); // 现在是 usize
```

Web 场景里，请求处理函数中大量使用不可变绑定；需要改状态时再 `mut`。

## 1.2 基本类型

### 标量

- 整数：`i32`（默认）、`u64`、`usize`…
- 浮点：`f64`（默认）、`f32`
- 布尔：`bool`
- 字符：`char`（Unicode 标量值，4 字节）

### 复合

```rust
let tup: (i32, f64, char) = (500, 6.4, '中');
let (a, b, c) = tup;

let arr: [i32; 3] = [1, 2, 3];
let first = arr[0];
```

数组长度固定；动态列表用 `Vec`（第 5 章）。

## 1.3 函数与表达式

Rust 里很多语法结构是**表达式**，能返回值：

```rust
fn abs_diff(a: i32, b: i32) -> i32 {
    if a > b { a - b } else { b - a }
}

fn describe(n: i32) -> &'static str {
    match n {
        0 => "zero",
        1..=9 => "single digit",
        _ => "big",
    }
}
```

注意：

- 函数签名必须标注参数/返回类型
- 最后一行无分号 = 返回该表达式
- `return` 可提前返回

## 1.4 控制流

```rust
// loop 可带值 break
let mut n = 0;
let doubled = loop {
    n += 1;
    if n == 10 {
        break n * 2;
    }
};

for i in 0..5 {
    println!("{i}");
}

for item in ["a", "b", "c"] {
    println!("{item}");
}
```

`match` 必须穷尽；这是后端里解析枚举状态机的主力。

### 独占范围模式（可读性更好）

```rust
fn bucket(n: u8) -> &'static str {
    match n {
        0 => "zero",
        1..10 => "single digit (1-9)", // 1 到 9，不含 10
        10..=99 => "two digits",
        _ => "big",
    }
}
```

### let chains（Edition 2024 / 1.88+，1.97 完全可用）

把多层 `if let` 拍平，后端参数校验非常常见：

```rust
fn can_publish(title: Option<&str>, user_active: bool) -> bool {
    if let Some(t) = title && !t.is_empty() && user_active {
        true
    } else {
        false
    }
}
```

## 1.5 字符串初识

```rust
let s1: &str = "hello";      // 字符串切片（通常只读视图）
let s2: String = String::from("hello"); // 可增长的堆字符串
let s3 = format!("{s1}, world");
```

规则直觉：

- API 入参优先 `&str`
- 需要拥有/修改时用 `String`
- JSON body 反序列化常见 `String` 字段

## 1.6 打印与调试

```rust
println!("user={} id={}", "neo", 1);
eprintln!("warn: something off");
dbg!(1 + 2); // 开发期调试，打印表达式与位置
```

生产日志后面用 `tracing`，不要只靠 `println!`。

## 1.7 与 Web 的关系

即使写 HTTP handler，底层仍是这些基础：

```rust
fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Unknown",
    }
}
```

## 1.8 面向 Rust 1.97 / edition 2024 的小提示

1. 新项目直接写 `edition = "2024"` + `rust-version = "1.97"`。
2. 优先用 `match` / `let-else` / **let chains** / `Option`/`Result`，少写哨兵值。
3. 调试先 `cargo check`，再 `cargo clippy`；1.97 的诊断信息通常已经足够定位。
4. 格式化与 CI 对齐：`cargo fmt --check` + `cargo clippy --all-targets`。
5. 文档代码块可用 `cargo test` 的 doc-test 守真（见第 8 章）。
6. **先别用**尚未稳定的 `gen/yield`、`try { ... }` 表达式（1.97 仍 experimental）。
7. 完整特性映射见 [附录 F](../appendix/F-rust-197-features.md)。

## 运行示例

```bash
cd examples
cargo run -p ch01_basics
```

## 本章小结

- `let` 默认不可变，`mut` 显式可变；shadowing 允许复用名字并改变类型
- 标量类型（整数、浮点、`bool`、`char`）与复合类型（元组、数组）；数组定长，动态列表用 `Vec`
- 函数签名必须标注类型；`if`/`match`/块都是表达式，末行无分号即返回值
- `match` 必须穷尽，支持 `1..10`（独占）与 `1..=9`（含端点）范围模式
- let chains（Edition 2024）把多层 `if let` 拍平，参数校验常用
- `&str` 是只读切片，`String` 是可增长堆字符串；API 入参优先 `&str`
- 开发期用 `dbg!`/`println!` 调试，生产日志交给 `tracing`

**自测清单**

- [ ] 我能解释 `let`、`mut` 与 shadowing 的区别
- [ ] 我能说出「表达式优先」意味着什么（末行无分号即返回值）
- [ ] 我能写出穷尽的 `match`，包括范围模式
- [ ] 我能用 `loop` 带值 `break`、用 `for` 遍历范围与数组
- [ ] 我能说出 `&str` 与 `String` 各自的适用场景

## 练习

1. 写 `celsius_to_fahrenheit` 与反向函数。
2. 实现猜数字（固定答案版即可）：循环读入、比较大小。
3. 写函数统计字符串里元音字母数量（先用 `char` 遍历）。

**动手做题**：打开 `examples/ch01_basics/src/exercises.rs`，把 `todo!()` 换成你的实现（`fahrenheit_to_celsius` 华氏转摄氏、`guess_hint` 猜数字提示、`fizzbuzz`）：

```bash
cd examples
cargo test -p ch01_basics -- --ignored   # 自动判题（没做完时失败是预期的）
```

全部通过后对照参考答案 `src/solutions.rs`（默认随 `cargo test` 验证，答案保证可靠）。

---

导航：[上一章](00-environment.md) | [返回目录](../README.md) | [下一章](02-ownership.md)
