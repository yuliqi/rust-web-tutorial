//! 第 10 章：异步并发（含 async closure）
//!
//! 练习入口：`src/exercises.rs`（做题）→ `src/solutions.rs`（对照答案）
//! 只跑练习测试：`cargo test -p ch10_async -- --ignored`

mod exercises;
mod solutions;

use std::time::Instant;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio::time::{sleep, timeout, Duration};

async fn work(id: u32) -> u32 {
    sleep(Duration::from_millis(30)).await;
    id
}

async fn flaky_downstream() {
    sleep(Duration::from_millis(1500)).await;
}

#[tokio::main]
async fn main() {
    let started = Instant::now();
    let mut set = JoinSet::new();
    for i in 0..3 {
        set.spawn(work(i));
    }

    let mut sum = 0;
    while let Some(res) = set.join_next().await {
        sum += res.unwrap();
    }
    println!(
        "joinset sum={sum}, elapsed_ms={}",
        started.elapsed().as_millis()
    );

    // 1.85+ async closures
    let bump = async |x: u32| x + 1;
    println!("async closure => {}", bump(41).await);

    let (tx, mut rx) = mpsc::channel::<String>(8);
    let producer = tokio::spawn(async move {
        for i in 1..=3 {
            tx.send(format!("job-{i}")).await.unwrap();
        }
    });

    let consumer = tokio::spawn(async move {
        while let Some(job) = rx.recv().await {
            println!("received {job}");
        }
    });

    let _ = tokio::join!(producer, consumer);

    match timeout(Duration::from_millis(200), flaky_downstream()).await {
        Ok(()) => println!("downstream ok"),
        Err(_) => println!("downstream timeout (expected in demo)"),
    }
}
