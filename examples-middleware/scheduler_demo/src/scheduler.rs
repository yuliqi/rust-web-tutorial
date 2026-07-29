//! 调度核心：两种触发形态（interval / cron）+ 集群去重包装（[`guarded_tick`]）
//! + 优雅停机（[`tokio_util::sync::CancellationToken`]）。
//!
//! 触发形态怎么选：
//! - [`run_interval_job`]：`tokio::time::interval`，「每隔 N 秒/分」触发。
//!   只关心间隔、不关心落在哪个具体时刻——健康检查、缓存刷新这类。
//! - [`run_cron_job`]：`tokio-cron-scheduler`，按 cron 表达式触发。
//!   适合「每天凌晨 3 点」「每周一 9 点」这类**日历规则**。
//!
//! 无论哪种触发，集群里都要套上 [`guarded_tick`] 这层去重，否则 N 个实例会把
//! 同一个 tick 执行 N 遍。

use anyhow::Result;
use redis::aio::ConnectionManager;
use sqlx::PgPool;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio_cron_scheduler::{Job, JobScheduler};
use tokio_util::sync::CancellationToken;

use crate::jobs::record_run;
use crate::lock::{try_lock, unlock};

/// 一次 tick 在**本实例**上的结局。集群里同一个 tick 会有多个实例各自跑一遍
/// [`guarded_tick`]，正常情况下只有一个实例得到 [`TickOutcome::Executed`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickOutcome {
    /// 抢到锁、也成功抢到 DB 记录，本实例真正执行了业务。
    Executed,
    /// 没抢到锁——别的实例正在做，本实例跳过（这是**正常**的，不是错误）。
    SkippedNoLock,
    /// 抢到了锁，但 DB 里这次触发已有记录——第二道防线拦下的重复触发
    /// （通常意味着上一持有者的锁提前失效了）。本实例跳过。
    SkippedDuplicate,
}

impl TickOutcome {
    /// 本实例这次是否真正执行了业务。
    pub fn executed(self) -> bool {
        matches!(self, TickOutcome::Executed)
    }
}

/// **集群去重包装**：把一次触发安全地收敛成「整个集群只执行一遍」。
///
/// 流程与两道防线：
/// 1. 抢分布式锁 `lock:job:{job_name}:{run_key}`。抢不到 → [`TickOutcome::SkippedNoLock`]，
///    直接返回（别的实例在做，这是正常路径，不报错）。**锁是第一道防线**：
///    正常情况下同一个 tick 只有一个实例能进到下面。
/// 2. 抢到锁后 [`record_run`] 落库。若唯一约束撞车（返回 false）→
///    [`TickOutcome::SkippedDuplicate`]：说明这次触发早已被记过（多半是上一个
///    持有者锁提前过期、活却还没干完，被本实例趁虚抢到了锁）。**DB 唯一约束是
///    第二道防线**，专治「锁不可靠」的漏网之鱼（第 18 章幂等）。
/// 3. 两关都过，才在持锁期间执行业务 `business`，结束后释放锁。
///
/// 业务放在「持锁期间」执行：锁的 TTL 要略大于业务最长耗时，否则任务没干完锁就
/// 过期，防线 1 失守——此时就靠防线 2（DB 唯一约束）拦住第二个实例。两道一起上，
/// 才敢说「不会重复执行」。
///
/// 注意 `cm` 用 `&mut`（`ConnectionManager` 可 `clone`），并发跑多个实例时各自
/// 克隆一份连接句柄即可。
pub async fn guarded_tick<F, Fut>(
    cm: &mut ConnectionManager,
    pool: &PgPool,
    job_name: &str,
    run_key: &str,
    instance: &str,
    lock_ttl: Duration,
    business: F,
) -> Result<TickOutcome>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let lock_key = format!("lock:job:{job_name}:{run_key}");
    let token = crate::lock::new_token();

    // —— 第一道防线：分布式锁 ——
    if !try_lock(cm, &lock_key, &token, lock_ttl).await? {
        return Ok(TickOutcome::SkippedNoLock);
    }

    // 抢到锁了，进临界区。无论后面成败，都要把自己的锁还回去。
    // —— 第二道防线：DB 唯一约束 ——
    let inserted = record_run(pool, job_name, run_key, instance).await;
    let outcome = match inserted {
        Ok(true) => {
            // 两关都过：在持锁期间干活。
            let biz = business().await;
            match biz {
                Ok(()) => Ok(TickOutcome::Executed),
                Err(e) => Err(e),
            }
        }
        Ok(false) => Ok(TickOutcome::SkippedDuplicate),
        Err(e) => Err(e),
    };

    // 释放锁：验身通过才 DEL，不会误删别人的锁。这里忽略「锁已不是自己的」这种
    // 正常结果，只在 Redis 出错时向上传播——但不因解锁失败覆盖掉业务结果。
    let _ = unlock(cm, &lock_key, &token).await;

    outcome
}

