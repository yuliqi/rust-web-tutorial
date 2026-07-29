//! 集成测试：验证「加密入库 → 库里无明文 → 盲索引命中 → 轮换后仍可读」全链路。
//!
//! 与 todo_api_pg 同一策略：连的是 docker-compose 里共享的 todos 库，因此
//! 1. 全部标 `#[ignore]`——数据库没起时 `cargo test` 依然全绿，
//!    想跑用 `cargo test -p db_crypto -- --ignored`；
//! 2. 断言容忍历史数据：手机号用纳秒时间戳生成、只认自己插入的 id，
//!    上次运行或 main 演示留下的行不影响结果。
//!
//! 注意：密钥必须用 `KeyRing::from_env()`（默认=教学密钥），与 main 演示同源——
//! rotate_all 是全表操作，会碰到历史行，密钥不一致会解不开而报错。

use db_crypto::crypto::{KeyRing, index_key_from_env};
use db_crypto::store;

async fn test_pool() -> sqlx::PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://tutorial:tutorial@localhost:5432/todos".into());
    store::connect(&url).await.expect("db")
}

/// 随机手机号：纳秒时间戳取 9 位，前缀 19 凑满 11 位。
/// 本机多次运行/并行测试互不撞号，从而在共享表里只认自己的数据。
fn unique_phone() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("19{:09}", nanos % 1_000_000_000)
}

#[tokio::test]
#[ignore = "需要先 docker compose up -d postgres"]
async fn encrypt_insert_query_rotate_roundtrip() {
    let pool = test_pool().await;
    let keyring = KeyRing::from_env().expect("keyring"); // active = v2
    let keyring_v1 = keyring.with_active_version(1).expect("v1 ring");
    let index_key = index_key_from_env().expect("index key");

    let phone = unique_phone();
    let id_card = "110101199001011234";

    // 用 v1 密钥插入，模拟轮换前的历史数据。
    let id = store::insert_contact(&pool, &keyring_v1, &index_key, "集成测试", &phone, id_card)
        .await
        .expect("insert");

    // 1. raw 行断言：数据库里不含明文手机号/身份证，密文带 v1 前缀。
    let raw = store::raw_row(&pool, id).await.expect("raw row");
    assert!(!raw.phone_enc.contains(&phone), "密文列不应含明文手机号");
    assert!(!raw.phone_idx.contains(&phone), "盲索引列不应含明文手机号");
    assert!(!raw.id_card_enc.contains(id_card), "密文列不应含明文身份证");
    assert!(
        raw.phone_enc.starts_with("v1:"),
        "老数据应是 v1 前缀: {}",
        raw.phone_enc
    );
    assert_eq!(raw.phone_idx.len(), 64, "盲索引应是 SHA-256 的 hex");

    // 2. 盲索引查询命中（输入带空格照样命中），解密结果与写入一致。
    let spaced = format!("{} {}", &phone[..3], &phone[3..]);
    let hits = store::find_by_phone(&pool, &keyring, &index_key, &spaced)
        .await
        .expect("find by phone");
    let me = hits
        .iter()
        .find(|c| c.id == id)
        .expect("应命中自己插入的行");
    assert_eq!(me.phone, phone);
    assert_eq!(me.id_card, id_card);

    // 3. 全表轮换：至少把自己这条 v1 行改写成 v2。
    let rotated = store::rotate_all(&pool, &keyring)
        .await
        .expect("rotate all");
    assert!(rotated >= 1, "至少应轮换本测试插入的 v1 行");

    // 4. 轮换后：版本前缀更新为 active(v2)，仍可解密且明文不变。
    let raw_after = store::raw_row(&pool, id)
        .await
        .expect("raw row after rotate");
    assert!(
        raw_after.phone_enc.starts_with("v2:"),
        "轮换后应是 v2 前缀: {}",
        raw_after.phone_enc
    );
    assert_ne!(raw_after.phone_enc, raw.phone_enc);
    // 盲索引与加密密钥版本无关，轮换不应动它。
    assert_eq!(raw_after.phone_idx, raw.phone_idx);

    let hits_after = store::find_by_phone(&pool, &keyring, &index_key, &phone)
        .await
        .expect("find after rotate");
    let me_after = hits_after
        .iter()
        .find(|c| c.id == id)
        .expect("轮换后仍应命中");
    assert_eq!(me_after.phone, phone);
    assert_eq!(me_after.id_card, id_card);

    // 5. 幂等：再跑一次 rotate_all，自己这行已是 v2，不应再被改写。
    store::rotate_all(&pool, &keyring)
        .await
        .expect("rotate again");
    let raw_twice = store::raw_row(&pool, id)
        .await
        .expect("raw row after 2nd rotate");
    assert_eq!(
        raw_twice.phone_enc, raw_after.phone_enc,
        "重复轮换应是 no-op"
    );
}
