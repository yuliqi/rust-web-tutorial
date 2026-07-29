//! 对云厂商 OpenAPI 的限流（第 18 章计数限流在本场景的落地）。
//!
//! ## 为什么同步引擎必须自我克制
//!
//! 每家云的 OpenAPI 都有**调用配额**（QPS / 每分钟次数），而且配额是按**云账号**算的，
//! 不是按调用方算的。如果我们的同步引擎不加节制地猛拉某租户的 `DescribeInstances`，
//! 打爆的是**那个租户整个云账号**的 API 配额——后果是该租户自己的运维脚本、监控、
//! 控制台也一起被限流，等于我们把客户的云账号搞瘫了。所以限流不是「保护自己」，
//! 而是「做一个不给客户惹祸的好公民」。
//!
//! ## 固定窗口计数（Redis INCR + EXPIRE）
//!
//! 最简单够用的算法：给「每租户每 provider 每分钟」一个计数 key，每次调用前 INCR，
//! 超过阈值就拒绝。第一次 INCR（返回 1）时顺手给 key 设 60 秒过期，窗口自然滚动。
//! 缺点是窗口边界可能瞬时翻倍（59 秒和 61 秒各打满一次），教学量级可接受；
//! 要更平滑就换滑动窗口 / 令牌桶（第 18 章有讨论），机制换 key 设计不变。

use anyhow::{Context, Result};
use redis::aio::ConnectionManager;

/// 每租户每 provider 每分钟允许的最大同步调用次数。教学取一个好演示的小值。
/// 真实值要按云厂商公布的配额和自己的实例数倒推（留足余量给客户自己用）。
pub const MAX_CALLS_PER_MINUTE: u32 = 5;

/// 固定窗口的秒数。
const WINDOW_SECS: u64 = 60;

/// 限流计数 key：`ratelimit:{tenant}:{provider}:{分钟桶}`。
///
/// 分钟桶 = `unix_secs / 60`：同一分钟内的调用落进同一个 key、共享同一个计数器，
/// 跨到下一分钟自然换 key（旧 key 靠 TTL 过期回收）。把 key 构造抽成纯函数，
/// 既保证「计数和读取用同一个 key」，也方便离线单测。
pub fn rate_limit_key(tenant_id: i64, provider: &str, unix_secs: u64) -> String {
    let bucket = unix_secs / WINDOW_SECS;
    format!("ratelimit:{tenant_id}:{provider}:{bucket}")
}

/// 结果：允许则带上「本窗口已用到第几次」，被限则带上上限，方便日志与报告。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateDecision {
    /// 放行。`count` 是本次计入后的当前计数（1 表示本窗口第一次）。
    Allowed { count: u32 },
    /// 拒绝。`limit` 是触发拒绝的上限。
    Limited { limit: u32 },
}

impl RateDecision {
    /// 是否放行——调用方据此决定「拉还是跳过」。
    pub fn allowed(self) -> bool {
        matches!(self, RateDecision::Allowed { .. })
    }
}

/// 检查并计数一次「即将对某 provider 发起的同步调用」。
///
/// 用当前挂钟时间算窗口 key，INCR 之。若这是窗口内第一次（INCR 返回 1），顺手设 60s 过期
/// ——否则计数器永不过期，会把「历史某分钟的计数」一直带到未来。
/// 超过 [`MAX_CALLS_PER_MINUTE`] 返回 `Limited`，调用方应跳过这次拉取。
pub async fn check_rate_limit(
    cm: &mut ConnectionManager,
    tenant_id: i64,
    provider: &str,
) -> Result<RateDecision> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let key = rate_limit_key(tenant_id, provider, now);

    // INCR 对不存在的 key 视作从 0 加起，返回 1——正好用来判断「是不是本窗口第一次」。
    let count: i64 = redis::cmd("INCR")
        .arg(&key)
        .query_async(cm)
        .await
        .with_context(|| format!("限流计数失败：{key}"))?;

    if count == 1 {
        // 只在第一次设过期：避免每次调用都刷新 TTL 把窗口无限延长。
        let _: () = redis::cmd("EXPIRE")
            .arg(&key)
            .arg(WINDOW_SECS)
            .query_async(cm)
            .await
            .with_context(|| format!("设置限流窗口过期失败：{key}"))?;
    }

    let count = u32::try_from(count).unwrap_or(u32::MAX);
    if count > MAX_CALLS_PER_MINUTE {
        Ok(RateDecision::Limited {
            limit: MAX_CALLS_PER_MINUTE,
        })
    } else {
        Ok(RateDecision::Allowed { count })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_buckets_by_minute() {
        // 取一个正好落在分钟边界上的基准（能被 60 整除），便于推理桶归属。
        let base = 1_700_000_040; // 1_700_000_040 / 60 = 28333334，整除
        // 同一分钟内 -> 同 key（共享计数器）。
        let a = rate_limit_key(1, "aliyun", base);
        let b = rate_limit_key(1, "aliyun", base + 59);
        assert_eq!(a, b);
        // 跨到下一分钟 -> 不同 key。
        let c = rate_limit_key(1, "aliyun", base + 60);
        assert_ne!(a, c);
    }

    #[test]
    fn key_isolates_tenant_and_provider() {
        let t = 1_700_000_040;
        // 租户维度隔离：一个租户打满不影响另一个。
        assert_ne!(
            rate_limit_key(1, "aliyun", t),
            rate_limit_key(2, "aliyun", t)
        );
        // provider 维度隔离：阿里云打满不连累 AWS。
        assert_ne!(
            rate_limit_key(1, "aliyun", t),
            rate_limit_key(1, "aws", t)
        );
    }

    #[test]
    fn decision_allowed_helper() {
        assert!(RateDecision::Allowed { count: 1 }.allowed());
        assert!(!RateDecision::Limited { limit: 5 }.allowed());
    }
}
