//! 集成测试：真连 etcd / Redis 验证注册发现与分布式锁的完整闭环。
//!
//! 需要先启动服务，因此默认全部 #[ignore]：
//!   cd examples-middleware && docker compose up -d etcd redis
//!   cargo test -p cluster_demo -- --ignored
//!
//! service 名 / lock key 都带随机后缀（复用 new_token）：测试之间、
//! 测试与手动演示之间互不污染，也允许并行跑。

use cluster_demo::discovery::{discover, register};
use cluster_demo::dlock::{new_token, try_lock, unlock};
use std::time::Duration;

async fn etcd() -> etcd_client::Client {
    let endpoint =
        std::env::var("ETCD_ENDPOINT").unwrap_or_else(|_| "localhost:2379".to_string());
    etcd_client::Client::connect([endpoint.as_str()], None)
        .await
        .expect("连不上 etcd：需要先 docker compose up -d etcd redis")
}

async fn redis_cm() -> redis::aio::ConnectionManager {
    let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    redis::Client::open(url.as_str())
        .expect("REDIS_URL 格式不对")
        .get_connection_manager()
        .await
        .expect("连不上 Redis：需要先 docker compose up -d etcd redis")
}

#[tokio::test]
#[ignore = "需要先 docker compose up -d etcd redis"]
async fn register_then_discover_then_deregister() {
    let client = etcd().await;
    // 随机服务名：不污染真实注册表，也不受历史残留影响
    let service = format!("it-svc-{}", new_token());
    let addr = "10.0.0.1:8080";

    // 注册后立刻可见
    let reg = register(&client, &service, addr, 5).await.expect("注册失败");
    let instances = discover(&client, &service).await.expect("discover 失败");
    assert_eq!(instances, vec![addr.to_string()], "注册后应能发现该实例");

    // 显式撤销：租约被 revoke，key 立刻消失，无需等 TTL
    reg.deregister().await.expect("deregister 失败");
    let instances = discover(&client, &service).await.expect("discover 失败");
    assert!(instances.is_empty(), "撤销注册后实例应立刻不可见");
}

#[tokio::test]
#[ignore = "需要先 docker compose up -d etcd redis"]
async fn lock_mutual_exclusion_and_safe_unlock() {
    let mut cm = redis_cm().await;
    // 随机 lock key：与其他测试/演示互不干扰
    let key = format!("it-lock-{}", new_token());
    let token_a = new_token();
    let token_b = new_token();
    let ttl = Duration::from_secs(10);

    // A 抢到锁；B 紧随其后必然失败（NX 只放第一个人过）
    assert!(try_lock(&mut cm, &key, &token_a, ttl).await.expect("抢锁失败"));
    assert!(!try_lock(&mut cm, &key, &token_b, ttl).await.expect("抢锁失败"));

    // 错 token 解锁不生效：B 删不掉 A 的锁，锁仍被持有
    assert!(!unlock(&mut cm, &key, &token_b).await.expect("解锁失败"));
    assert!(
        !try_lock(&mut cm, &key, &token_b, ttl).await.expect("抢锁失败"),
        "错 token 解锁后锁必须仍在 A 手里"
    );

    // 正确 unlock 后锁真正释放，B 可以再抢
    assert!(unlock(&mut cm, &key, &token_a).await.expect("解锁失败"));
    assert!(try_lock(&mut cm, &key, &token_b, ttl).await.expect("抢锁失败"));

    // 收尾：删掉测试 key，不给 Redis 留垃圾
    assert!(unlock(&mut cm, &key, &token_b).await.expect("解锁失败"));
}
