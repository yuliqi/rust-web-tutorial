//! 混合加密信封：本示例的核心，纯逻辑、不碰网络，所以可以直接单测。
//!
//! ## 为什么有了 TLS 还要做应用层加密？
//!
//! TLS 只保护「传输段」。企业环境里 TLS 常在网关 / 反向代理 / WAF 上终止
//! （所谓 TLS termination），之后到业务服务之间跑的是 HTTP 明文——
//! 这些明文会进网关访问日志、APM 采样、抓包审计系统。手机号、身份证号
//! 这类字段就这样躺进了一堆你控制不了的日志里。
//!
//! 应用层报文加密的目标：**即使中间节点看到完整的 HTTP 报文，也只看到密文信封**。
//!
//! ## 诚实的边界（注释比代码重要的地方）
//!
//! - 这**不替代 TLS**，而是叠加在 TLS 之上：TLS 防的是链路窃听/中间人，
//!   本方案防的是「TLS 终止之后」的日志与代理侧被动泄露。
//! - 前端 JS 可以被逆向，密钥生成逻辑对端上攻击者完全透明。
//!   防的是「日志里不出现明文」，不是「防住控制了浏览器的人」。
//!
//! ## 方案：混合加密（TLS 自己内部也是同样的思路）
//!
//! 1. 前端每次会话随机生成 AES-256-GCM 密钥（对称加密快，适合加密正文）；
//! 2. 用服务端 RSA 公钥（RSA-OAEP-SHA256）加密这把 AES 密钥，随请求携带
//!    （非对称加密慢且有长度上限，只用来递送对称密钥）；
//! 3. 请求体、响应体全程 AES-GCM 密文；
//! 4. 内层明文带时间戳，超出时间窗的请求直接拒绝（防重放）。
//!
//! ## 信封格式（JSON，二进制字段一律 base64）
//!
//! 请求：`{"ek": RSA-OAEP(aes_key), "nonce": 12字节, "ct": AES-GCM(inner_json)}`
//! 响应：`{"nonce": 新的12字节, "ct": ...}` —— 复用请求带来的 AES 密钥。
//! 内层明文：`{"ts": unix_ms, "data": <业务JSON>}`。

use aes_gcm::aead::{Aead, KeyInit, OsRng as AeadOsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Key, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rsa::pkcs8::DecodePublicKey;
use rsa::{Oaep, RsaPrivateKey, RsaPublicKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;

/// 会话对称密钥：AES-256 就是 32 字节。用定长数组而不是 Vec，
/// 长度错误在类型层面就不可能发生。
pub type AesKey = [u8; 32];

/// 重放时间窗：|now - ts| ≤ 5 分钟。
/// 太小会误伤时钟略有偏差的客户端，太大则重放窗口过宽。
/// 更严格的方案是「一次性 nonce 去重」：把见过的 nonce 写入 Redis `SET NX`，
/// 存活期设为时间窗长度，重复出现即拒绝——与第 18 章幂等键是同一机制。
pub const REPLAY_WINDOW_MS: i64 = 300_000;

/// 请求信封：三个字段都是 base64 文本，整个信封就是网关日志里能看到的全部内容。
#[derive(Debug, Serialize, Deserialize)]
pub struct RequestEnvelope {
    /// encrypted key：RSA-OAEP-SHA256 加密后的 AES 会话密钥（256 字节 → base64）。
    pub ek: String,
    /// AES-GCM 的 12 字节 nonce（IV）。可以明文携带——nonce 不是秘密，只要求不重复。
    pub nonce: String,
    /// AES-GCM 密文（Web Crypto 与 aes-gcm crate 一致：密文尾部自带 16 字节认证标签）。
    pub ct: String,
}

/// 响应信封：不再有 ek——响应复用请求带来的 AES 密钥。
/// 但 nonce 必须是**新生成**的：GCM 下「同一把密钥 + 同一个 nonce」用两次
/// 是灾难级错误（两段密文异或即泄露明文异或，认证密钥也随之可解）。
/// 所以规则很死板：每次加密都造新 nonce，绝不复用。
#[derive(Debug, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    pub nonce: String,
    pub ct: String,
}

/// 信封处理错误。内部区分清楚（日志、测试要用），
/// 但对外 HTTP 层会把三种情况**统一映射成 400 + 同一句话**。
///
/// 为什么不告诉客户端具体哪步失败？历史教训：Padding Oracle 攻击
/// （如 2002 年针对 CBC 填充的 Vaudenay 攻击、后来一系列 TLS 变体）
/// 正是靠服务端「填充错」和「MAC 错」返回不同错误/耗时，逐字节推出明文。
/// 区分错误就是给攻击者送信息，统一对外口径是密码工程的常识。
#[derive(Debug, thiserror::Error)]
pub enum EnvelopeError {
    /// base64 解不开、JSON 不成形、nonce 长度不对……结构层面的错。
    #[error("envelope malformed")]
    Malformed,
    /// RSA 或 AES-GCM 解密失败（含认证标签校验失败，即密文被篡改）。
    #[error("decrypt failed")]
    Decrypt,
    /// 时间戳超出重放窗口。
    #[error("timestamp outside replay window")]
    Expired,
}

/// 当前 Unix 毫秒时间戳。
pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before 1970")
        .as_millis() as i64
}

