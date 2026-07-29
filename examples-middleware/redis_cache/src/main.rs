//! 演示入口:同一个 id 读两次,直观感受缓存的价值——
//! 第一次未命中要走 300ms 的「慢查询」,第二次直接命中缓存,微秒~毫秒级返回。
//! 需要先启动 Redis:在 examples-middleware/ 目录下 `docker compose up -d redis`。

use anyhow::Result;
use redis_cache::{Todo, connect, get_todo_cached, update_todo};
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<()> {
    let redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/".to_string());

    let mut cm = match connect(&redis_url).await {
        Ok(cm) => cm,
        Err(e) => {
            eprintln!("连接 Redis 失败: {e:#}");
            eprintln!("提示: 请先在 examples-middleware/ 目录执行 `docker compose up -d redis`");
            std::process::exit(1);
        }
    };

    let id = 1;

    // 先清掉可能残留的缓存,保证第一次读一定未命中,演示效果稳定。
    let todo = Todo { id, title: String::new(), done: false };
    update_todo(&mut cm, &todo).await?;

    // 第一次读:缓存未命中 → 慢查询 → 回填缓存。
    let start = Instant::now();
    let first = get_todo_cached(&mut cm, id).await?;
    println!("第一次读取(未命中,回源): {:?}, 耗时 {:?}", first, start.elapsed());

    // 第二次读:直接命中缓存,不再碰「数据库」。
    let start = Instant::now();
    let second = get_todo_cached(&mut cm, id).await?;
    println!("第二次读取(命中缓存):   {:?}, 耗时 {:?}", second, start.elapsed());

    // 写路径:更新「库」后删缓存,下一次读会重新回源拿最新值。
    let updated = Todo { id, title: "更新后的标题".to_string(), done: true };
    update_todo(&mut cm, &updated).await?;
    let start = Instant::now();
    let third = get_todo_cached(&mut cm, id).await?;
    println!("更新并删缓存后再读(重新回源): {:?}, 耗时 {:?}", third, start.elapsed());

    Ok(())
}
