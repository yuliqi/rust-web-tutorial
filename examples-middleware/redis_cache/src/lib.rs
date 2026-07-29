//! Redis 做旁路缓存（cache-aside）的完整示例：
//! 读路径「先查缓存,未命中再查库并回填」,写路径「先更新库,再删缓存」。
//! 数据库用一个 sleep 模拟的「慢查询」代替——重点是缓存模式本身,不是 SQL。

use anyhow::{Context, Result};
use redis::AsyncCommands;
use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// 缓存的兜底过期时间(秒)。为什么一定要设 TTL:
/// 删缓存的代码总有可能因为 bug、宕机、网络分区而没执行到,
/// TTL 保证脏数据最多活这么久,是最后一道防线。
pub const CACHE_TTL_SECS: u64 = 60;

/// 模拟的「数据库查询」耗时。真实场景里这可能是一条几十毫秒的复杂 SQL。
pub const SLOW_QUERY_DELAY: Duration = Duration::from_millis(300);

/// 待办事项。缓存里存的是它的 JSON:serde_json 可读性好、跨语言通用,
/// 排查问题时 redis-cli GET 出来直接就能看懂,教学与多数业务场景都够用。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Todo {
    pub id: i64,
    pub title: String,
    pub done: bool,
}

/// 统一生成缓存 key。key 设计的两个惯例:
/// - 业务前缀 "todo:":Redis 是所有业务共用的一个大字典,前缀避免撞 key,也方便按前缀排查;
/// - 版本号 "v1:":哪天 Todo 的 JSON 结构变了,把版本升到 v2,旧缓存自然失效,不用挨个清。
pub fn cache_key(id: i64) -> String {
    format!("todo:v1:{id}")
}

/// 模拟慢查询:睡一会儿再返回数据,代替真实的数据库访问。
/// 返回 Option 是为了如实模拟「库里也没有」的情况——这正是缓存穿透的入口(见下)。
pub async fn slow_db_query(id: i64) -> Option<Todo> {
    tokio::time::sleep(SLOW_QUERY_DELAY).await;
    // 演示用:约定负数 id 表示「库里不存在」。
    if id < 0 {
        return None;
    }
    Some(Todo { id, title: format!("todo #{id} (from slow db)"), done: false })
}

/// 连接 Redis。用 ConnectionManager 而不是裸的 Connection:
/// 裸连接一旦断开(Redis 重启、网络抖动)就永久废了,之后每次请求都报错;
/// ConnectionManager 会自动重连,且可以 Clone 后在多个任务间共享——生产环境的标配。
pub async fn connect(redis_url: &str) -> Result<ConnectionManager> {
    let client = redis::Client::open(redis_url)
        .with_context(|| format!("invalid REDIS_URL: {redis_url}"))?;
    let cm = client
        .get_connection_manager()
        .await
        .with_context(|| format!("connect redis failed: {redis_url}"))?;
    Ok(cm)
}

/// 读路径:cache-aside 的核心三步——查缓存 → 未命中查库 → 回填缓存(带 TTL)。
///
/// 顺带认识缓存的三个经典问题(本示例点到为止,不做完整实现):
/// - 穿透:反复查「库里根本没有」的 key,缓存永远不命中,压力全打到库。
///   对策:把「不存在」也缓存一个短 TTL 的空值,或在入口加布隆过滤器。
/// - 击穿:某个热 key 恰好过期的瞬间,大量请求同时涌向数据库。
///   对策:加互斥锁让一个请求去回源,其余等它回填(singleflight)。
/// - 雪崩:一大批 key 同时过期(比如都设了整点),数据库瞬间被打垮。
///   对策:TTL 加随机抖动,让过期时间散开。
pub async fn get_todo_cached(cm: &mut ConnectionManager, id: i64) -> Result<Option<Todo>> {
    let key = cache_key(id);

    // 1. 先查缓存。GET 一个不存在的 key 返回 nil,对应这里的 None。
    let cached: Option<String> = cm.get(&key).await.context("redis GET")?;
    if let Some(json) = cached {
        let todo: Todo = serde_json::from_str(&json).context("decode cached todo")?;
        return Ok(Some(todo));
    }

    // 2. 未命中,回源查「数据库」(慢)。
    let Some(todo) = slow_db_query(id).await else {
        // 库里也没有:直接返回。若担心穿透,这里可以缓存一个短 TTL 的空标记。
        return Ok(None);
    };

    // 3. 回填缓存。SET key value EX ttl 一条命令搞定「写入 + 设过期」,
    //    比 SET 完再 EXPIRE 两条命令更好:原子,不会出现「写入了却没设过期」的中间态。
    let json = serde_json::to_string(&todo).context("encode todo")?;
    let _: () = cm.set_ex(&key, json, CACHE_TTL_SECS).await.context("redis SET EX")?;

    Ok(Some(todo))
}

/// 写路径:先更新数据库,再删缓存(而不是改缓存)。
/// 为什么删而不是改:两个并发写各自「先更库再改缓存」时,改缓存的顺序可能与更库的
/// 顺序相反,缓存里留下旧值且一直不过期;删缓存则让下一次读重新回源,天然拿到最新值。
pub async fn update_todo(cm: &mut ConnectionManager, todo: &Todo) -> Result<()> {
    // 1. 先更新「数据库」(这里用 sleep 模拟一次写库)。
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 2. 再删缓存。DEL 不存在的 key 也不算错(返回 0),所以不用先查再删。
    let _: () = cm.del(cache_key(todo.id)).await.context("redis DEL")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // 离线单测:key 规范是缓存正确性的根基,前缀或格式一变,老缓存就全部失效了。
    #[test]
    fn cache_key_has_prefix_and_version() {
        assert_eq!(cache_key(42), "todo:v1:42");
        assert_eq!(cache_key(0), "todo:v1:0");
        // 不同 id 必须生成不同 key,否则数据互相覆盖。
        assert_ne!(cache_key(1), cache_key(2));
    }

    // 离线单测:序列化 round-trip——存进 Redis 的 JSON 必须能原样读回来。
    #[test]
    fn todo_json_roundtrip() {
        let todo = Todo { id: 7, title: "学缓存".to_string(), done: true };
        let json = serde_json::to_string(&todo).unwrap();
        let back: Todo = serde_json::from_str(&json).unwrap();
        assert_eq!(back, todo);
    }
}
