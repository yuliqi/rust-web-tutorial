//! 集成测试：需要真实 Postgres，默认 `#[ignore]`。
//! 跑法：先 `docker compose up -d postgres`，再 `cargo test -p iam_demo -- --ignored`。
//!
//! 每个用例用时间戳生成随机 org/email，互不干扰，也不受历史遗留数据影响。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use iam_demo::config::Config;
use iam_demo::model::GrantSpec;
use iam_demo::{app, authz::can, build_state, store, totp, AppState};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt; // oneshot

fn tag() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

async fn state() -> AppState {
    build_state(Config::from_env())
        .await
        .expect("connect db (先 docker compose up -d postgres)")
}

/// 发一个 JSON 请求，返回 (状态码, 响应体 JSON)。
async fn send(st: &AppState, method: &str, uri: &str, token: Option<&str>, body: Value) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    let req = req.body(Body::from(body.to_string())).unwrap();
    let resp = app(st.clone()).oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

#[tokio::test]
#[ignore = "需要先 docker compose up -d postgres"]
async fn hierarchy_grant_and_cross_account_isolation() {
    let st = state().await;
    let t = tag();

    // org + 主账号 + 两个子账号（分别管 A / B）。
    let (org_id, owner) = store::create_org_owner(
        &st.pool,
        &format!("org-{t}"),
        "pro",
        &format!("owner-{t}@x.test"),
        "password123",
    )
    .await
    .unwrap();

    let sub_a = store::create_sub_account(
        &st.pool, org_id, owner, &format!("a-{t}@x.test"), "password123", "member",
        &[GrantSpec::new("cloud-account", "A", "*")],
    )
    .await
    .unwrap();
    let _sub_b = store::create_sub_account(
        &st.pool, org_id, owner, &format!("b-{t}@x.test"), "password123", "member",
        &[GrantSpec::new("cloud-account", "B", "*")],
    )
    .await
    .unwrap();

    // 跨账号资源隔离：sub_a 能访问 A、不能访问 B。
    let grants_a = store::load_grants(&st.pool, sub_a).await.unwrap();
    assert!(can(&grants_a, "cloud-account", "A", "read"));
    assert!(!can(&grants_a, "cloud-account", "B", "read"));

    // 子账号越权被拒：以 sub_a 为父建孙账号索要 B（sub_a 没有）→ Forbidden。
    let over = store::create_sub_account(
        &st.pool, org_id, sub_a, &format!("gc-{t}@x.test"), "password123", "member",
        &[GrantSpec::new("cloud-account", "B", "read")],
    )
    .await;
    assert!(matches!(over, Err(store::StoreError::Forbidden(_))));

    // 在父范围内的授权则成功。
    let ok = store::create_sub_account(
        &st.pool, org_id, sub_a, &format!("gc2-{t}@x.test"), "password123", "member",
        &[GrantSpec::new("cloud-account", "A", "read")],
    )
    .await;
    assert!(ok.is_ok());
}

#[tokio::test]
#[ignore = "需要先 docker compose up -d postgres"]
async fn login_requires_second_step_when_totp_enabled() {
    let st = state().await;
    let t = tag();
    let email = format!("mfa-{t}@x.test");

    let (org_id, owner) = store::create_org_owner(
        &st.pool, &format!("org-{t}"), "free", &email, "password123",
    )
    .await
    .unwrap();

    // 没开 2FA：一步登录直接拿到正式 token。
    let (status, body) = send(&st, "POST", "/auth/login", None,
        json!({"email": email, "password": "password123"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["mfa_required"], json!(false));
    let full_token = body["token"].as_str().unwrap().to_string();

    // 正式 token 能访问受保护路由。
    let (status, _) = send(&st, "GET", "/me/permissions", Some(&full_token), Value::Null).await;
    assert_eq!(status, StatusCode::OK);

    // 开启 2FA。
    let secret = totp::generate_secret();
    store::enable_totp(&st.pool, org_id, owner, &secret).await.unwrap();

    // 现在登录只给半程 token，且它进不了受保护路由。
    let (status, body) = send(&st, "POST", "/auth/login", None,
        json!({"email": email, "password": "password123"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["mfa_required"], json!(true));
    let half = body["token"].as_str().unwrap().to_string();

    let (status, _) = send(&st, "GET", "/me/permissions", Some(&half), Value::Null).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED); // 半程 token 被 AuthUser 拒

    // 第二步：带正确 TOTP 码换正式 token。
    let code = totp::current_code(&secret, unix_now()).unwrap();
    let (status, body) = send(&st, "POST", "/auth/login/totp", None,
        json!({"token": half, "code": code})).await;
    assert_eq!(status, StatusCode::OK);
    let real = body["token"].as_str().unwrap().to_string();

    let (status, _) = send(&st, "GET", "/me/permissions", Some(&real), Value::Null).await;
    assert_eq!(status, StatusCode::OK);

    // 错误的 TOTP 码：半程换正式失败，401。
    let (status, _) = send(&st, "POST", "/auth/login/totp", None,
        json!({"token": half, "code": "000000"})).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
