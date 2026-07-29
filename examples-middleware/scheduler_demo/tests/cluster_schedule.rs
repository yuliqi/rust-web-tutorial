//! 集成测试：真连 Postgres / Redis，验证「集群里同一次触发只执行一遍」的完整闭环。
//!
//! 需要先启动服务，因此默认全部 #[ignore]：
//!   cd examples-middleware && docker compose up -d postgres redis
//!   cargo test -p scheduler_demo -- --ignored
//!
//! 每个用例都用随机 job_name（复用 new_token）：测试之间、测试与手动演示之间
//! 互不污染，也允许并行跑，不受历史残留数据影响。

use scheduler_demo::jobs::{connect, count_runs, record_run};
use scheduler_demo::lock::{DEFAULT_REDIS_URL, new_token, try_lock, unlock};
use scheduler_demo::scheduler::{TickOutcome, guarded_tick};
use std::time::Duration;

fn database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://tutorial:tutorial@localhost:5432/todos".to_string())
}

async fn redis_cm() -> redis::aio::ConnectionManager {
    let url = std::env::var("REDIS_URL").unwrap_or_else(|_| DEFAULT_REDIS_URL.to_string());
    redis::Client::open(url.as_str())
        .expect("REDIS_URL 格式不对")
        .get_connection_manager()
        .await
        .expect("连不上 Redis：需要先 docker compose up -d postgres redis")
}

/// 核心用例：并发跑两个 guarded_tick（同 job_name 同 run_key），
/// 只有一个返回「执行」，job_runs 只多一条。这就是集群去重的断言。
#[tokio::test]
#[ignore = "需要先 docker compose up -d postgres redis"]
async fn concurrent_ticks_execute_once() {
    let pool = connect(&database_url()).await.expect("连 PG 失败");
    let job_name = format!("it-job-{}", new_token());
    let run_key = "tick-0"; // 同一次触发

    // 两个实例：各自克隆一份连接句柄，模拟两台机器。
    let mut cm_a = redis_cm().await;
    let mut cm_b = redis_cm().await;
    let (pool_a, pool_b) = (pool.clone(), pool.clone());
    let (job_a, job_b) = (job_name.clone(), job_name.clone());

    let a = tokio::spawn(async move {
        guarded_tick(
            &mut cm_a,
            &pool_a,
            &job_a,
            run_key,
            "instance-A",
            Duration::from_secs(5),
            || async { Ok(()) },
        )
        .await
        .expect("guarded_tick A 出错")
    });
    let b = tokio::spawn(async move {
        guarded_tick(
            &mut cm_b,
            &pool_b,
            &job_b,
            run_key,
            "instance-B",
            Duration::from_secs(5),
            || async { Ok(()) },
        )
        .await
        .expect("guarded_tick B 出错")
    });

    let (out_a, out_b) = (a.await.unwrap(), b.await.unwrap());

    // 恰好一个 Executed（另一个 SkippedNoLock 或 SkippedDuplicate，取决于抢锁先后）。
    let executed = [out_a, out_b]
        .iter()
        .filter(|o| o.executed())
        .count();
    assert_eq!(executed, 1, "同一次触发应恰好由一个实例执行，实得 {executed}");

    // 落库也只多一条。
    let n = count_runs(&pool, &job_name).await.expect("count 失败");
    assert_eq!(n, 1, "job_runs 应只有 1 条，实得 {n}");
}

/// record_run 的幂等性：同 (job_name, run_key) 插两次，第二次因 ON CONFLICT 返回 false。
#[tokio::test]
#[ignore = "需要先 docker compose up -d postgres redis"]
async fn record_run_is_idempotent() {
    let pool = connect(&database_url()).await.expect("连 PG 失败");
    let job_name = format!("it-job-{}", new_token());
    let run_key = "tick-42";

    let first = record_run(&pool, &job_name, run_key, "inst-1")
        .await
        .expect("第一次 record_run 出错");
    assert!(first, "首次插入应返回 true");

    let second = record_run(&pool, &job_name, run_key, "inst-2")
        .await
        .expect("第二次 record_run 出错");
    assert!(!second, "同一 (job_name, run_key) 再插应返回 false（唯一约束兜底）");

    assert_eq!(count_runs(&pool, &job_name).await.unwrap(), 1);
}

/// 锁的抢占/释放基本流程：A 抢到、B 抢不到；A 释放后 B 才抢得到；
/// B 用错误 token 删不掉 A 的锁。
#[tokio::test]
#[ignore = "需要先 docker compose up -d postgres redis"]
async fn lock_acquire_and_release() {
    let mut cm = redis_cm().await;
    let key = format!("it-lock-{}", new_token());
    let token_a = new_token();
    let token_b = new_token();
    let ttl = Duration::from_secs(10);

    let a_got = try_lock(&mut cm, &key, &token_a, ttl).await.unwrap();
    let b_got = try_lock(&mut cm, &key, &token_b, ttl).await.unwrap();
    assert!(a_got && !b_got, "同一把锁只能一个持有者");

    // B 拿自己的 token 删不掉 A 的锁（验身失败）。
    let stolen = unlock(&mut cm, &key, &token_b).await.unwrap();
    assert!(!stolen, "错误 token 不应能删掉别人的锁");

    // A 释放后 B 才抢得到。
    let released = unlock(&mut cm, &key, &token_a).await.unwrap();
    assert!(released, "持有者应能释放自己的锁");
    let b_retry = try_lock(&mut cm, &key, &token_b, ttl).await.unwrap();
    assert!(b_retry, "锁释放后应能被再次抢到");

    unlock(&mut cm, &key, &token_b).await.unwrap(); // 收尾
}

/// 顺带验证 TickOutcome 的语义在集成环境下自洽。
#[tokio::test]
#[ignore = "需要先 docker compose up -d postgres redis"]
async fn second_instance_skips() {
    let pool = connect(&database_url()).await.expect("连 PG 失败");
    let mut cm = redis_cm().await;
    let job_name = format!("it-job-{}", new_token());
    let run_key = "tick-7";

    let first = guarded_tick(
        &mut cm,
        &pool,
        &job_name,
        run_key,
        "inst-1",
        Duration::from_secs(5),
        || async { Ok(()) },
    )
    .await
    .unwrap();
    assert_eq!(first, TickOutcome::Executed);

    // 同一次触发再来一遍：锁已释放，但 DB 唯一约束会拦下 -> SkippedDuplicate。
    let second = guarded_tick(
        &mut cm,
        &pool,
        &job_name,
        run_key,
        "inst-2",
        Duration::from_secs(5),
        || async { Ok(()) },
    )
    .await
    .unwrap();
    assert_eq!(second, TickOutcome::SkippedDuplicate);

    assert_eq!(count_runs(&pool, &job_name).await.unwrap(), 1);
}
