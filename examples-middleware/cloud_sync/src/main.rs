//! 可运行演示：给租户存三朵云的凭证 → 同步 → 打印报告 + 列出归一化后的资产 →
//! 再同步一次演示 upsert（更新而非重复插入）。
//!
//! 运行前先起服务：`docker compose up -d postgres redis`（在 examples-middleware/ 下）。
//! 然后 `cargo run -p cloud_sync`。连不上会打印 docker compose 提示后退出。

use cloud_sync::credentials::{self, Credential};
use cloud_sync::crypto::Keyring;
use cloud_sync::lock::DEFAULT_REDIS_URL;
use cloud_sync::sync;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info,cloud_sync=info")
        .init();

    // 演示用固定租户。真实系统里 tenant_id 来自 JWT（第 17 章），不会硬编码。
    let tenant_id: i64 = 1;

    // 密钥装配：教学用环境变量 + 默认值（见 crypto.rs 顶部警告）；生产接 KMS。
    let keyring = Keyring::from_env()?;

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://tutorial:tutorial@localhost:5432/todos".into());
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| DEFAULT_REDIS_URL.into());

    // connect 内部含幂等迁移（建 cloudsync schema + credentials/assets 两张表）。
    let pool = credentials::connect(&database_url).await?;
    let mut cm = redis::Client::open(redis_url.as_str())?
        .get_connection_manager()
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "{e}\n提示：请先在 examples-middleware/ 下执行 `docker compose up -d redis`"
            )
        })?;

    println!("== 1. 给租户 {tenant_id} 存三朵云的凭证（加密入库） ==");
    let creds = [
        Credential {
            tenant_id,
            provider: "aliyun".into(),
            access_key: "LTAI_demo_ak".into(),
            secret_key: "aliyun_demo_sk".into(),
            endpoint: None,
        },
        Credential {
            tenant_id,
            provider: "aws".into(),
            access_key: "AKIA_demo_ak".into(),
            secret_key: "aws_demo_sk".into(),
            endpoint: None,
        },
        Credential {
            tenant_id,
            provider: "private".into(),
            access_key: "priv_demo_ak".into(),
            secret_key: "priv_demo_sk".into(),
            // 私有云演示自定义 endpoint。
            endpoint: Some("https://cloud.customer-idc.example".into()),
        },
    ];
    for c in &creds {
        // 盲索引去重：重复运行本演示时同一把 AK 不会被重复插入（返回 false）。
        let inserted = credentials::save_credential(&pool, &keyring, c).await?;
        println!(
            "  {} / {}：{}",
            c.provider,
            c.access_key,
            if inserted { "新增" } else { "已存在（盲索引去重）" }
        );
    }
    println!();

    println!("== 2. 首次同步：抢锁 → 解密凭证 → 限流 → 拉取 → 归一化 upsert ==");
    let report = sync::sync_tenant(&pool, &mut cm, &keyring, tenant_id).await?;
    print_report(&report);

    println!("== 3. 列出归一化后的资产（三朵云混在一张表里，统一模型） ==");
    let assets = sync::list_assets(&pool, tenant_id).await?;
    for a in &assets {
        println!(
            "  [{:<7}] {:<11} {:<16} name={:<14} region={}",
            a.provider, a.asset_type, a.external_id, a.name, a.region
        );
    }
    println!("→ 阿里云 ECS / AWS EC2 / 私有云 VM 字段被抹平成同一个 CloudAsset\n");

    println!("== 4. 再同步一次：演示 upsert（更新而非重复插入） ==");
    let report2 = sync::sync_tenant(&pool, &mut cm, &keyring, tenant_id).await?;
    print_report(&report2);
    let assets2 = sync::list_assets(&pool, tenant_id).await?;
    println!(
        "→ 资产总数：第一次 {} 条，第二次仍 {} 条（没翻倍）；第二次全部走 UPDATE 分支",
        assets.len(),
        assets2.len()
    );

    Ok(())
}

fn print_report(r: &sync::SyncReport) {
    if !r.ran {
        println!("  未执行（同步锁被别的实例占用，正常的集群去重）\n");
        return;
    }
    println!(
        "  处理凭证 {} 份 | 新增 {} | 更新 {} | 限流跳过 {} | 未支持跳过 {} | 落库合计 {}\n",
        r.credentials,
        r.inserted,
        r.updated,
        r.rate_limited,
        r.unsupported,
        r.total()
    );
}
