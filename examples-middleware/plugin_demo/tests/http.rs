//! 集成测试：用 axum 的内存直连（oneshot）打全套 HTTP 端点，无需监听端口。默认运行。
//! 插件调用这条链路会真 spawn 样例插件子进程（路径同样来自 `env!("CARGO_BIN_EXE_sample_plugin")`）。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use plugin_demo::plugin::PluginManifest;
use plugin_demo::{app, AppState};
use serde_json::{json, Value};
use tower::ServiceExt; // oneshot

async fn send(state: &AppState, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app(state.clone()).oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

fn plugin_manifest() -> PluginManifest {
    PluginManifest {
        name: "echo-tools".into(),
        version: "0.1.0".into(),
        entry: env!("CARGO_BIN_EXE_sample_plugin").to_string(),
        capabilities: vec!["read_text".into(), "transform_text".into()],
    }
}

#[tokio::test]
async fn list_apps_returns_catalog() {
    let state = AppState::default();
    let (status, body) = send(&state, "GET", "/apps", Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.as_array().unwrap().iter().any(|m| m["id"] == "postgres"));
}

#[tokio::test]
async fn install_renders_compose_and_records() {
    let state = AppState::default();
    let (status, body) = send(
        &state,
        "POST",
        "/apps/postgres/install",
        json!({"instance_name": "pg1", "values": {"password": "S3cret", "port": "6543"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let compose = body["compose"].as_str().unwrap();
    assert!(compose.contains("POSTGRES_PASSWORD: \"S3cret\""));
    assert!(compose.contains("- \"6543:5432\""));

    // 安装记录已落库（内存）。
    let (status, listed) = send(&state, "GET", "/installed", Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["instance_name"], "pg1");
}

#[tokio::test]
async fn install_missing_required_param_is_400() {
    let state = AppState::default();
    // postgres 的 password 必填，不给 → 400。
    let (status, body) = send(
        &state,
        "POST",
        "/apps/postgres/install",
        json!({"instance_name": "pg1", "values": {"port": "5432"}}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("password"));
}

#[tokio::test]
async fn install_unknown_app_is_404() {
    let state = AppState::default();
    let (status, _) = send(
        &state,
        "POST",
        "/apps/nope/install",
        json!({"instance_name": "x", "values": {}}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn call_plugin_over_http_roundtrips() {
    let state = AppState::default();
    state
        .plugins
        .install(plugin_manifest(), &["read_text", "transform_text"])
        .await
        .expect("install plugin");

    let (status, body) = send(
        &state,
        "POST",
        "/plugins/echo-tools/call",
        json!({"method": "uppercase", "params": "abc"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!("ABC"));

    // 插件清单也列得出。
    let (status, plugins) = send(&state, "GET", "/plugins", Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(plugins[0]["name"], "echo-tools");
}

#[tokio::test]
async fn call_unknown_plugin_is_404() {
    let state = AppState::default();
    let (status, _) = send(
        &state,
        "POST",
        "/plugins/ghost/call",
        json!({"method": "ping", "params": null}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
