//! 集成测试：会话录制与审计的端到端往返，验证堡垒机的灵魂能力。
//!
//! 需要真实 Postgres，默认 `#[ignore]`。先起库再跑：
//! ```bash
//! docker compose up -d postgres           # 在 examples-middleware/ 下
//! cargo test -p webterm_demo -- --ignored  # 显式跑被忽略的用例
//! ```
//!
//! 用例流程：migrate → start_session → 记录若干 input/output → end_session →
//! replay 顺序正确、审计可查。target 带随机后缀，避免历史数据干扰断言。

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::time::{SystemTime, UNIX_EPOCH};
use webterm_demo::session::{
    self, Direction,
};

async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://tutorial:tutorial@localhost:5432/todos".into());
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("连 Postgres 失败：请先 `docker compose up -d postgres`");
    session::migrate(&pool).await.expect("migrate");
    pool
}

/// 唯一后缀，隔离并行/历史数据。
fn unique_target() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("test-target-{nanos}")
}

#[tokio::test]
#[ignore = "需要先 docker compose up -d postgres"]
async fn record_and_replay_roundtrip() {
    let pool = pool().await;
    let target = unique_target();

    // 1) 开会话。
    let session_id = session::start_session(&pool, 42, &target, "10.0.0.9")
        .await
        .expect("start_session");

    // 2) 记录一串交错的 input / output，时间戳递增。
    let base = session::now_ms();
    session::record_event(&pool, session_id, Direction::Input, b"whoami\n", base)
        .await
        .unwrap();
    session::record_event(&pool, session_id, Direction::Output, b"root\n", base + 5)
        .await
        .unwrap();
    session::record_event(&pool, session_id, Direction::Input, b"rm -rf /tmp/x\n", base + 10)
        .await
        .unwrap();
    session::record_event(&pool, session_id, Direction::Output, b"done\n", base + 15)
        .await
        .unwrap();

    // 3) 结束会话。
    session::end_session(&pool, session_id).await.expect("end_session");

    // 4) 回放：顺序必须与写入一致，方向与内容都对得上。
    let events = session::replay(&pool, session_id).await.expect("replay");
    assert_eq!(events.len(), 4, "应回放出 4 条事件");
    assert_eq!(events[0].direction, Direction::Input);
    assert_eq!(events[0].data, "whoami\n");
    assert_eq!(events[1].direction, Direction::Output);
    assert_eq!(events[1].data, "root\n");
    assert_eq!(events[2].direction, Direction::Input);
    assert_eq!(events[2].data, "rm -rf /tmp/x\n"); // 危险命令留痕：谁敲的、何时敲的
    assert_eq!(events[3].data, "done\n");
    // 时间戳单调不减（回放据此确定重放节奏）。
    assert!(events.windows(2).all(|w| w[0].at_ms <= w[1].at_ms));

    // 5) 审计可查：列表里能找到这条会话，且已结束（ended_at 已补上）。
    let sessions = session::list_sessions(&pool).await.expect("list_sessions");
    let ours = sessions
        .iter()
        .find(|s| s.id == session_id)
        .expect("审计列表里应能查到刚才的会话");
    assert_eq!(ours.account_id, 42);
    assert_eq!(ours.target, target);
    assert_eq!(ours.client_ip, "10.0.0.9");
    assert!(ours.ended_at.is_some(), "结束后 ended_at 应已写入");
}

#[tokio::test]
#[ignore = "需要先 docker compose up -d postgres"]
async fn end_session_is_idempotent() {
    let pool = pool().await;
    let target = unique_target();
    let session_id = session::start_session(&pool, 1, &target, "127.0.0.1")
        .await
        .unwrap();
    // 结束两次不应报错（第二次是 no-op：只更新 ended_at IS NULL 的行）。
    session::end_session(&pool, session_id).await.unwrap();
    session::end_session(&pool, session_id).await.unwrap();
}
