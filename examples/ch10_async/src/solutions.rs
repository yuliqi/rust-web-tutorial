//! 第 10 章练习参考答案。先自己完成 `exercises.rs`，再来对照。
//!
//! 本文件的测试与练习测试断言完全相同，且默认随 `cargo test` 运行，
//! 保证参考答案本身始终是对的。
#![allow(dead_code)]

use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio::time::{Duration, sleep, timeout};

async fn fake_request(id: u32) -> u32 {
    sleep(Duration::from_millis(40)).await;
    id
}

/// 练习 1 参考答案：用 `JoinSet` 先把所有请求 spawn 出去再收结果。
/// 关键在「先全部 spawn、后统一 await」——所有任务被同时交给运行时，
/// 等待期彼此重叠（单线程运行时下也会在 await 让出时交错推进）；
/// 若写成 `for id { fake_request(id).await }` 则是串行 200ms。
pub async fn concurrent_sum(ids: &[u32]) -> u32 {
    let mut set = JoinSet::new();
    for &id in ids {
        set.spawn(fake_request(id));
    }
    let mut sum = 0;
    while let Some(res) = set.join_next().await {
        sum += res.unwrap();
    }
    sum
}

/// 练习 2 参考答案：单生产者 + 当前任务消费。
/// `tx` 被 move 进生产者任务，任务结束即 drop，通道随之关闭，
/// `rx.recv()` 返回 `None` 让循环自然退出——不需要额外的「结束标记」。
/// 单生产者的 mpsc 保证接收顺序与发送顺序一致，测试才能断言顺序。
pub async fn run_job_queue(jobs: Vec<String>) -> Vec<String> {
    let (tx, mut rx) = mpsc::channel::<String>(8);
    let producer = tokio::spawn(async move {
        for job in jobs {
            tx.send(job).await.unwrap();
        }
    });

    let mut done = Vec::new();
    while let Some(job) = rx.recv().await {
        done.push(format!("done: {job}"));
    }
    producer.await.unwrap();
    done
}

/// 练习 3 参考答案：`timeout` 把「超时」变成 `Err(Elapsed)` 这个普通值，
/// 我们再用 match 把它翻译成业务错误字符串。内部 future 到点会被直接
/// drop（取消），不会继续占着运行时。
pub async fn call_with_timeout(delay_ms: u64, limit_ms: u64) -> Result<u32, &'static str> {
    let downstream = async {
        sleep(Duration::from_millis(delay_ms)).await;
        42
    };
    match timeout(Duration::from_millis(limit_ms), downstream).await {
        Ok(v) => Ok(v),
        Err(_) => Err("timeout"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[tokio::test]
    async fn ex1_concurrent_sum() {
        let started = Instant::now();
        assert_eq!(concurrent_sum(&[1, 2, 3, 4, 5]).await, 15);
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "5 个 40ms 的请求应并发完成（串行要 200ms）"
        );
    }

    #[tokio::test]
    async fn ex2_run_job_queue() {
        let jobs = vec!["build".to_string(), "test".to_string(), "deploy".to_string()];
        assert_eq!(
            run_job_queue(jobs).await,
            vec!["done: build", "done: test", "done: deploy"]
        );
        assert_eq!(run_job_queue(Vec::new()).await, Vec::<String>::new());
    }

    #[tokio::test]
    async fn ex3_call_with_timeout() {
        assert_eq!(call_with_timeout(10, 80).await, Ok(42));
        assert_eq!(call_with_timeout(80, 10).await, Err("timeout"));
    }
}
