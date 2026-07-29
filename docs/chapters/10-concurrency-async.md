# 第 10 章：并发与异步

> 示例：`examples/ch10_async`

## 学习目标

- 区分线程并发与异步并发
- 掌握 `async/await` + `tokio` 基础
- 避开阻塞、取消、锁跨越 await 的坑

## 10.1 为什么 Web 后端偏爱异步

一个 API 服务大量时间在等：

- 数据库
- 下游 HTTP
- 磁盘/网络

异步用更少线程承载更高并发连接。

## 10.2 线程模型（简）

```rust
use std::thread;

let h = thread::spawn(|| 1 + 2);
let v = h.join().unwrap();
```

适合：CPU 密集、隔离阻塞任务。

## 10.3 `async/await`

```rust
async fn fetch_hello() -> String {
    // 这里通常 .await 某个 IO
    "hello".to_string()
}

#[tokio::main]
async fn main() {
    let s = fetch_hello().await;
    println!("{s}");
}
```

`async fn` 返回 `Future`；只有被 executor poll 才会推进。

## 10.4 Tokio 常用能力

```rust
use tokio::time::{sleep, Duration};
use tokio::task::JoinSet;

async fn work(id: u32) -> u32 {
    sleep(Duration::from_millis(10)).await;
    id
}

#[tokio::main]
async fn main() {
    let mut set = JoinSet::new();
    for i in 0..5 {
        set.spawn(work(i));
    }
    while let Some(res) = set.join_next().await {
        println!("{:?}", res);
    }
}
```

通道：

```rust
use tokio::sync::mpsc;

let (tx, mut rx) = mpsc::channel(32);
tx.send("event").await.unwrap();
let got = rx.recv().await;
```

## 10.5 `Send` / `Sync`

- `Send`：值可以转移到另一线程
- `Sync`：引用可以在线程间共享（`&T` 是 `Send`）

异步任务跨线程调度时，常要求 `Future + Send`。  
`Rc` / 普通 `RefCell` 不适合；用 `Arc` / `Mutex`。

## 10.6 常见陷阱

1. **在异步里做重 CPU/阻塞 IO**  
   用 `spawn_blocking` 或独立线程池。

2. **持有 `std::sync::Mutex` 跨越 `.await`**  
   可能卡死调度；跨 await 用 `tokio::sync::Mutex`，或缩小锁范围。

3. **忘记超时**  
   下游调用务必 `timeout`。

4. **任务泄漏**  
   明确生命周期，避免 `spawn` 后无人管理。

```rust
use tokio::time::{timeout, Duration};

let result = timeout(Duration::from_secs(2), downstream()).await;
```

## 10.7 async closures（1.85+，1.97 常用）

异步闭包可直接 `.await`，写小任务工厂更干净：

```rust
let add = async |x: i32| x + 1;
let y = add(41).await;
```

配合 `JoinSet`/短生命周期任务时很方便；复杂逻辑仍建议独立 `async fn`（更好测、更好读）。

## 10.8 与 axum 的关系

axum handler 本身就是异步函数：

```rust
async fn health() -> &'static str {
    "ok"
}
```

状态、DB pool、HTTP client 都放进 `Arc<AppState>`，在 handler 里 `.await` 仓库方法。

## 运行示例

```bash
cargo run -p ch10_async
```

## 本章小结

- Web 后端大量时间在等 IO，异步用更少线程承载更高并发；CPU 密集才用线程
- `async fn` 返回 `Future`，只有被 executor poll 才推进
- Tokio 常用能力：`JoinSet` 并发收集、`mpsc` 通道、`sleep`/`timeout`
- `Send` 是值可转移到别的线程，`Sync` 是引用可跨线程共享；跨线程调度的 Future 常要求 `Send`，所以用 `Arc`/`Mutex` 而非 `Rc`/`RefCell`
- 四大陷阱：异步里做重 CPU/阻塞 IO、持 `std::sync::Mutex` 跨 `.await`、忘记超时、任务泄漏
- async 闭包（1.85+）适合小任务工厂，复杂逻辑仍用独立 `async fn`
- axum handler 就是异步函数，共享状态放 `Arc<AppState>`

**自测清单**

- [ ] 我能说出什么任务适合线程、什么任务适合异步
- [ ] 我能解释 `Send` 和 `Sync` 的区别
- [ ] 我能说出跨 `.await` 持锁的风险和两种解法
- [ ] 我能用 `JoinSet` 或 `mpsc` 描述一个并发方案
- [ ] 我能解释为什么下游调用必须加 `timeout`

## 练习

1. 并发 sleep 3 个任务并汇总耗时。
2. 用 `mpsc` 做简单任务队列。
3. 给假网络调用加 1 秒超时。

**动手做题**：打开 `examples/ch10_async/src/exercises.rs`，把 `todo!()` 换成你的实现。三道题分别对应：`concurrent_sum`（并发 join 汇总）、`run_job_queue`（mpsc 任务队列）、`call_with_timeout`（超时控制）。

```bash
cd examples
cargo test -p ch10_async -- --ignored   # 自动判题（没做完时失败是预期的）
```

全部通过后对照参考答案 `src/solutions.rs`（默认随 `cargo test` 验证，答案保证可靠）。

---

导航：[上一章](09-smart-pointers.md) | [返回目录](../README.md) | [下一章](11-web-ecosystem.md)
