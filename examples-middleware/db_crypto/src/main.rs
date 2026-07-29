//! 可运行演示：敏感字段加密入库 → 看数据库里实际存了什么 → 盲索引查询 → 密钥轮换。
//!
//! 运行前先起数据库：`docker compose up -d postgres`（在 examples-middleware/ 下）。
//! 然后 `cargo run -p db_crypto`。重复运行会往表里追加演示行，属正常现象——
//! find_by_phone 会把同号码的历史行一起查出来。

use db_crypto::crypto::{KeyRing, index_key_from_env};
use db_crypto::store;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 密钥装配。教学用环境变量 + 默认值（见 crypto.rs 顶部的显眼警告）；
    // 生产应是：启动时向 KMS 请求解密 DEK，明文密钥只存在于进程内存。
    let keyring = KeyRing::from_env()?; // active = v2，环里同时挂着 v1（解老数据用）
    let index_key = index_key_from_env()?;

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://tutorial:tutorial@localhost:5432/todos".into());
    // connect 内部含幂等迁移；连不上时错误里带 docker compose 提示。
    let pool = store::connect(&database_url).await?;

    println!("== 1. 加密写入两条联系人 ==");
    let id_a = store::insert_contact(
        &pool,
        &keyring,
        &index_key,
        "张三",
        "13800001234",
        "110101199001011234",
    )
    .await?;
    // 李四的手机号故意带连字符——盲索引规范化后与纯数字等价。
    let id_b = store::insert_contact(
        &pool,
        &keyring,
        &index_key,
        "李四",
        "139-0000-5678",
        "310101198512254321",
    )
    .await?;
    println!("插入成功: 张三 id={id_a}, 李四 id={id_b}\n");

    println!("== 2. 数据库里实际存储的行（拖库者看到的就是这些） ==");
    for id in [id_a, id_b] {
        let raw = store::raw_row(&pool, id).await?;
        println!("id={} name={}", raw.id, raw.name);
        println!("  phone_enc   = {}", raw.phone_enc);
        println!("  phone_idx   = {}", raw.phone_idx);
        println!("  id_card_enc = {}", raw.id_card_enc);
    }
    println!("→ 没有任何明文手机号/身份证；密文带 v2 版本前缀，索引列是 HMAC 的 hex\n");

    println!("== 3. 按手机号盲索引查询（用户输入带空格也能命中） ==");
    let hits = store::find_by_phone(&pool, &keyring, &index_key, "138 0000 1234").await?;
    for c in &hits {
        println!(
            "命中: id={} name={} phone={} id_card={}",
            c.id, c.name, c.phone, c.id_card
        );
    }
    println!("→ WHERE 条件是 HMAC 值，数据库端全程没见过手机号明文\n");

    println!("== 4. 密钥轮换演示：v1 老数据 → rotate_all → v2 ==");
    // 用「active = v1」的密钥环插入一条，模拟轮换前的历史数据。
    let keyring_v1 = keyring.with_active_version(1)?;
    let id_old = store::insert_contact(
        &pool,
        &keyring_v1,
        &index_key,
        "王五(老数据)",
        "13700009999",
        "440101197803150011",
    )
    .await?;
    let before = store::raw_row(&pool, id_old).await?;
    println!("轮换前: phone_enc = {}", before.phone_enc);

    // 全表轮换：v1 行重加密为 v2，已是 v2 的行幂等跳过。
    let rotated = store::rotate_all(&pool, &keyring).await?;
    println!("rotate_all 改写了 {rotated} 行");

    let after = store::raw_row(&pool, id_old).await?;
    println!("轮换后: phone_enc = {}", after.phone_enc);
    let check = store::find_by_phone(&pool, &keyring, &index_key, "13700009999").await?;
    println!(
        "轮换后仍可解密: {:?}",
        check
            .iter()
            .map(|c| (&c.name, &c.phone))
            .collect::<Vec<_>>()
    );
    println!("→ 前缀 v1 变 v2，明文不变；全表无 v1 残留后，v1 密钥才允许下线");
    Ok(())
}
