//! 集成测试：真连 Postgres / Redis，验证同步引擎的端到端行为。
//!
//! 需要先启动服务，因此默认全部 #[ignore]：
//!   cd examples-middleware && docker compose up -d postgres redis
//!   cargo test -p cloud_sync -- --ignored
//!
//! 每个用例用**随机 tenant_id**（纳秒时间戳）：与历史数据、并行用例互不干扰，
//! 断言只针对本用例造出来的租户，共享的 todos 库里其他数据不影响结果。
//!
//! 密钥用 `Keyring::from_env()`（默认=教学密钥），与 main 演示同源。

use cloud_sync::credentials::{self, Credential};
use cloud_sync::crypto::Keyring;
use cloud_sync::lock::DEFAULT_REDIS_URL;
use cloud_sync::sync;

fn database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://tutorial:tutorial@localhost:5432/todos".to_string())
}

async fn redis_cm() -> redis::aio::ConnectionManager {
    let url = std::env::var("REDIS_URL").unwrap_or_else(|_| DEFAULT_REDIS_URL.to_string());
    redis::Client::open(url.as_str())
        .expect("REDIS_URL 格式不对")
        .get_connection_manager()
        .await
        .expect("连不上 Redis：需要先 docker compose up -d postgres redis")
}

/// 唯一租户 id：微秒时间戳（避开历史/上次运行的残留）+ 进程内自增序号
/// （保证**本次运行内每次调用都不同**，杜绝同一次运行里两个用例撞号导致共用 assets 行）。
fn unique_tenant_id() -> i64 {
    use std::sync::atomic::{AtomicI64, Ordering};
    static SEQ: AtomicI64 = AtomicI64::new(0);
    let micros = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros() as i64;
    // *1000 给自增序号留足空间；单次运行只造几个租户，SEQ 远小于 1000，绝不重叠。
    micros.wrapping_mul(1000) + SEQ.fetch_add(1, Ordering::Relaxed)
}

fn three_cloud_creds(tenant_id: i64) -> Vec<Credential> {
    vec![
        Credential {
            tenant_id,
            provider: "aliyun".into(),
            access_key: "LTAI_it".into(),
            secret_key: "sk".into(),
            endpoint: None,
        },
        Credential {
            tenant_id,
            provider: "aws".into(),
            access_key: "AKIA_it".into(),
            secret_key: "sk".into(),
            endpoint: None,
        },
        Credential {
            tenant_id,
            provider: "private".into(),
            access_key: "priv_it".into(),
            secret_key: "sk".into(),
            endpoint: Some("https://idc.example".into()),
        },
    ]
}

/// 主用例：存凭证 → 同步 → 资产落库且带正确 tenant_id → 再同步总数不翻倍（upsert 生效）。
#[tokio::test]
#[ignore = "需要先 docker compose up -d postgres redis"]
async fn sync_then_resync_is_upsert() {
    let pool = credentials::connect(&database_url()).await.expect("连 PG");
    let mut cm = redis_cm().await;
    let keyring = Keyring::from_env().expect("keyring");
    let tenant_id = unique_tenant_id();

    // 存三朵云的凭证。
    for c in three_cloud_creds(tenant_id) {
        assert!(
            credentials::save_credential(&pool, &keyring, &c).await.expect("save"),
            "首次存入应为新增"
        );
    }

    // 首次同步：抢到锁、真正执行，落库若干资产。
    let r1 = sync::sync_tenant(&pool, &mut cm, &keyring, tenant_id)
        .await
        .expect("sync 1");
    assert!(r1.ran, "首次同步应抢到锁并执行");
    assert_eq!(r1.credentials, 3, "应处理三份凭证");
    assert!(r1.inserted > 0, "首次同步应有新增资产");
    assert_eq!(r1.updated, 0, "首次同步不应有更新");
    let total_after_first = r1.total();

    // 资产表里确实有本租户的数据。
    let assets = sync::list_assets(&pool, tenant_id).await.expect("list");
    assert_eq!(
        assets.len(),
        total_after_first,
        "list 的条数应与报告的落库总数一致"
    );
    assert!(!assets.is_empty());
    // 三朵云的资产都在同一张表里（统一模型）。
    for p in ["aliyun", "aws", "private"] {
        assert!(
            assets.iter().any(|a| a.provider == p),
            "应含 {p} 的资产"
        );
    }

    // 再同步一次：同一批 external_id 命中 ON CONFLICT，走 UPDATE，不新增。
    let r2 = sync::sync_tenant(&pool, &mut cm, &keyring, tenant_id)
        .await
        .expect("sync 2");
    assert!(r2.ran);
    assert_eq!(r2.inserted, 0, "第二次同步不应有新增（全是 upsert 更新）");
    assert_eq!(r2.updated, total_after_first, "第二次应全部走更新分支");

    // 资产总数没翻倍。
    let assets2 = sync::list_assets(&pool, tenant_id).await.expect("list 2");
    assert_eq!(
        assets2.len(),
        total_after_first,
        "再同步总数不应翻倍（upsert 生效）"
    );
}

/// 多租户隔离：租户 B 看不到租户 A 的资产，各自同步互不串。
#[tokio::test]
#[ignore = "需要先 docker compose up -d postgres redis"]
async fn tenants_are_isolated() {
    let pool = credentials::connect(&database_url()).await.expect("连 PG");
    let mut cm = redis_cm().await;
    let keyring = Keyring::from_env().expect("keyring");

    let tenant_a = unique_tenant_id();
    // 确保 B 与 A 不同（纳秒可能相邻，+1 兜底）。
    let tenant_b = tenant_a + 1;

    // 只给 A 存凭证并同步；B 什么都不存。
    for c in three_cloud_creds(tenant_a) {
        credentials::save_credential(&pool, &keyring, &c).await.expect("save A");
    }
    let ra = sync::sync_tenant(&pool, &mut cm, &keyring, tenant_a)
        .await
        .expect("sync A");
    assert!(ra.inserted > 0);

    // B 同步：没有任何凭证，处理 0 份、落库 0 条。
    let rb = sync::sync_tenant(&pool, &mut cm, &keyring, tenant_b)
        .await
        .expect("sync B");
    assert!(rb.ran);
    assert_eq!(rb.credentials, 0, "租户 B 没有凭证");
    assert_eq!(rb.total(), 0);

    // A 有资产，B 没有——list_assets 的 WHERE tenant_id 保证了隔离。
    assert!(!sync::list_assets(&pool, tenant_a).await.expect("A assets").is_empty());
    assert!(
        sync::list_assets(&pool, tenant_b).await.expect("B assets").is_empty(),
        "租户 B 不应看到任何资产（更不该看到 A 的）"
    );
}
