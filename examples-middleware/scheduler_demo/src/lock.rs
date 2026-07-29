//! 分布式锁（Redis 版）——定时任务集群去重的第一道防线。
//!
//! **这与第 15 章 `cluster_demo/src/dlock.rs` 是同一套机制**（SET NX 抢锁、
//! Lua 脚本验身解锁），此处刻意重写一份是为了让本示例自包含、能单独 `cargo run`，
//! 而不是跨 crate 去 `use cluster_demo::dlock`。生产项目里这类基础能力应当
//! 抽成一个公共 crate（比如 `common-dlock`）被各服务复用，而不是到处复制粘贴——
//! 这里的重复只是教学取舍。锁的深层话题（看门狗续期、Redlock 争议、
//! 需要绝对正确时改用 etcd/数据库锁 + fencing token）见第 15 章那份文件，本处从略。

use anyhow::{Context, Result};
use redis::aio::ConnectionManager;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// docker compose 起的本地 Redis（无密码，仅供学习）。
pub const DEFAULT_REDIS_URL: &str = "redis://127.0.0.1:6379";

/// 生成一个「本进程本次抢锁」专属的 token。
///
/// token 的唯一使命是让 [`unlock`] 能验明正身：「这把锁还是我抢到的那把吗？」
/// 所以它只需要在锁的竞争者之间不撞车，不必是密码学随机数——
/// 进程 id 区分「谁」，纳秒时间戳 + 进程内自增序号区分「哪一次」，拼起来就够了。
pub fn new_token() -> String {
    // 自增序号兜底：即使系统时钟精度不足以区分两次极快的连续调用，
    // 序号也能保证进程内生成的 token 互不相同。
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
/// - **NX**：key 不存在才设置成功。Redis 单线程执行命令，多个实例同时抢时
///   命令必然有先后，只有第一个人 SET 成功——「谁先到谁得锁」的互斥就此成立。
/// - **PX ttl**：给锁设保质期。持有者中途崩溃也不会有人来解锁，但 TTL 一到
///   Redis 自动删 key，锁自动释放——这是防死锁的保险丝，没有 TTL 的分布式锁是颗雷。
///
/// 用在定时任务上：TTL 要略大于「一次任务预期最长耗时」，否则任务还没干完锁就
/// 过期，第二个实例会趁虚而入（这正是 [`crate::jobs`] 里 DB 唯一约束要兜底的情形）。
pub async fn try_lock(
    cm: &mut ConnectionManager,
    key: &str,
    token: &str,
    ttl: Duration,
) -> Result<bool> {
    // 成功回 "OK"，NX 未通过（别人持有）回 nil，对应这里的 Some/None。
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

/// 解锁脚本：先 GET 比对 token，是自己的锁才 DEL。Lua 在 Redis 里**原子执行**，
/// 比对和删除之间插不进别的命令——若拆成客户端先 GET 再 DEL 两步，
/// 比对通过后、DEL 到达前锁可能恰好过期并被别人抢走，DEL 就误删了别人的锁。
const UNLOCK_SCRIPT: &str = r#"
if redis.call("GET", KEYS[1]) == ARGV[1] then
    return redis.call("DEL", KEYS[1])
else
    return 0
end
"#;

/// 释放锁。返回 true = 确实删掉了自己持有的锁；
/// false = 锁已不是你的（早已过期、或过期后被别人抢走），什么也没动。
///
/// 为什么不能无脑 DEL：A 抢锁（TTL 10s）却干了 12s，第 10 秒锁过期、B 抢到，
/// 第 12 秒 A 干完无脑 DEL 删掉的是 **B 正持有的锁**。token 验身就是为这一步准备的。
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
    fn tokens_are_unique() {
        // 连续快速生成也不允许重复——这是 unlock 验身的前提
        let tokens: HashSet<String> = (0..1000).map(|_| new_token()).collect();
        assert_eq!(tokens.len(), 1000);
    }

    #[test]
    fn token_contains_pid() {
        // token 里带进程 id：跨进程竞争同一把锁时，天然不会撞车
        assert!(new_token().starts_with(&std::process::id().to_string()));
    }
}
