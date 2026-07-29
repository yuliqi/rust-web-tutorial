# 第 2 章：所有权（Rust 之魂）

> 示例：`examples/ch02_ownership`

## 学习目标

- 理解 Move / Borrow / Copy
- 能解释为什么某些代码不能编译
- 在函数边界正确传递数据

## 2.1 为什么需要所有权

很多语言靠 GC 或手动 `free`。Rust 用编译期规则保证：

1. 每个值有且只有一个所有者
2. 所有者离开作用域时值被释放
3. 借用必须始终有效

这换来：**无 GC 停顿 + 内存安全**。

## 2.2 Move 语义

```rust
let s1 = String::from("hello");
let s2 = s1; // s1 被 move，不能再使用
// println!("{s1}"); // ❌
println!("{s2}");    // ✅
```

`String` 管理堆内存，赋值默认 **move**，避免双重释放。

像 `i32`、`bool`、`char`、小元组等实现了 `Copy` 的类型会复制而非移动：

```rust
let x = 5;
let y = x;
println!("{x}, {y}"); // 都可用
```

## 2.3 函数与所有权

```rust
fn take(s: String) {
    println!("{s}");
} // s drop

fn give() -> String {
    String::from("owned")
}

fn take_and_give(s: String) -> String {
    s
}
```

频繁 take/return 很吵，于是有了借用。

## 2.4 借用与引用

```rust
fn len(s: &String) -> usize {
    s.len()
}

fn append(s: &mut String) {
    s.push_str("!");
}

let mut name = String::from("Neo");
let n = len(&name);
append(&mut name);
```

规则：

- 同一时刻：要么多个 `&T`，要么一个 `&mut T`
- 引用必须始终指向有效数据

这直接对应 Web 服务里「只读共享状态」vs「可写状态要加锁/消息传递」。

## 2.5 `String` vs `&str`

```rust
fn greet(name: &str) {
    println!("hi, {name}");
}

let owned = String::from("Trinity");
greet(&owned);   // String 可强制转成 &str
greet("Morpheus");
```

经验法则：

| 场景 | 类型 |
|------|------|
| 函数只读字符串 | `&str` |
| 需要拥有并修改 | `String` |
| 长期存入结构体 | 常选 `String` |
| 静态字面量 | `&'static str` |

## 2.6 Slice

```rust
let nums = [10, 20, 30, 40];
let part = &nums[1..3]; // [20, 30]

let text = String::from("hello world");
let first = first_word(&text);

fn first_word(s: &str) -> &str {
    match s.find(' ') {
        Some(i) => &s[..i],
        None => s,
    }
}
```

## 2.7 生命周期直觉（先不写标注）

编译器要确保：返回的引用不会比输入活得更久。

```rust
fn longest<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.len() >= b.len() { a } else { b }
}
```

第 6 章再系统讲标注；现在只需知道：**借用检查器在保护你**。

## 2.8 后端中的所有权模式

```rust
// 入参借用
fn validate_email(email: &str) -> bool {
    email.contains('@')
}

// handler 结束时丢弃请求体（所有权自然释放）
struct CreateUser {
    email: String,
    name: String,
}
```

状态共享常见选择（后文展开）：

- 只读配置：`Arc<Config>`
- 可写缓存：`Arc<Mutex<T>>` / `Arc<RwLock<T>>`
- 跨任务消息：`tokio::sync::mpsc`

## 运行示例

```bash
cargo run -p ch02_ownership
```

## 本章小结

- 所有权三规则：每个值有唯一所有者；所有者离开作用域即释放；借用必须始终有效
- `String` 等管理堆内存的类型赋值/传参默认 **move**；`i32`、`bool`、`char` 等 `Copy` 类型按位复制
- 借用规则：同一时刻要么多个 `&T`，要么一个 `&mut T`
- 函数只读字符串入参用 `&str`，需要拥有/修改用 `String`，`&String` 可自动转 `&str`
- Slice（`&[T]`、`&str`）是对连续数据的借用视图，不拷贝数据
- 返回的引用不能比输入活得久——借用检查器在编译期保证这一点
- 后端状态共享模式：只读配置 `Arc<Config>`，可写缓存 `Arc<Mutex/RwLock<T>>`，跨任务消息 `mpsc`

**自测清单**

- [ ] 我能解释 move 和 borrow 的区别
- [ ] 我能说出哪些类型是 `Copy`、为什么 `String` 不是
- [ ] 我能背出借用规则，并解释它防止了什么问题
- [ ] 我能解释为什么返回局部变量的引用不能编译
- [ ] 我能在函数签名里正确选择 `&str` / `String` / `&mut String`

## 练习

1. 写 `count_words(text: &str) -> usize`。
2. 写 `normalize(s: &mut String)`：去首尾空白并转小写（可用现成方法）。
3. 解释：为什么 `fn bad() -> &str { let s = String::from("x"); &s }` 不能编译。

**动手做题**：打开 `examples/ch02_ownership/src/exercises.rs`，把 `todo!()` 换成你的实现（`last_word` 切片借用、`shout` 可变借用、`longest_in` 返回引用与生命周期）：

```bash
cd examples
cargo test -p ch02_ownership -- --ignored   # 自动判题（没做完时失败是预期的）
```

全部通过后对照参考答案 `src/solutions.rs`（默认随 `cargo test` 验证，答案保证可靠）。

---

导航：[上一章](01-language-basics.md) | [返回目录](../README.md) | [下一章](03-structs-enums.md)
