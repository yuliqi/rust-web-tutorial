//! 演示入口：串一遍「连库 → 增 → 查 → 改 → 删」。
//! 需要先启动 MySQL：在 examples-middleware/ 目录下 `docker compose up -d mysql`。

use anyhow::Result;
use mysql_demo::{connect, create_todo, delete_todo, get_todo, list_todos, set_done};

#[tokio::main]
async fn main() -> Result<()> {
    // 默认值对准 docker-compose.yml 里的 MySQL 配置，开箱即用；
    // 真实项目里 DATABASE_URL 一定来自环境变量，不会把密码写进代码。
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "mysql://root:tutorial@localhost:3306/todos".to_string());

    // 连不上时最常见的原因就是容器没起,把解法直接印在错误信息里。
    let pool = match connect(&database_url).await {
        Ok(pool) => pool,
        Err(e) => {
            eprintln!("连接 MySQL 失败: {e:#}");
            eprintln!("提示: 请先在 examples-middleware/ 目录执行 `docker compose up -d mysql`");
            std::process::exit(1);
        }
    };

    // 增:MySQL 没有 RETURNING,create_todo 内部用 last_insert_id() 两步走拿回 id。
    let todo = create_todo(&pool, "体验 MySQL 方言").await?;
    println!("created: {todo:?}");

    // 查单条。
    let fetched = get_todo(&pool, todo.id).await?;
    println!("fetched: {fetched:?}");

    // 改:标记完成。
    let updated = set_done(&pool, todo.id, true).await?;
    println!("set_done -> {updated}");

    // 查全部(库里可能有历史数据,这里只打印条数)。
    let all = list_todos(&pool).await?;
    println!("total todos in db: {}", all.len());

    // 删:收尾,让演示可以反复运行而不堆垃圾数据。
    let deleted = delete_todo(&pool, todo.id).await?;
    println!("deleted -> {deleted}");

    Ok(())
}
