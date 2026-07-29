//! 限流与幂等键（第 18 章）：两个能力都构建在 Redis 的原子命令之上。
//!
//! 为什么用 Redis 而不是进程内存：SaaS 服务水平扩展后有 N 个实例，进程内的
//! 计数器各数各的，30 次/分钟会变成 30×N；Redis 是所有实例共享的单一计数点。
//! 键的构造独立成纯函数，方便离线单测（见文件底部）。

use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{AppError, AppResult};

/// 每租户每分钟的请求上限。SaaS 语境下限流按**租户**而不是按 IP：
/// 一个租户的脚本失控不该拖垮别的租户——限流是隔离故障域的手段，
/// 也是套餐差异化的落点（pro 给更高配额，此处教学从简统一 30）。
pub const RATE_LIMIT_PER_MINUTE: i64 = 30;

/// 幂等键的保存时长：24 小时。Stripe 的幂等键也是 24h 量级——
/// 要覆盖「客户端隔了很久才重试」的场景，又不能让键永久堆积。
pub const IDEMPOTENCY_TTL_SECS: i64 = 86_400;

/// 当前的 unix 分钟数（unix 秒 / 60）：固定窗口限流的窗口编号。
pub fn current_unix_minute() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs()
        / 60
}

/// 限流计数器的 Redis 键：`rl:{tenant_id}:{unix_minute}`。
/// 分钟数进了键名，跨窗口自动「换一个键从零数起」，旧键靠 EXPIRE 自然消亡。
pub fn rate_limit_key(tenant_id: i64, unix_minute: u64) -> String {
    format!("rl:{tenant_id}:{unix_minute}")
}

/// 幂等键的 Redis 键：`idem:{tenant_id}:{client_key}`。
/// tenant_id 必须进键名——否则 A 租户可以用和 B 租户相同的 Idempotency-Key
/// 读到 B 缓存的响应，幂等机制反而变成跨租户信息泄露的洞。
pub fn idempotency_redis_key(tenant_id: i64, client_key: &str) -> String {
    format!("idem:{tenant_id}:{client_key}")
}

/// 固定窗口限流：INCR 当前分钟的计数器，首次时挂 60 秒 TTL，超过上限返回 429。
///
/// INCR 是原子的，N 个实例并发调用也不会数错——这就是「共享计数点」的意义。
/// 已知短板一：INCR 与 EXPIRE 是两条命令，若进程恰好在两者之间崩溃，键会
/// 无 TTL 常驻（生产可用 Lua 脚本或 `SET ... EX NX` + INCR 组合封成原子）。
/// 已知短板二：固定窗口的**边界毛刺**——攻击者在 0:59 打 30 发、1:01 再打
/// 30 发，2 秒内实际通过 60 发，瞬时是名义限速的两倍。滑动窗口（把计数拆到
/// 更细的子窗口加权求和）能抹平毛刺，留作练习。
pub async fn enforce_rate_limit(redis: &ConnectionManager, tenant_id: i64) -> AppResult<()> {
    // ConnectionManager 内部是多路复用的共享连接，clone 是廉价的句柄复制。
    let mut conn = redis.clone();
    let key = rate_limit_key(tenant_id, current_unix_minute());
    let count: i64 = conn.incr(&key, 1).await?;
    if count == 1 {
        // 窗口首个请求负责挂 TTL；60 秒后整个键消失，无需清理任务。
        let _: bool = conn.expire(&key, 60).await?;
    }
    if count > RATE_LIMIT_PER_MINUTE {
        return Err(AppError::too_many_requests(
            "rate limit exceeded (30 req/min per tenant)",
        ));
    }
    Ok(())
}

// 离线单测：键构造是纯函数，格式一旦变更（比如少了 tenant_id）测试立刻报警。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_key_format() {
        assert_eq!(rate_limit_key(42, 29_000_000), "rl:42:29000000");
        // 不同租户/不同分钟必须落到不同的键——各租户各窗口独立计数的根据。
        assert_ne!(rate_limit_key(1, 100), rate_limit_key(2, 100));
        assert_ne!(rate_limit_key(1, 100), rate_limit_key(1, 101));
    }

    #[test]
    fn idempotency_key_format() {
        assert_eq!(idempotency_redis_key(7, "order-abc"), "idem:7:order-abc");
        // 同一个客户端键在不同租户下必须是不同的 Redis 键（防跨租户读缓存）。
        assert_ne!(
            idempotency_redis_key(1, "same-key"),
            idempotency_redis_key(2, "same-key")
        );
    }

    #[test]
    fn unix_minute_is_seconds_div_60() {
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let minute = current_unix_minute();
        // 两次取时间之间最多跨过一个分钟边界。
        assert!(minute == now_secs / 60 || minute == now_secs / 60 + 1);
    }
}
