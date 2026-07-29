//! 可运行演示：在**同一个进程里模拟两个逻辑实例**（instance-A / instance-B），
//! 它们共享同一个 Redis 和 Postgres，各自跑一份一模一样的调度循环——正是集群里
//! 「每个副本都揣着一个调度器」的缩影。
//!
//! 每 2 秒触发一次、跑约 6 秒（3 次触发）。你会看到每次 tick 两个实例抢同一把锁，
//! 只有一个打印「抢到锁，执行」，另一个「未抢到，跳过」。最后打印 job_runs 表的行数：
//! **3 次触发只落 3 条记录，而不是 6 条**——这就是集群去重的效果。
//!
//! 运行前先启动依赖：
//!   cd examples-middleware && docker compose up -d postgres redis
//! 然后 `cargo run -p scheduler_demo`。演示带整体超时，会自动退出、不常驻。

use anyhow::{Context, Result};
use scheduler_demo::jobs::{
    DEFAULT_DATABASE_URL, bucket_run_key, connect, count_runs, instance_id,
};
use scheduler_demo::lock::DEFAULT_REDIS_URL;
use scheduler_demo::scheduler::{TickOutcome, guarded_tick, run_interval_job};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_util::sync::CancellationToken;

/// 触发周期：每 2 秒一次。run_key 的取整周期必须与它对齐（见 bucket_run_key 注释）。
const PERIOD: Duration = Duration::from_secs(2);
/// 锁 TTL 要略大于「一次任务耗时」，这里业务几乎瞬时，给 1 秒足够。
const LOCK_TTL: Duration = Duration::from_secs(1);

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string());
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| DEFAULT_REDIS_URL.to_string());

    let pool = connect(&database_url).await?;
    let redis = redis::Client::open(redis_url.as_str())
        .context("REDIS_URL 格式不对")?
        .get_connection_manager()
        .await
        .with_context(|| {
            format!(
                "连不上 Redis（{redis_url}）？请先在 examples-middleware 目录执行 \
                 `docker compose up -d postgres redis`"
            )
        })?;

    // 每次运行用带时间戳的 job_name，避免历史数据干扰观察。
    let job_name = format!(
        "demo-billing-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    );
    tracing::info!(%job_name, "开始演示：两个逻辑实例共抢一把锁，每 2 秒触发一次");

    // 一个取消令牌，约 6.5 秒后触发，让两个调度循环一起优雅停机。
    let shutdown = CancellationToken::new();

    // —— 启动两个逻辑实例：各自的 instance_id、各自的调度循环，共享 Redis/PG ——
    let inst_a = format!("{}#A", instance_id());
    let inst_b = format!("{}#B", instance_id());

    let task_a = spawn_instance("instance-A", inst_a, pool.clone(), redis.clone(), job_name.clone(), shutdown.clone());
    let task_b = spawn_instance("instance-B", inst_b, pool.clone(), redis.clone(), job_name.clone(), shutdown.clone());

    // 跑约 6.5 秒（够 3 次触发），然后发停机信号。
    tokio::time::sleep(Duration::from_millis(6_500)).await;
    tracing::info!("演示时间到，发送停机信号");
    shutdown.cancel();

    // 等两个实例的调度循环收尾。
    let _ = tokio::join!(task_a, task_b);

    // —— 收官：看 job_runs 表 ——
    let n = count_runs(&pool, &job_name).await?;
    tracing::info!(
        "job_runs 中 {job_name} 的记录数 = {n} 条（3 次触发去重后应为 3，而非两个实例各记一遍的 6）"
    );

    Ok(())
}

/// 启一个「逻辑实例」：一个独立的 interval 调度循环。每个 tick 里，本实例算出
/// 这次触发的 run_key，套上 [`guarded_tick`] 去和另一个实例抢锁。
fn spawn_instance(
    label: &'static str,
    instance: String,
    pool: sqlx::PgPool,
    redis: redis::aio::ConnectionManager,
    job_name: String,
    shutdown: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let cm = redis;
        run_interval_job(PERIOD, shutdown, move || {
            // 闭包被 FnMut 反复调用，返回的 future 又会跨 await 存活，不能让它借用闭包
            // 捕获的变量（借用会随每次调用结束而失效）。这里每个 tick 各克隆一份：
            // ConnectionManager / PgPool 都是「可克隆的句柄」，克隆的是引用计数、
            // 底层连接池共享，代价极小——这是 async 闭包里最顺手的写法。
            let pool = pool.clone();
            let mut cm = cm.clone();
            let instance = instance.clone();
            let job_name = job_name.clone();
            async move {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                let run_key = bucket_run_key(now, PERIOD.as_secs() as i64);

                let outcome = guarded_tick(
                    &mut cm,
                    &pool,
                    &job_name,
                    &run_key,
                    &instance,
                    LOCK_TTL,
                    || async {
                        // 这里就是「业务」：真实项目里是发一封催费邮件、生成一张账单……
                        // 记住重活要外置到消息队列（第 14 章），tick 只做入队这种轻动作。
                        Ok(())
                    },
                )
                .await;

                match outcome {
                    Ok(TickOutcome::Executed) => {
                        tracing::info!("[{label}] run_key={run_key} 抢到锁，执行 ✅");
                    }
                    Ok(TickOutcome::SkippedNoLock) => {
                        tracing::info!("[{label}] run_key={run_key} 未抢到锁，跳过（别的实例在做）");
                    }
                    Ok(TickOutcome::SkippedDuplicate) => {
                        tracing::info!("[{label}] run_key={run_key} 锁抢到了但已有记录，跳过（第二道防线拦下）");
                    }
                    Err(e) => {
                        tracing::error!("[{label}] run_key={run_key} 出错：{e:#}");
                    }
                }
            }
        })
        .await;
    })
}
