//! 时序存储的集成测试（第 28 章配套）。
//!
//! 策略与 todo_api_pg/tests/api.rs 一致：连的是 docker-compose 里同一个 Postgres，
//! 所以每个测试都标 `#[ignore]`——数据库没起时 `cargo test` 依然全绿，要跑它用：
//!
//! ```bash
//! docker compose up -d postgres        # 先在 examples-middleware/ 下起库
//! cargo test -p monitor_demo -- --ignored
//! ```
//!
//! 隔离手法：不假设表是空的。每个测试用**随机指标名**造自己的数据、只按这个名字回查，
//! 从而能容忍并行测试或上次运行的残留行（更严格的做法是独立 schema / 事务回滚，教学从简）。

use monitor_demo::metrics::Metric;
use monitor_demo::store::{self, DEFAULT_DATABASE_URL};

/// 连接串取 DATABASE_URL，缺省对齐 docker-compose.yml。
fn database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_string())
}

/// 随机指标名：纳秒时间戳足以让本机多次运行/并行测试互不撞名。
fn unique_name(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{prefix}_{nanos}")
}

/// save_samples → query_range 一致性：写进去几条，按名字 + 时间范围回查，逐字段对得上。
#[tokio::test]
#[ignore = "需要先 docker compose up -d postgres"]
async fn save_then_query_range_round_trips() {
    let pool = store::connect(&database_url()).await.expect("connect db");
    let name = unique_name("test_metric");

    // 造 3 条：时间递增、其中带 label（验证 JSONB 存取），值各不同。
    let base_ts = 1_700_000_000_000_i64;
    let samples = vec![
        Metric {
            name: name.clone(),
            value: 10.5,
            labels: vec![],
            ts_ms: base_ts,
        },
        Metric {
            name: name.clone(),
            value: 20.0,
            labels: vec![("mount".into(), "/".into())],
            ts_ms: base_ts + 1000,
        },
        Metric {
            name: name.clone(),
            value: 30.25,
            labels: vec![("mount".into(), "/data".into()), ("fs".into(), "apfs".into())],
            ts_ms: base_ts + 2000,
        },
    ];

    let written = store::save_samples(&pool, &samples).await.expect("save");
    assert_eq!(written, 3, "应写入 3 行");

    // 回查覆盖全部三条的时间范围。
    let got = store::query_range(&pool, &name, base_ts, base_ts + 2000)
        .await
        .expect("query_range");
    assert_eq!(got, samples, "回查结果应与写入完全一致（含 label 与顺序）");
}

/// query_range 的时间边界：闭区间含两端；范围外的样本不返回。
#[tokio::test]
#[ignore = "需要先 docker compose up -d postgres"]
async fn query_range_respects_bounds() {
    let pool = store::connect(&database_url()).await.expect("connect db");
    let name = unique_name("bound_metric");
    let base = 2_000_000_000_000_i64;

    let samples: Vec<Metric> = (0..5)
        .map(|i| Metric {
            name: name.clone(),
            value: i as f64,
            labels: vec![],
            ts_ms: base + i * 100,
        })
        .collect();
    store::save_samples(&pool, &samples).await.expect("save");

    // 只取中间三条 [base+100, base+300]（闭区间含两端）。
    let got = store::query_range(&pool, &name, base + 100, base + 300)
        .await
        .expect("query");
    assert_eq!(got.len(), 3, "闭区间应命中 ts=100/200/300 三条");
    assert_eq!(got.first().unwrap().value, 1.0);
    assert_eq!(got.last().unwrap().value, 3.0);
}

/// 空输入不落库、不报错（尽力而为的安全边界）。
#[tokio::test]
#[ignore = "需要先 docker compose up -d postgres"]
async fn save_empty_is_noop() {
    let pool = store::connect(&database_url()).await.expect("connect db");
    let n = store::save_samples(&pool, &[]).await.expect("save empty");
    assert_eq!(n, 0);
}