/// 「每隔 N 秒/分」触发的最简调度循环，用 `tokio::time::interval` 实现。
///
/// 每到点就调一次 `on_tick`（通常内部就是 [`guarded_tick`]）。循环用 `select!`
/// 同时盯着 `shutdown`：一旦被取消，就**停止再触发**并退出——不打断正在执行的
/// `on_tick`（`select!` 只在 await 点抢占，当前这次 tick 会自然跑完）。这就是
/// 第 16 章优雅停机的形态：不再接新活，把手头的活干完再走。
///
/// `interval` 默认的 `MissedTickBehavior` 是 Burst（错过的 tick 会补跑）；
/// 定时任务通常不希望积压补跑，可按需改成 `Skip`/`Delay`，这里保持默认以最简。
pub async fn run_interval_job<F, Fut>(period: Duration, shutdown: CancellationToken, mut on_tick: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ()>,
{
    let mut ticker = tokio::time::interval(period);
    // 第一次 tick 会立刻就绪；跳过它，让「首个业务 tick」也等满一个周期，
    // 语义更贴近「每隔 N 秒」。
    ticker.tick().await;
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                on_tick().await;
            }
            _ = shutdown.cancelled() => {
                tracing::info!("interval 调度收到停机信号，停止触发");
                break;
            }
        }
    }
}

/// 按 cron 表达式触发，用 `tokio-cron-scheduler` 实现。
///
/// cron 表达式这里用的是 **6 段（含秒）** 格式：`秒 分 时 日 月 周`。
/// 例如 `"*/5 * * * * *"` = 每 5 秒触发一次；`"0 0 3 * * *"` = 每天凌晨 3:00:00。
/// （传统 crontab 是 5 段、不含秒，注意区别。）
///
/// `on_tick` 会被反复调用，所以约束是 `Fn`（而非只调一次的 `FnOnce`），
/// 用 `Arc` 包起来在每次触发时克隆。收到 `shutdown` 后关闭调度器并返回。
pub async fn run_cron_job<F, Fut>(
    cron_expr: &str,
    shutdown: CancellationToken,
    on_tick: F,
) -> Result<()>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let mut sched = JobScheduler::new().await?;
    let on_tick = Arc::new(on_tick);
    let job = Job::new_async(cron_expr, move |_uuid, _lock| {
        let on_tick = Arc::clone(&on_tick);
        Box::pin(async move {
            on_tick().await;
        })
    })?;
    sched.add(job).await?;
    sched.start().await?;

    // 阻塞到收到停机信号，再优雅关闭调度器（等内部任务收尾）。
    shutdown.cancelled().await;
    tracing::info!("cron 调度收到停机信号，关闭调度器");
    sched.shutdown().await?;
    Ok(())
}

/// 离线校验一个 cron 表达式是否合法——只构造、不启动调度器。
/// 给单元测试和「启动前先验证配置」用：表达式写错时尽早报错，别等到运行时。
pub fn parse_cron(cron_expr: &str) -> Result<()> {
    // Job::new 在构造时就会解析 schedule，非法表达式直接返回 Err。
    Job::new(cron_expr, |_uuid, _lock| {})?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_cron_parses() {
        // 6 段含秒：每 5 秒
        assert!(parse_cron("*/5 * * * * *").is_ok());
        // 每天凌晨 3 点
        assert!(parse_cron("0 0 3 * * *").is_ok());
    }

    #[test]
    fn invalid_cron_rejected() {
        // 段数不对 / 纯乱写都应被拒
        assert!(parse_cron("not a cron").is_err());
        assert!(parse_cron("* * *").is_err());
    }
}
