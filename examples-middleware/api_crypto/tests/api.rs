//! 集成测试：用 Rust 版「客户端」（envelope.rs 里 client_* 函数，
//! 与 static/index.html 的 Web Crypto 流程逐步对应）模拟前端，
//! 覆盖纯信封逻辑 + oneshot 真 HTTP 全链路。无任何外部服务，默认全跑。

use api_crypto::envelope::{
    client_open_response, client_seal_request, client_seal_request_at, open_request,
    seal_response, EnvelopeError,
};
use api_crypto::{app, AppState};
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use http_body_util::BodyExt;
use rsa::pkcs8::DecodePublicKey;
use serde_json::{json, Value};
use tower::ServiceExt; // for `oneshot`

/// 测试统一用内嵌教学公钥（与服务端私钥配对）。
/// 文件头的中文警告行由 envelope 里的 strip_pem_banner 处理。
const PUBLIC_PEM: &str = include_str!("../keys/dev_public.pem");

fn sample_profile() -> Value {
    json!({ "name": "张三", "phone": "13800001234", "id_card": "110101199001011234" })
}

// ---------------------------------------------------------------------------
// 纯信封逻辑（不走 HTTP）
// ---------------------------------------------------------------------------

/// 全链路 roundtrip：client_seal → open_request → seal_response → client_open。
#[test]
fn envelope_roundtrip() {
    let state = AppState::from_dev_keys();
    let data = sample_profile();

    // 客户端封请求。
    let (aes_key, req_env) = client_seal_request(PUBLIC_PEM, &data).unwrap();
    // 服务端拆信封：解出的 AES 密钥与业务数据都应与客户端一致。
    let (server_key, opened) = open_request(&state.private_key, &req_env).unwrap();
    assert_eq!(server_key, aes_key, "两端应持有同一把会话密钥");
    assert_eq!(opened, data, "拆包后的业务数据应与发送前一致");

    // 服务端封响应 → 客户端拆响应。
    let resp_payload = json!({ "ok": true, "echo": "收到" });
    let resp_env = seal_response(&server_key, &resp_payload).unwrap();
    let client_got = client_open_response(&aes_key, &resp_env).unwrap();
    assert_eq!(client_got, resp_payload);
}

/// AEAD 完整性：篡改密文任意一个字节都应导致拒绝（GCM 认证标签校验失败）。
#[test]
fn tampered_ciphertext_rejected() {
    let state = AppState::from_dev_keys();
    let (_key, mut req_env) = client_seal_request(PUBLIC_PEM, &sample_profile()).unwrap();

    // 翻转密文第一个字节的最低位后再 base64 回去。
    let mut ct = B64.decode(&req_env.ct).unwrap();
    ct[0] ^= 0x01;
    req_env.ct = B64.encode(ct);

    let err = open_request(&state.private_key, &req_env).unwrap_err();
    assert!(matches!(err, EnvelopeError::Decrypt), "篡改应表现为解密失败，实际: {err:?}");
}

/// 用错误的 AES 密钥解响应 → 拒绝（模拟拿错会话密钥/密钥已轮换）。
#[test]
fn wrong_aes_key_rejected() {
    let real_key: [u8; 32] = [7u8; 32];
    let wrong_key: [u8; 32] = [8u8; 32];
    let resp_env = seal_response(&real_key, &json!({"secret": "x"})).unwrap();
    let err = client_open_response(&wrong_key, &resp_env).unwrap_err();
    assert!(matches!(err, EnvelopeError::Decrypt));
}

/// 防重放：伪造 10 分钟前的时间戳（窗口是 5 分钟）应被拒绝。
#[test]
fn stale_timestamp_rejected() {
    let state = AppState::from_dev_keys();
    let old_ts = api_crypto::envelope::now_ms() - 600_000;
    let (_key, req_env) =
        client_seal_request_at(PUBLIC_PEM, &sample_profile(), old_ts).unwrap();
    let err = open_request(&state.private_key, &req_env).unwrap_err();
    assert!(matches!(err, EnvelopeError::Expired), "过期时间戳应被拒绝，实际: {err:?}");
}

/// 防「其实没加密」的回归：信封 JSON 序列化后不得出现明文手机号/身份证。
#[test]
fn envelope_does_not_leak_plaintext() {
    let (_key, req_env) = client_seal_request(PUBLIC_PEM, &sample_profile()).unwrap();
    let wire = serde_json::to_string(&req_env).unwrap();
    assert!(!wire.contains("13800001234"), "信封里出现了明文手机号！");
    assert!(!wire.contains("110101199001011234"), "信封里出现了明文身份证号！");
    assert!(!wire.contains("张三"), "信封里出现了明文姓名！");
}

// ---------------------------------------------------------------------------
// oneshot 真 HTTP（不占端口，Router 在内存里直接处理请求）
// ---------------------------------------------------------------------------

/// 公钥端点：返回 {"spki_b64"}，且 base64 解开后是可解析的 SPKI DER。
#[tokio::test]
async fn public_key_endpoint_serves_valid_spki() {
    let app = app(AppState::from_dev_keys());
    let resp = app
        .oneshot(Request::get("/crypto/public-key").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    let spki_b64 = json["spki_b64"].as_str().expect("应有 spki_b64 字段");
    let der = B64.decode(spki_b64).expect("spki_b64 应是合法 base64");
    // DER 真能被当作 RSA 公钥解析（前端 importKey("spki") 的 Rust 等价物）。
    rsa::RsaPublicKey::from_public_key_der(&der).expect("应是合法的 SPKI RSA 公钥");
}

/// HTTP 全流程：加密信封 POST 进去 → 解密响应 → 校验脱敏结果。
#[tokio::test]
async fn secure_profile_http_roundtrip() {
    let app = app(AppState::from_dev_keys());
    let (aes_key, req_env) = client_seal_request(PUBLIC_PEM, &sample_profile()).unwrap();

    let resp = app
        .oneshot(
            Request::post("/api/secure/profile")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&req_env).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    // 线上的响应体也只是 {nonce, ct} 密文信封——顺手断言不泄露。
    let wire = String::from_utf8_lossy(&body);
    assert!(!wire.contains("13800001234"), "响应信封里出现了明文手机号！");

    let resp_env: api_crypto::envelope::ResponseEnvelope =
        serde_json::from_slice(&body).unwrap();
    let data = client_open_response(&aes_key, &resp_env).unwrap();
    assert_eq!(data["name"], "张三");
    assert_eq!(data["masked_phone"], "138****1234");
    assert_eq!(data["masked_id"], "110***********1234");
    assert!(data["received_at"].as_i64().unwrap() > 0);
}

/// 篡改后的信封走 HTTP 应得到统一的 400（不泄露具体失败原因）。
#[tokio::test]
async fn tampered_envelope_gets_uniform_400() {
    let app = app(AppState::from_dev_keys());
    let (_key, mut req_env) = client_seal_request(PUBLIC_PEM, &sample_profile()).unwrap();
    let mut ct = B64.decode(&req_env.ct).unwrap();
    let last = ct.len() - 1;
    ct[last] ^= 0xff;
    req_env.ct = B64.encode(ct);

    let resp = app
        .oneshot(
            Request::post("/api/secure/profile")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&req_env).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    // 对外只有一句统一的话，看不出是解密失败、格式错还是超时间窗。
    assert_eq!(json["error"], "invalid envelope");
}
