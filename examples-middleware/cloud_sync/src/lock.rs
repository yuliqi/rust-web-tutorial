//! 分布式锁（Redis 版）——同步任务集群去重的第一道防线。
//!
//! **这与第 21 章 `scheduler_demo/src/lock.rs`、第 15 章 `cluster_demo/src/dlock.rs`
//! 是同一套机制**（SET NX 抢锁、Lua 脚本验身解锁），此处刻意重写一份是为了让本示例
//! 自包含、能单独 `cargo run`，而不是跨 crate 去 `use`。生产项目里这类基础能力应当
//! 抽成一个公共 crate（比如 `common-dlock`）被各服务复用——这里的重复只是教学取舍。
//! 锁的深层话题（看门狗续期、Redlock 争议、需要绝对正确时改用 fencing token）见第 15 章。
//!
//! 本 crate 用它做什么：SaaS 部署了 N 个实例，每个实例都可能定时触发「同步租户 T」。
//! 若不去重，N 个实例会同时拿着同一把 AK 猛拉云厂商接口——既浪费、又可能因并发写
//! 把资产表搞出竞态。抢到 `lock:sync:{tenant_id}` 的那一个实例才真正同步，其余跳过。

use anyhow::{Context, Result};
use redis::aio::ConnectionManager;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// docker compose 起的本地 Redis（无密码，仅供学习）。
pub const DEFAULT_REDIS_URL: &str = "redis://127.0.0.1:6379";

/// 同步锁的 key：一租户一把。
///
/// 抽成函数是为了「构造规则只有一处」——写锁和解锁必须用完全一样的 key，
/// 散在各处手拼字符串迟早对不齐。也方便离线单测这条规则本身。
pub fn sync_lock_key(tenant_id: i64) -> String {
    format!("lock:sync:{tenant_id}")
}

/// 生成一个「本进程本次抢锁」专属的 token，供 [`unlock`] 验明正身：
/// 「这把锁还是我抢到的那把吗？」进程 id 区分「谁」，纳秒时间戳 + 自增序号区分「哪一次」。
pub fn new_token() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{pid}-{nanos}-{seq}")
}

/// 尝试抢锁：`SET key token NX PX ttl`。返回 true = 抢到了。
///
/// - **NX**：key 不存在才设置成功。Redis 单线程执行命令，多实例同抢时只有第一个 SET 成功。
/// - **PX ttl**：给锁设保质期。持有者崩溃也不会死锁，TTL 一到 Redis 自动删 key 释放锁。
///   TTL 要略大于「一次同步预期最长耗时」，否则同步没干完锁就过期，第二个实例趁虚而入。
pub async fn try_lock(
    cm: &mut ConnectionManager,
    key: &str,
    token: &str,
    ttl: Duration,
) -> Result<bool> {
    let reply: Option<String> = redis::cmd("SET")
        .arg(key)
        .arg(token)
        .arg("NX")
        .arg("PX")
        .arg(u64::try_from(ttl.as_millis()).unwrap_or(u64::MAX))
        .query_async(cm)
        .await
        .with_context(|| format!("抢锁 {key} 失败（Redis 命令出错）"))?;
    Ok(reply.is_some())
}

/// 解锁脚本：先 GET 比对 token，是自己的锁才 DEL。Lua 在 Redis 里**原子执行**——
/// 若拆成客户端先 GET 再 DEL，比对通过后、DEL 到达前锁可能恰好过期并被别人抢走，DEL 就误删了别人的锁。
const UNLOCK_SCRIPT: &str = r#"
if redis.call("GET", KEYS[1]) == ARGV[1] then
    return redis.call("DEL", KEYS[1])
else
    return 0
end
"#;

/// 释放锁。返回 true = 确实删掉了自己持有的锁；false = 锁已不是你的（过期/被抢走），什么也没动。
pub async fn unlock(cm: &mut ConnectionManager, key: &str, token: &str) -> Result<bool> {
    let deleted: i64 = redis::Script::new(UNLOCK_SCRIPT)
        .key(key)
        .arg(token)
        .invoke_async(cm)
        .await
        .with_context(|| format!("解锁 {key} 失败（Redis 脚本出错）"))?;
    Ok(deleted == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn lock_key_is_per_tenant() {
        assert_eq!(sync_lock_key(1), "lock:sync:1");
        // 不同租户不同锁——否则租户 1 同步时会挡住租户 2。
        assert_ne!(sync_lock_key(1), sync_lock_key(2));
    }

    #[test]
    fn tokens_are_unique() {
        let tokens: HashSet<String> = (0..1000).map(|_| new_token()).collect();
        assert_eq!(tokens.len(), 1000);
    }
}
