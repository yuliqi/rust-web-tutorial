//! 可运行的最小演示：服务注册/发现 + 客户端负载均衡 + 分布式锁。
//! 「为什么需要这些」见 lib.rs 与各模块文件头的教学注释。
//!
//! 运行前先启动 etcd 和 Redis：
//!   cd examples-middleware && docker compose up -d etcd redis
//! 然后 `cargo run -p cluster_demo`。

use anyhow::{Context, Result};
use cluster_demo::discovery::{
    DEFAULT_ETCD_ENDPOINT, ServiceEvent, discover, pick_round_robin, register, watch_service,
};
use cluster_demo::dlock::{DEFAULT_REDIS_URL, new_token, try_lock, unlock};
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    // 环境变量可覆盖，默认连 docker compose 起的本地实例。
    let etcd_endpoint =
        std::env::var("ETCD_ENDPOINT").unwrap_or_else(|_| DEFAULT_ETCD_ENDPOINT.to_string());
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| DEFAULT_REDIS_URL.to_string());

    // 连不上时最常见的原因是容器没起，把解决办法直接写进错误信息里。
    let etcd = etcd_client::Client::connect([etcd_endpoint.as_str()], None)
        .await
        .with_context(|| {
            format!(
                "连不上 etcd（{etcd_endpoint}）？请先在 examples-middleware 目录执行 \
                 `docker compose up -d etcd redis`"
            )
        })?;
    let mut redis_cm = redis::Client::open(redis_url.as_str())
        .context("REDIS_URL 格式不对")?
        .get_connection_manager()
        .await
        .with_context(|| {
            format!(
                "连不上 Redis（{redis_url}）？请先在 examples-middleware 目录执行 \
                 `docker compose up -d etcd redis`"
            )
        })?;

    // ---------- 第一幕：服务注册与发现 ----------
    let service = "hello-service";

    // 先订阅再注册，实例上下线的推送一条不落。
    let (_watcher, _watch_task) = watch_service(&etcd, service, |event| match event {
        ServiceEvent::Up { addr } => tracing::info!(%addr, "[watch] 实例上线"),
        ServiceEvent::Down { addr } => tracing::info!(%addr, "[watch] 实例下线"),
    })
    .await?;

    // 假装有两个实例启动了（真实世界里这两行分别跑在两台机器上）。
    let reg_a = register(&etcd, service, "127.0.0.1:8081", 5).await?;
    let reg_b = register(&etcd, service, "127.0.0.1:8082", 5).await?;
    tracing::info!("已注册 2 个实例（TTL 5 秒，后台心跳自动续租）");

    // 给 watch 推送留一点时间，让日志顺序符合直觉。
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 消费方视角：查注册表拿到当前活着的实例名单。
    let instances = discover(&etcd, service).await?;
    tracing::info!(?instances, "discover 结果");

    // 客户端负载均衡：4 次请求轮流打到两个实例上。
    let counter = AtomicUsize::new(0);
    for i in 1..=4 {
        let picked = pick_round_robin(&instances, &counter)
            .context("没有可用实例（正常场景下调用方要处理这种情况）")?;
        tracing::info!("第 {i} 次请求 -> {picked}");
    }

    // ---------- 第二幕：分布式锁 ----------
    let lock_key = "cluster_demo:report_job:lock";
    // 两个「进程」各自生成 token（真实世界里它们在不同机器上跑同一段代码）。
    let token_a = new_token();
    let token_b = new_token();

    let a_got = try_lock(&mut redis_cm, lock_key, &token_a, Duration::from_secs(10)).await?;
    let b_got = try_lock(&mut redis_cm, lock_key, &token_b, Duration::from_secs(10)).await?;
    tracing::info!("进程 A 抢锁：{}；进程 B 抢锁：{}", a_got, b_got);
    assert!(a_got && !b_got, "同一把锁只能有一个持有者");

    // B 拿错误的 token 也删不掉 A 的锁——Lua 脚本验身后才 DEL。
    let stolen = unlock(&mut redis_cm, lock_key, &token_b).await?;
    tracing::info!("进程 B 试图用自己的 token 解 A 的锁：{}（锁安然无恙）", stolen);

    // A 干完活，正常释放；B 这才抢得到。
    let released = unlock(&mut redis_cm, lock_key, &token_a).await?;
    let b_retry = try_lock(&mut redis_cm, lock_key, &token_b, Duration::from_secs(10)).await?;
    tracing::info!("进程 A 释放锁：{}；进程 B 再抢：{}", released, b_retry);
    unlock(&mut redis_cm, lock_key, &token_b).await?; // 收尾，别把锁留到下次运行

    // ---------- 谢幕：优雅下线 ----------
    // 显式 deregister：撤销租约，key 立刻消失（watch 会推送两条「实例下线」）。
    reg_a.deregister().await?;
    reg_b.deregister().await?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let after = discover(&etcd, service).await?;
    tracing::info!(?after, "下线后的 discover 结果（应为空）");

    tracing::info!("演示结束");
    Ok(())
}
