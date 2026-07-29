# 第 9 章：智能指针与内存模型

> 示例：`examples/ch09_smart_pointers`

## 学习目标

- 理解 `Box` / `Rc` / `Arc` / `RefCell` / `Mutex`
- 会在 Web 状态共享中选择正确容器
- 知道 `unsafe` 的边界心态

## 9.1 `Box<T>`：堆分配

```rust
let b = Box::new(5);
```

用途：

- 递归类型（如链表/树）
- 大对象避免栈上拷贝
- trait 对象：`Box<dyn Error>`

## 9.2 `Rc` / `Arc`：共享所有权

- `Rc`：单线程引用计数
- `Arc`：原子引用计数，可跨线程

```rust
use std::sync::Arc;

let config = Arc::new(String::from("prod"));
let c1 = Arc::clone(&config);
let c2 = Arc::clone(&config);
```

Axum 的 `State` 几乎总是 `Arc<AppState>`。

## 9.3 内部可变性：`RefCell` / `Mutex`

共享但还想改：

| 场景 | 工具 |
|------|------|
| 单线程 | `Rc<RefCell<T>>` |
| 多线程/异步 | `Arc<Mutex<T>>` / `Arc<RwLock<T>>` |
| 异步锁优先 | `tokio::sync::Mutex`（跨 `.await` 时） |

```rust
use std::sync::{Arc, Mutex};

let counter = Arc::new(Mutex::new(0));
{
    let mut guard = counter.lock().unwrap();
    *guard += 1;
}
```

注意：锁粒度要小，避免持锁做 IO。

## 9.4 `Weak` 打破循环

`Rc` 循环引用会导致内存泄露；父子图结构常用 `Weak`。

## 9.5 Web 状态模板

```rust
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;

#[derive(Clone)]
struct AppState {
    todos: Arc<RwLock<HashMap<u64, String>>>,
}
```

只读多、写少：`RwLock`；写冲突高：考虑 DB/消息队列，不只靠内存锁。

## 9.6 `unsafe` 心态

你可以写完整后端而几乎不碰 `unsafe`。

只有在以下情况才认真考虑：

- FFI
- 极致性能数据结构
- 编译器无法证明但你能证明的不变式

规则：把 `unsafe` 关进最小模块，用安全 API 包起来，并写清楚安全不变量。

## 9.7 全局惰性初始化：优先 `LazyLock`

老代码常见 `lazy_static!` / `once_cell`。1.97 时代后端配置/客户端单例优先标准库：

```rust
use std::sync::LazyLock;

static APP_ENV: LazyLock<String> = LazyLock::new(|| {
    std::env::var("APP_ENV").unwrap_or_else(|_| "dev".into())
});
```

适合：进程级只读配置、正则、HTTP client 模板。  
不适合：请求级状态（仍放 `AppState` / 连接池）。

## 运行示例

```bash
cargo run -p ch09_smart_pointers
```

## 本章小结

- `Box<T>` 做堆分配：递归类型、大对象、trait 对象
- `Rc` 是单线程引用计数，`Arc` 是原子引用计数可跨线程；Axum `State` 几乎总是 `Arc<AppState>`
- 内部可变性按场景选：单线程 `Rc<RefCell<T>>`，多线程 `Arc<Mutex<T>>` / `Arc<RwLock<T>>`，跨 `.await` 用 `tokio::sync::Mutex`
- 锁粒度要小，避免持锁做 IO
- `Rc` 循环引用会泄露内存，父子结构用 `Weak` 打破
- 后端几乎不需要 `unsafe`；真要用就关进最小模块并写清安全不变量
- 进程级只读单例优先标准库 `LazyLock`；`lazy_static!`/`once_cell` 是老代码的常见写法

**自测清单**

- [ ] 我能说出 `Rc` 和 `Arc` 的适用场景差异
- [ ] 我能解释为什么共享可变状态需要 `RefCell` 或 `Mutex` 配合
- [ ] 我能说出跨 `.await` 持锁时该选哪种 Mutex
- [ ] 我能解释 `Weak` 解决了什么问题
- [ ] 我能判断一个全局配置该用 `LazyLock` 还是放进 `AppState`

## 练习

1. 用 `Arc<Mutex<u64>>` 实现多线程计数。
2. 比较 `Mutex` 与 `RwLock` 在读多写下的语义差异（文字即可）。
3. 设计 `AppState`，包含配置（只读）与缓存（可写）。

**动手做题**：打开 `examples/ch09_smart_pointers/src/exercises.rs`，把 `todo!()` 换成你的实现。三道题分别对应：`parallel_count`（Arc+Mutex 并发计数）、`list_sum`（Box 递归链表求和）、`AppState`（Rc+RefCell 内部可变性缓存）。

```bash
cd examples
cargo test -p ch09_smart_pointers -- --ignored   # 自动判题（没做完时失败是预期的）
```

全部通过后对照参考答案 `src/solutions.rs`（默认随 `cargo test` 验证，答案保证可靠）。

---

导航：[上一章](08-testing-quality.md) | [返回目录](../README.md) | [下一章](10-concurrency-async.md)
