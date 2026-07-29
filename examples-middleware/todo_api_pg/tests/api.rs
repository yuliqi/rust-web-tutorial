//! 集成测试：策略与 SQLite 版一致——不监听端口，用 tower 的 `oneshot`
//! 把 Request 直接喂给 `app()` 返回的 Router，走完提取器→handler→错误转换全链路。
//!
//! 差异点：测试隔离。SQLite 版每个测试起一个独立内存库（`sqlite::memory:`），
//! 天然隔离；Postgres 没有等价的「一次性内存库」，所有测试连的是 docker-compose
//! 里同一个 todos 库。因此：
//! 1. 每个测试标 `#[ignore]`——数据库没起时 `cargo test` 依然全绿，
//!    想跑它们用 `cargo test -p todo_api_pg -- --ignored`；
//! 2. 断言不假设库是空的：每个测试用随机 title 创建自己的数据、按自己拿到的
//!    id 查询，能容忍并行测试或上次运行的残留行。
//!    （更进一步的做法是每个测试建独立 schema 或用事务回滚，教学从简。）

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use todo_api_pg::{app, db, AppState};
use tower::ServiceExt;

/// 与生产同一套组装：连接串取 DATABASE_URL，缺省对齐 docker-compose.yml。
async fn test_app() -> axum::Router {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://tutorial:tutorial@localhost:5432/todos".into());
    let pool = db::connect(&url).await.expect("db");
    app(AppState { pool })
}

/// 随机 title：纳秒时间戳足以让本机多次运行/并行测试互不撞名，
/// 从而在共享库里也能只认自己造的数据。
fn unique_title(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{prefix}-{nanos}")
}

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(json!({}))
}

#[tokio::test]
#[ignore = "需要先 docker compose up -d postgres"]
async fn health_ok() {
    let app = test_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

// CRUD 全流程串成一个测试：create → get → patch → list 过滤 → delete → 404。
// oneshot 会消费 Router，所以除最后一次外都用 app.clone()。
#[tokio::test]
#[ignore = "需要先 docker compose up -d postgres"]
async fn todo_crud_flow() {
    let app = test_app().await;
    let title = unique_title("crud");

    // create：断言只看本次创建返回的内容，不数库里总共有几行。
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/todos")
                .header("content-type", "application/json")
                .body(Body::from(json!({ "title": title }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = body_json(response).await;
    assert_eq!(created["title"], title.as_str());
    assert_eq!(created["done"], false);
    let id = created["id"].as_i64().unwrap();

    // get
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/todos/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let fetched = body_json(response).await;
    assert_eq!(fetched["title"], title.as_str());

    // patch（底下是单条 UPDATE ... RETURNING，见 services/todos.rs）
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/todos/{id}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"done": true}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let updated = body_json(response).await;
    assert_eq!(updated["done"], true);
    // 没传 title 的字段应保持原值（COALESCE 生效的证据）。
    assert_eq!(updated["title"], title.as_str());

    // list 过滤：共享库里可能有别的行，只断言「包含我这条」，不断言总数。
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/todos?done=true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let list = body_json(response).await;
    assert!(list.as_array().unwrap().iter().any(|t| t["id"] == id));

    // delete
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/todos/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // not found
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/todos/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// 错误路径：services 校验 + error.rs 转换最终产出 400（不会写入任何数据）。
#[tokio::test]
#[ignore = "需要先 docker compose up -d postgres"]
async fn reject_empty_title() {
    let app = test_app().await;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/todos")
                .header("content-type", "application/json")
                .body(Body::from(json!({"title":"   "}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