/// PEM 文件里允许我们放中文警告头，但 RFC 7468 严格解析器不认前导文本，
/// 所以先把 `-----BEGIN` 之前的内容剪掉再交给解析器。
fn strip_pem_banner(pem: &str) -> &str {
    match pem.find("-----BEGIN") {
        Some(idx) => &pem[idx..],
        None => pem,
    }
}

/// 从 PEM 文本加载 RSA 私钥（PKCS#8）。启动时调用一次，失败直接 panic 快速暴露配置错误。
pub fn load_private_key(pem: &str) -> anyhow::Result<RsaPrivateKey> {
    use rsa::pkcs8::DecodePrivateKey;
    Ok(RsaPrivateKey::from_pkcs8_pem(strip_pem_banner(pem))?)
}

/// 公钥的 SPKI DER 字节。前端 Web Crypto 的 `importKey("spki", ...)`
/// 吃的正是这个格式（SubjectPublicKeyInfo），所以接口直接下发 DER 的 base64。
pub fn public_key_spki_der(key: &RsaPublicKey) -> anyhow::Result<Vec<u8>> {
    use rsa::pkcs8::EncodePublicKey;
    Ok(key.to_public_key_der()?.as_bytes().to_vec())
}

// ---------------------------------------------------------------------------
// 服务端侧
// ---------------------------------------------------------------------------

/// 服务端：拆开请求信封。
/// 步骤：RSA 私钥解出 AES 密钥 → AES-GCM 解出内层明文 → 校验时间戳。
/// 返回 AES 密钥（加密响应要用）和业务数据 `data`。
pub fn open_request(
    private_key: &RsaPrivateKey,
    envelope: &RequestEnvelope,
) -> Result<(AesKey, Value), EnvelopeError> {
    // 1. base64 还原三个二进制字段。解不开就是格式错。
    let ek = B64.decode(&envelope.ek).map_err(|_| EnvelopeError::Malformed)?;
    let nonce = B64.decode(&envelope.nonce).map_err(|_| EnvelopeError::Malformed)?;
    let ct = B64.decode(&envelope.ct).map_err(|_| EnvelopeError::Malformed)?;
    if nonce.len() != 12 {
        return Err(EnvelopeError::Malformed);
    }

    // 2. RSA-OAEP 解出会话 AES 密钥。OAEP 的哈希参数必须与加密端一致
    //    （前端 Web Crypto 里写的是 hash: "SHA-256"，两边对不上就永远解不开）。
    let key_bytes = private_key
        .decrypt(Oaep::new::<Sha256>(), &ek)
        .map_err(|_| EnvelopeError::Decrypt)?;
    let aes_key: AesKey = key_bytes.try_into().map_err(|_| EnvelopeError::Decrypt)?;

    // 3. AES-GCM 解密内层。GCM 是 AEAD：解密与完整性校验一步完成，
    //    密文被改动任何一个比特，这里都会直接报错——不存在「解出脏数据」。
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&aes_key));
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), ct.as_slice())
        .map_err(|_| EnvelopeError::Decrypt)?;

    // 4. 内层必须是 {"ts": unix_ms, "data": ...}。
    let inner: Value = serde_json::from_slice(&plaintext).map_err(|_| EnvelopeError::Malformed)?;
    let ts = inner
        .get("ts")
        .and_then(Value::as_i64)
        .ok_or(EnvelopeError::Malformed)?;
    let data = inner.get("data").cloned().ok_or(EnvelopeError::Malformed)?;

    // 5. 防重放：时间戳离当前超过窗口就拒绝。注意用绝对值——
    //    客户端时钟快于服务端时 ts 会「来自未来」，同样在容忍范围内。
    if (now_ms() - ts).abs() > REPLAY_WINDOW_MS {
        return Err(EnvelopeError::Expired);
    }

    Ok((aes_key, data))
}

