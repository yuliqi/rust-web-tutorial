//! 集成测试：从 HTTP 层整体验证 API 行为。
//! 策略：不真正监听端口——用 tower 的 `oneshot` 直接把 Request 喂给 Router，
//! 走完提取器、handler、错误转换的完整链路，但比起真实网络快且稳定。
//! 每个测试各建一个内存 SQLite（`sqlite::memory:`），互不共享数据，天然隔离、可并行。
//! 注意：内存库按「物理连接」隔离——所以 db::connect 对内存库把连接池收紧为
//! 1 条连接，保证建表和后续查询落在同一个库上（见 src/db.rs 的说明）。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use todo_api::{app, db, AppState};
use tower::ServiceExt;

// 复用 lib.rs 的 app()：测试与生产组装的是同一个 Router，测的就是真实配置。
async fn test_app() -> axum::Router {
    // 每个测试使用独立内存库，测试结束、连接关闭即销毁，不留任何文件
    let pool = db::connect("sqlite::memory:").await.expect("db");
    app(AppState { pool })
}

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(json!({}))
}

#[tokio::test]
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
// 步骤之间有数据依赖（后面的操作用前面拿到的 id），拆开反而要重复造数据。
// oneshot 会消费 Router，所以除最后一次外都用 app.clone()。
#[tokio::test]
async fn todo_crud_flow() {
    let app = test_app().await;

    // create
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/todos")
                .header("content-type", "application/json")
                .body(Body::from(json!({"title":"write tests"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let created = body_json(response).await;
    assert_eq!(created["title"], "write tests");
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

    // patch
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/todos/{id}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"done":true}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let updated = body_json(response).await;
    assert_eq!(updated["done"], true);

    // list filter
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

// 错误路径也要测：验证 services 的校验 + error.rs 的转换最终产出 400。
#[tokio::test]
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
