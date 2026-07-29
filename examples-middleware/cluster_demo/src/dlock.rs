//! 分布式锁（Redis 版）。
//!
//! 单机的 `Mutex` 锁的是同一个进程里的线程；一旦服务部署了多个实例，
//! 「同一时刻只允许一个人干活」（跑定时任务、扣同一份库存……）就得把锁
//! 放到所有实例都能访问的外部存储上。Redis 单线程执行命令的特性
//! 让它天然适合做这件事，核心只有两条命令：
//!
//! - 抢锁：`SET key token NX PX ttl`（见 [`try_lock`]）
//! - 放锁：Lua 脚本「GET 比对 token 后才 DEL」（见 [`unlock`]）
//!
//! 进阶话题（本示例点到为止，注释里有展开）：看门狗续期、Redlock 争议、
//! 需要绝对正确时改用 etcd/数据库锁 + fencing token。

use anyhow::{Context, Result};
use redis::aio::ConnectionManager;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// docker compose 起的本地 Redis（无密码，仅供学习）。
pub const DEFAULT_REDIS_URL: &str = "redis://127.0.0.1:6379";

/// 生成一个「本进程本次抢锁」专属的 token。
///
/// token 的唯一使命是让 unlock 能验明正身：「这把锁还是我抢到的那把吗？」
/// 所以它只需要在锁的竞争者之间不撞车即可，不必是密码学随机数——
/// 进程 id 区分「谁」，纳秒时间戳 + 进程内自增序号区分「哪一次」，
/// 拼起来就够了，不值得为此引入随机数依赖。
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
/// 一条命令里藏着分布式锁的两大支柱：
/// - **NX**（Not eXists）：key 不存在才能设置成功。Redis 单线程执行命令,
///   多个实例同时抢时命令必然有先后——只有第一个人 SET 成功，
///   后来者拿到 nil。「谁先到谁得锁」的互斥就这么成立了。
/// - **PX ttl**（毫秒过期）：给锁设个保质期。持有者若中途崩溃，
///   没有任何人会来解锁，但 TTL 一到 Redis 自动删 key，锁自动释放——
///   这是防死锁的保险丝。没有 TTL 的分布式锁等于埋了颗雷。
///
/// 没抢到怎么办由调用方决定：放弃、稍后重试、或排队等——这不是锁本身的职责。
pub async fn try_lock(
    cm: &mut ConnectionManager,
    key: &str,
    token: &str,
    ttl: Duration,
) -> Result<bool> {
    // 成功时 Redis 回 "OK"，NX 未通过（别人持有）时回 nil，
    // 对应这里的 Some/None。
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

/// 解锁脚本：先 GET 比对 token，是自己的锁才 DEL。
/// Lua 脚本在 Redis 里**原子执行**，比对和删除之间不可能插进别的命令——
/// 若拆成客户端先 GET 再 DEL 两步,比对通过后、DEL 到达前锁可能恰好过期
/// 并被别人抢走，DEL 就又误删了别人的锁,等于白比对。
const UNLOCK_SCRIPT: &str = r#"
if redis.call("GET", KEYS[1]) == ARGV[1] then
    return redis.call("DEL", KEYS[1])
else
    return 0
end
"#;

/// 释放锁。返回 true = 确实删掉了自己持有的锁；
/// false = 锁已不是你的（早已过期，或过期后被别人抢走），什么也没动。
///
/// 为什么不能直接 DEL？想象这个时序：
/// 1. 实例 A 抢到锁（TTL 10 秒），干活干了 12 秒——干活超时了；
/// 2. 第 10 秒锁过期自动释放，实例 B 抢到了锁；
/// 3. 第 12 秒 A 干完活，无脑 DEL——删掉的是 **B 正持有的锁**！
///    接着实例 C 又能抢到锁，B 和 C 同时在临界区里，互斥彻底破产。
///
/// token 用随机值就是为这一步准备的：DEL 前先验「锁里存的还是我的 token 吗」，
/// 是才删。上面的时序里 A 会发现 token 对不上，安全地放弃（返回 false）。
///
/// 进阶话题，点到为止：
/// - **TTL 内没干完活怎么办？** 后台起个「看门狗」任务，活着就定期给锁续期
///   （PEXPIRE），Java 的 Redisson 就内置了这个机制。TTL 定短了靠续期救，
///   定长了故障恢复慢，是个权衡。
/// - **Redis 主从切换下锁不绝对可靠**：主节点确认抢锁后还没把 key 同步给
///   从节点就宕机，切换后锁凭空消失，两个人会同时持锁。官方的 Redlock
///   算法（多个独立 Redis 上多数派抢锁）试图缓解，但其安全性在业界
///   （Martin Kleppmann vs antirez 之争）一直有争议。
/// - **需要绝对正确时**（比如涉及钱），别把正确性押在 Redis 锁上：改用
///   etcd/ZooKeeper（共识协议保证）或数据库锁，并配合 fencing token
///   （单调递增的持锁编号，资源方拒绝旧编号的写入）做最后一道防线。
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
