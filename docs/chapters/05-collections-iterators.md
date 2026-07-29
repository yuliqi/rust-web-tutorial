# 第 5 章：集合与迭代器

> 示例：`examples/ch05_collections`

## 学习目标

- 熟练 `Vec` / `HashMap` 等常用集合
- 用迭代器做转换与聚合
- 理解闭包与所有权交互

## 5.1 `Vec<T>`

```rust
let mut ids = vec![1, 2, 3];
ids.push(4);
let first = ids[0];
let maybe = ids.get(10); // Option<&T>，更安全
```

常见 API：`push/pop/extend/retain/sort/dedup`。

## 5.2 `HashMap<K, V>`

```rust
use std::collections::HashMap;

let mut scores = HashMap::new();
scores.insert("neo", 10);
*scores.entry("trinity").or_insert(0) += 1;
```

后端常见用途：

- 请求头索引
- 内存仓库
- 聚合统计

## 5.3 迭代器思维

```rust
let titles = vec!["a", "bb", "ccc"];
let long: Vec<_> = titles
    .iter()
    .filter(|t| t.len() > 1)
    .map(|t| t.to_uppercase())
    .collect();
```

惰性：适配器不立刻执行，遇到 `collect`/`sum`/`for` 等消费者才算。

### 消费方式与所有权

```rust
for x in &titles { /* &str */ }
for x in titles.iter() { /* &&str or &String */ }
for x in titles.into_iter() { /* 拿走元素所有权 */ }
```

## 5.4 闭包

```rust
let prefix = String::from("todo:");
let decorate = |title: &str| format!("{prefix}{title}");
// prefix 被不可变借用进闭包
```

Trait 直觉：

- `Fn`：可重复不可变调用
- `FnMut`：可修改捕获
- `FnOnce`：可能消耗捕获，最多调用一次

## 5.5 实用聚合

```rust
let nums = [1, 2, 3, 4, 5];
let sum: i32 = nums.iter().sum();
let max = nums.iter().copied().max();
let all_positive = nums.iter().all(|n| *n > 0);
```

## 5.6 Web 场景例子：查询过滤

```rust
#[derive(Clone)]
struct Todo {
    id: u64,
    title: String,
    done: bool,
}

fn filter_todos(items: &[Todo], q: Option<&str>, done: Option<bool>) -> Vec<Todo> {
    items
        .iter()
        .filter(|t| done.is_none_or(|d| t.done == d))
        .filter(|t| q.is_none_or(|query| t.title.contains(query)))
        .cloned()
        .collect()
}
```

## 5.7 现代 Option API（请优先使用）

```rust
// 比 map(|x| pred(x)).unwrap_or(true) 更直读
done.is_none_or(|d| t.done == d)
q.is_some_and(|s| !s.is_empty())
```

Web 过滤/可选查询参数场景几乎天天用到。

## 运行示例

```bash
cargo run -p ch05_collections
```

## 本章小结

- `Vec` 是动态数组；`get` 返回 `Option<&T>`，比索引越界 panic 更安全
- `HashMap` 的 entry API（`entry(..).or_insert(..)`）是计数与聚合的利器
- 迭代器是惰性的：`filter`/`map` 等适配器不执行，遇到 `collect`/`sum`/`for` 等消费者才算
- `iter()` 借用元素，`into_iter()` 拿走所有权，选错会引发所有权报错
- 闭包捕获环境变量；`Fn`/`FnMut`/`FnOnce` 对应不可变、可变、消耗三种捕获方式
- `is_none_or`/`is_some_and` 处理可选查询参数比 `map + unwrap_or` 更直读

**自测清单**

- [ ] 我能用 entry API 写出词频/计数统计
- [ ] 我能解释迭代器的惰性求值，说出哪些是适配器、哪些是消费者
- [ ] 我能区分 `iter()` 与 `into_iter()` 对所有权的影响
- [ ] 我能说出 `Fn`/`FnMut`/`FnOnce` 的区别
- [ ] 我能用 `filter`/`map`/`collect` 链完成转换与聚合

## 练习

1. 统计一段日志里每个状态码出现次数（`HashMap<u16, usize>`）。
2. 用迭代器实现 `top_k` 词频。
3. 对 `Vec<Todo>` 做分页：`page/page_size`。

**动手做题**：打开 `examples/ch05_collections/src/exercises.rs`，把 `todo!()` 换成你的实现（`word_freq` entry API、`top_k` 排序取前 k、`sum_even_squares` 迭代器链）：

```bash
cd examples
cargo test -p ch05_collections -- --ignored   # 自动判题（没做完时失败是预期的）
```

全部通过后对照参考答案 `src/solutions.rs`（默认随 `cargo test` 验证，答案保证可靠）。

---

导航：[上一章](04-error-handling.md) | [返回目录](../README.md) | [下一章](06-generics-traits.md)
