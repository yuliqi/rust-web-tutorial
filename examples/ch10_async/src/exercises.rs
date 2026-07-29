//! 第 10 章练习：把每个 `todo!()` 换成你的实现。
//!
//! 做题流程：
//! 1. 只跑练习测试（未完成时会失败，这是预期的）：
//!    `cargo test -p ch10_async -- --ignored`
//! 2. 全部通过后，对照参考答案 `src/solutions.rs`（先自己做，别偷看）。
#![allow(dead_code)]
#![allow(unused_variables)]

use tokio::time::{Duration, sleep};

/// 模拟一次耗时 40ms 的「网络请求」，完成后原样返回 id。
/// 练习 1 请直接复用它，不要改动。
async fn fake_request(id: u32) -> u32 {
    sleep(Duration::from_millis(40)).await;
    id
}

/// 练习 1：并发发起多个 `fake_request` 并汇总结果之和。
///
/// 要求：所有请求必须「同时」进行——5 个 40ms 的请求总耗时应远小于
/// 串行的 200ms（测试会检查耗时 < 100ms）。
///
/// 提示：`async fn` 返回的 Future 是惰性的，逐个 `.await` 就变成串行了；
/// 想真正并发要么 `tokio::spawn` 把每个 Future 交给运行时（再逐个等
/// `JoinHandle`），要么用 `main.rs` 里演示过的 `JoinSet`。
pub async fn concurrent_sum(ids: &[u32]) -> u32 {
    todo!("先全部 spawn 出去，再逐个收结果")
}

/// 练习 2：用 `mpsc` 通道实现一个最小任务队列。
///
/// 要求：spawn 一个生产者任务把 `jobs` 里的每个任务名发进通道；
/// 当前任务作为消费者逐个接收，把每项变成 `"done: {job}"` 后按
/// 接收顺序收集进 Vec 返回。
///
/// 提示：`tokio::sync::mpsc::channel(8)` 创建有界通道；把 `tx` `move`
/// 进生产者任务，任务结束时 `tx` 被 drop，通道关闭，消费者侧的
/// `rx.recv().await` 才会返回 `None` 结束循环——这就是「关闭通道 =
/// 广播结束信号」的惯用法。
pub async fn run_job_queue(jobs: Vec<String>) -> Vec<String> {
    todo!("channel -> spawn 生产者 -> while let Some(job) = rx.recv().await")
}

/// 练习 3：给一次模拟调用加超时保护。
///
/// 要求：内部模拟一个「先 sleep `delay_ms` 毫秒、然后返回 42」的
/// 下游调用；用 `limit_ms` 毫秒作为超时上限包住它：
/// - 按时完成 → `Ok(42)`
/// - 超时 → `Err("timeout")`
///
/// 提示：`tokio::time::timeout(Duration, future)` 返回
/// `Result<T, Elapsed>`——超时是「值」不是 panic，用 `match` 把它翻译
/// 成业务层的 `Result` 即可（对照 `main.rs` 里的 `flaky_downstream`）。
pub async fn call_with_timeout(delay_ms: u64, limit_ms: u64) -> Result<u32, &'static str> {
    todo!("timeout(Duration::from_millis(limit_ms), 内部 future).await")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[tokio::test]
    #[ignore = "练习：完成后运行 cargo test -p ch10_async -- --ignored"]
    async fn ex1_concurrent_sum() {
        let started = Instant::now();
        assert_eq!(concurrent_sum(&[1, 2, 3, 4, 5]).await, 15);
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "5 个 40ms 的请求应并发完成（串行要 200ms）"
        );
    }

    #[tokio::test]
    #[ignore = "练习：完成后运行 cargo test -p ch10_async -- --ignored"]
    async fn ex2_run_job_queue() {
        let jobs = vec!["build".to_string(), "test".to_string(), "deploy".to_string()];
        assert_eq!(
            run_job_queue(jobs).await,
            vec!["done: build", "done: test", "done: deploy"]
        );
        assert_eq!(run_job_queue(Vec::new()).await, Vec::<String>::new());
    }

    #[tokio::test]
    #[ignore = "练习：完成后运行 cargo test -p ch10_async -- --ignored"]
    async fn ex3_call_with_timeout() {
        assert_eq!(call_with_timeout(10, 80).await, Ok(42));
        assert_eq!(call_with_timeout(80, 10).await, Err("timeout"));
    }
}
