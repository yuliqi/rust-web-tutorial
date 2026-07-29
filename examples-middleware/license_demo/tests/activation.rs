//! 在线激活服务的集成测试：用 axum 的内存直连（oneshot）走完整 HTTP 链路，
//! 无需监听端口、无外部依赖，默认全跑。
//!
//! 覆盖：activate -> heartbeat 成功；吊销后 heartbeat 失败；同一 key 超机器数配额被拒。

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use license_demo::activation::{router, ActivateResp, ActivationStore, HeartbeatResp};
use serde_json::{json, Value};
use tower::ServiceExt; // for `oneshot`

/// 发一个 JSON POST，返回 (状态码, 响应体 JSON)。
async fn post_json(store: &ActivationStore, path: &str, body: Value) -> (StatusCode, Value) {
    let app = router(store.clone());
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, value)
}

#[tokio::test]
async fn activate_then_heartbeat_over_http() {
    let store = ActivationStore::new();
    store.register_key("KEY-1", 2);

    // 激活。
    let (st, body) = post_json(
        &store,
        "/activate",
        json!({ "license_key": "KEY-1", "machine_fp": "m1" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let act: ActivateResp = serde_json::from_value(body).unwrap();
    assert!(!act.activation_token.is_empty());

    // 心跳续租。
    let (st, body) = post_json(
        &store,
        "/heartbeat",
        json!({ "activation_token": act.activation_token }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let hb: HeartbeatResp = serde_json::from_value(body).unwrap();
    assert!(hb.lease_expires >= act.lease_expires);
}

#[tokio::test]
async fn heartbeat_fails_after_revoke() {
    let store = ActivationStore::new();
    store.register_key("KEY-1", 2);

    let (_st, body) = post_json(
        &store,
        "/activate",
        json!({ "license_key": "KEY-1", "machine_fp": "m1" }),
    )
    .await;
    let act: ActivateResp = serde_json::from_value(body).unwrap();

    // 厂商远程吊销。
    store.revoke("KEY-1");

    let (st, body) = post_json(
        &store,
        "/heartbeat",
        json!({ "activation_token": act.activation_token }),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    assert!(body["error"].as_str().unwrap().contains("吊销"));
}

#[tokio::test]
async fn quota_exceeded_over_http() {
    let store = ActivationStore::new();
    store.register_key("KEY-1", 2);

    for m in ["m1", "m2"] {
        let (st, _) = post_json(
            &store,
            "/activate",
            json!({ "license_key": "KEY-1", "machine_fp": m }),
        )
        .await;
        assert_eq!(st, StatusCode::OK);
    }
    // 第三台新机器超配额。
    let (st, body) = post_json(
        &store,
        "/activate",
        json!({ "license_key": "KEY-1", "machine_fp": "m3" }),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    assert!(body["error"].as_str().unwrap().contains("配额"));
}

#[tokio::test]
async fn unknown_key_unauthorized() {
    let store = ActivationStore::new();
    let (st, _) = post_json(
        &store,
        "/activate",
        json!({ "license_key": "GHOST", "machine_fp": "m1" }),
    )
    .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
}
