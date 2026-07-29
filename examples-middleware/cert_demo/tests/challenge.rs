//! HTTP-01 挑战路由的集成测试：用 axum 的内存直连（oneshot）走完整 HTTP 链路，
//! 无需监听端口、无外部依赖，默认全跑。
//!
//! 验证「token 存进 ChallengeStore -> CA 来 GET 时路由能原样应答」这条核心链路，
//! 以及未知 token 返回 404。这正是 CA 用来确认「域名归你」的那次探测的服务端行为。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cert_demo::app;
use cert_demo::challenge::ChallengeStore;
use cert_demo::{AppState, CertInfo};
use http_body_util::BodyExt;
use std::sync::Arc;
use tower::ServiceExt; // for `oneshot`

fn test_state(challenges: ChallengeStore) -> AppState {
    AppState {
        challenges,
        cert: Arc::new(CertInfo {
            domains: vec!["localhost".into()],
            not_after: 1_000_000,
            now: 0,
            needs_renewal: false,
        }),
    }
}

async fn get(state: AppState, path: &str) -> (StatusCode, String) {
    let req = Request::builder()
        .method("GET")
        .uri(path)
        .body(Body::empty())
        .unwrap();
    let resp = app(state).oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

#[tokio::test]
async fn known_token_is_answered() {
    let store = ChallengeStore::new();
    store.insert("tok-abc", "tok-abc.account-thumbprint");

    let (status, body) = get(
        test_state(store),
        "/.well-known/acme-challenge/tok-abc",
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "tok-abc.account-thumbprint");
}

#[tokio::test]
async fn unknown_token_is_404() {
    let store = ChallengeStore::new();
    let (status, _) = get(
        test_state(store),
        "/.well-known/acme-challenge/does-not-exist",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cert_info_returns_json() {
    let store = ChallengeStore::new();
    let (status, body) = get(test_state(store), "/cert/info").await;
    assert_eq!(status, StatusCode::OK);
    // 断言几个关键字段序列化进了 JSON。
    assert!(body.contains("\"not_after\":1000000"));
    assert!(body.contains("\"needs_renewal\":false"));
    assert!(body.contains("localhost"));
}
