//! 集成测试:真连 Redis 验证 cache-aside 的完整生命周期——
//! 未命中(慢) → 回填 → 命中(快) → 更新后失效 → 再次回源。
//! 默认被 ignore;本地想跑:`docker compose up -d redis` 后 `cargo test -p redis_cache -- --ignored`。

use rand::Rng;
use redis::AsyncCommands;
use redis_cache::{SLOW_QUERY_DELAY, Todo, cache_key, connect, get_todo_cached, update_todo};
use std::time::Instant;

fn redis_url() -> String {
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/".to_string())
}

#[tokio::test]
#[ignore = "需要先 docker compose up -d redis"]
async fn miss_fill_hit_invalidate_flow() {
    let mut cm = connect(&redis_url()).await.expect("连接失败,请先 docker compose up -d redis");

    // 随机 id → 随机 key:Redis 是共享服务,固定 key 会被上次运行的残留数据污染。
    let id: i64 = rand::thread_rng().gen_range(1_000_000..i64::MAX);
    let key = cache_key(id);

    // 1. 未命中:第一次读取应该走慢查询,耗时不低于模拟的 DB 延迟。
    let start = Instant::now();
    let first = get_todo_cached(&mut cm, id).await.unwrap().expect("正数 id 应该查得到");
    assert!(start.elapsed() >= SLOW_QUERY_DELAY, "未命中应走慢查询");
    assert_eq!(first.id, id);

    // 2. 回填:此时缓存里应已存在这个 key,且 TTL 已设置(不是永不过期的 -1)。
    let cached: Option<String> = cm.get(&key).await.unwrap();
    assert!(cached.is_some(), "回源之后应已回填缓存");
    let ttl: i64 = cm.ttl(&key).await.unwrap();
    assert!(ttl > 0, "回填的缓存必须带 TTL,实际为 {ttl}");

    // 3. 命中:第二次读取不应再等慢查询,耗时远小于 DB 延迟。
    let start = Instant::now();
    let second = get_todo_cached(&mut cm, id).await.unwrap().unwrap();
    assert!(start.elapsed() < SLOW_QUERY_DELAY / 2, "命中缓存不该走慢查询");
    assert_eq!(second, first);

    // 4. 失效:更新走「先更库,再删缓存」,缓存 key 应消失。
    let updated = Todo { id, title: "updated".to_string(), done: true };
    update_todo(&mut cm, &updated).await.unwrap();
    let after_update: Option<String> = cm.get(&key).await.unwrap();
    assert!(after_update.is_none(), "更新后缓存应已被删除");

    // 5. 再读会重新回源并再次回填(慢一次,换来数据是新的)。
    let third = get_todo_cached(&mut cm, id).await.unwrap().unwrap();
    assert_eq!(third.id, id);

    // 清理:删掉测试产生的 key,不给共享的 Redis 留垃圾。
    let _: () = cm.del(&key).await.unwrap();
}
