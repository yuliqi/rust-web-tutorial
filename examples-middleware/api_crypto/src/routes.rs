//! HTTP 层：公钥分发 + 加密业务端点 + 演示页面。
//!
//! 注意分层：handler 只负责「拆信封 → 调业务 → 封信封」，
//! 真正的业务函数（`mask_profile`）拿到的是明文 JSON，对加密完全无感——
//! 这意味着给存量接口加报文加密时，业务代码可以一行不改。

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde_json::{json, Value};

use crate::envelope::{self, RequestEnvelope};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(index))
        .route("/crypto/public-key", get(public_key))
        .route("/api/secure/profile", post(secure_profile))
}

/// 演示页面。单文件教学示例用 include_str! 编译期内嵌即可，
/// 不必引入 ServeDir——没有静态目录遍历、缓存头这些额外概念要解释。
async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

/// 下发公钥（SPKI DER 的 base64）。公钥是公开的，此端点无需鉴权——
/// 整个方案的安全性只依赖「私钥不泄露」，这正是非对称加密的意义。
/// 格式选 SPKI 是为了对接前端：Web Crypto 的 importKey("spki", ...) 原样可用。
async fn public_key(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "spki_b64": B64.encode(state.spki_der.as_ref()) }))
}

/// 加密业务端点：密文信封进、密文信封出。
///
/// 演示场景刻意选了「提交个人资料」：手机号、身份证号正是最不想
/// 出现在网关访问日志 / 抓包审计里的字段。没有报文加密时，
/// 这些值就明晃晃躺在每一层代理的 request body 日志里。
async fn secure_profile(
    State(state): State<AppState>,
    Json(req): Json<RequestEnvelope>,
) -> Response {
    // 拆信封。注意：无论是格式错、解密失败还是时间戳超窗，
    // 对外一律 400 + 同一句话（原因见 envelope::EnvelopeError 的注释——
    // 区分错误类型会成为 padding-oracle 式攻击的信息源）。
    // 具体原因只进服务端日志，供排障用。
    let (aes_key, data) = match envelope::open_request(&state.private_key, &req) {
        Ok(ok) => ok,
        Err(e) => {
            tracing::warn!("open_request failed: {e}");
            return bad_envelope();
        }
    };

    // 业务逻辑：只碰明文，不知道加密的存在。
    let masked = mask_profile(&data);

    // 封响应：复用请求的 AES 密钥、全新 nonce。
    match envelope::seal_response(&aes_key, &masked) {
        Ok(env) => Json(env).into_response(),
        Err(e) => {
            tracing::error!("seal_response failed: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
        }
    }
}

/// 统一的信封错误响应：故意不区分失败原因。
fn bad_envelope() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": "invalid envelope" })),
    )
        .into_response()
}

/// 业务逻辑：对收到的敏感字段做脱敏回显，证明服务端确实解开并读懂了数据。
/// 入参 data 形如 {"name","phone","id_card"}。
fn mask_profile(data: &Value) -> Value {
    let name = data.get("name").and_then(Value::as_str).unwrap_or("");
    let phone = data.get("phone").and_then(Value::as_str).unwrap_or("");
    let id_card = data.get("id_card").and_then(Value::as_str).unwrap_or("");
    json!({
        "name": name,
        // 手机号：留头 3 尾 4，如 138****1234。
        "masked_phone": mask_middle(phone, 3, 4),
        // 身份证：留头 3 尾 4，如 110***********1234。
        "masked_id": mask_middle(id_card, 3, 4),
        "received_at": envelope::now_ms(),
    })
}

/// 通用打码：保留前 `head`、后 `tail` 个字符，中间替换为等量 '*'。
/// 按字符（char）而不是字节切，避免多字节字符处切断导致 panic。
fn mask_middle(s: &str, head: usize, tail: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= head + tail {
        // 太短没法打码，全部替换为 *（长度信息也尽量少泄露）。
        return "*".repeat(chars.len());
    }
    let mut out = String::new();
    out.extend(&chars[..head]);
    out.extend(std::iter::repeat_n('*', chars.len() - head - tail));
    out.extend(&chars[chars.len() - tail..]);
    out
}

#[cfg(test)]
mod tests {
    use super::mask_middle;

    #[test]
    fn mask_middle_works() {
        assert_eq!(mask_middle("13800001234", 3, 4), "138****1234");
        assert_eq!(mask_middle("110101199001011234", 3, 4), "110***********1234");
        // 太短：全打码。
        assert_eq!(mask_middle("1234567", 3, 4), "*******");
        // 多字节字符不 panic。
        assert_eq!(mask_middle("张三李四王五赵六", 3, 4), "张三李*王五赵六");
    }
}