/// 服务端：把业务数据封进响应信封。复用请求的 AES 密钥，但 nonce 重新随机生成
/// （见 [`ResponseEnvelope`] 上关于 GCM nonce 绝不能重用的注释）。
pub fn seal_response(aes_key: &AesKey, data: &Value) -> Result<ResponseEnvelope, EnvelopeError> {
    let inner = json!({ "ts": now_ms(), "data": data });
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(aes_key));
    let nonce = Aes256Gcm::generate_nonce(&mut AeadOsRng);
    let ct = cipher
        .encrypt(&nonce, inner.to_string().as_bytes())
        .map_err(|_| EnvelopeError::Decrypt)?;
    Ok(ResponseEnvelope {
        nonce: B64.encode(nonce),
        ct: B64.encode(ct),
    })
}

// ---------------------------------------------------------------------------
// 「客户端侧」：真实客户端是 static/index.html 里的 Web Crypto JS，
// 下面的 Rust 版是它的逐行对照翻译，供集成测试在没有浏览器的环境里
// 模拟完整前后端链路。两边步骤一一对应，改任何一边都要同步另一边。
// ---------------------------------------------------------------------------

/// 客户端：生成会话 AES 密钥并封装请求。
/// 返回 AES 密钥（客户端要留着解响应）和可直接 POST 的信封。
pub fn client_seal_request(
    public_key_pem: &str,
    data: &Value,
) -> anyhow::Result<(AesKey, RequestEnvelope)> {
    client_seal_request_at(public_key_pem, data, now_ms())
}

/// 同上，但时间戳由调用者指定。**仅供测试**伪造过期时间戳验证重放窗口；
/// 正常代码永远走 [`client_seal_request`]。
pub fn client_seal_request_at(
    public_key_pem: &str,
    data: &Value,
    ts: i64,
) -> anyhow::Result<(AesKey, RequestEnvelope)> {
    let public_key = RsaPublicKey::from_public_key_pem(strip_pem_banner(public_key_pem))?;

    // 1. 随机生成本次会话的 AES-256 密钥（对应前端 crypto.getRandomValues(32字节)）。
    let key = Aes256Gcm::generate_key(&mut AeadOsRng);
    let aes_key: AesKey = key.into();

    // 2. 组内层明文 {ts, data}，AES-GCM 加密（对应前端 subtle.encrypt AES-GCM）。
    let inner = json!({ "ts": ts, "data": data });
    let cipher = Aes256Gcm::new(&key);
    let nonce = Aes256Gcm::generate_nonce(&mut AeadOsRng);
    let ct = cipher
        .encrypt(&nonce, inner.to_string().as_bytes())
        .map_err(|e| anyhow::anyhow!("aes-gcm encrypt: {e}"))?;

    // 3. 用服务端公钥 RSA-OAEP 加密 AES 密钥（对应前端 subtle.encrypt RSA-OAEP）。
    //    OAEP 填充需要随机数，所以 encrypt 要传 rng——这也是同一明文
    //    每次加密结果都不同的原因（概率加密，防止密文比对攻击）。
    let ek = public_key
        .encrypt(&mut rand::rngs::OsRng, Oaep::new::<Sha256>(), &aes_key)
        .map_err(|e| anyhow::anyhow!("rsa-oaep encrypt: {e}"))?;

    Ok((
        aes_key,
        RequestEnvelope {
            ek: B64.encode(ek),
            nonce: B64.encode(nonce),
            ct: B64.encode(ct),
        },
    ))
}

/// 客户端：用留存的会话 AES 密钥拆响应信封，返回业务数据 `data`。
pub fn client_open_response(
    aes_key: &AesKey,
    envelope: &ResponseEnvelope,
) -> Result<Value, EnvelopeError> {
    let nonce = B64.decode(&envelope.nonce).map_err(|_| EnvelopeError::Malformed)?;
    let ct = B64.decode(&envelope.ct).map_err(|_| EnvelopeError::Malformed)?;
    if nonce.len() != 12 {
        return Err(EnvelopeError::Malformed);
    }
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(aes_key));
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), ct.as_slice())
        .map_err(|_| EnvelopeError::Decrypt)?;
    let inner: Value = serde_json::from_slice(&plaintext).map_err(|_| EnvelopeError::Malformed)?;
    inner.get("data").cloned().ok_or(EnvelopeError::Malformed)
}
