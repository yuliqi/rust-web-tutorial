//! 字段加密的精简版：AES-256-GCM 可逆加密 + HMAC-SHA256 盲索引。
//!
//! **机制与第 20 章 `db_crypto/src/crypto.rs` 完全同源**（密文带 `v{n}:` 密钥版本
//! 前缀、盲索引另存一列、加密钥与索引钥必须分开），此处刻意重写一份精简版是为了
//! 让本示例自包含、不跨 crate `use db_crypto::crypto`。生产项目里这类基础能力应当
//! 抽成一个公共 crate（比如 `common-fieldcrypto`）被各服务复用，而不是到处复制粘贴——
//! 这里的重复只是教学取舍。密钥轮换（rotate_field/rotate_all）、盲索引为何不用
//! 「确定性加密」等更深的话题见第 20 章那份文件，本处从略。
//!
//! ## 存储格式
//!
//! 密文列统一存文本：`v{n}:{base64(nonce)}:{base64(ciphertext)}`——`v{n}` 是密钥
//! 版本前缀（解密按前缀选钥），nonce 每次加密新造（明文携带，不是秘密），密文尾部
//! 自带 GCM 的 16 字节认证标签（改一比特解密即报错）。

use std::collections::HashMap;

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Key, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

// ---------------------------------------------------------------------------
// 教学用默认密钥
// ---------------------------------------------------------------------------

// !!! 仅限教学 !!!
// 默认密钥写在源码里意味着「任何拿到代码的人都能解开用默认钥加密的数据」，
// 生产环境绝不允许：密钥应来自 KMS（信封加密解出的 DEK）或部署系统注入的 secret，
// 且绝不进代码库与镜像。这里给默认值只是为了教程「clone 下来就能跑」。
//
// 对云 AK/SK 而言这条红线尤其致命——见 credentials.rs 顶部的安全讨论。
/// v1 密钥（hex，32 字节）。模拟「上线初期用的第一代密钥」。
pub const DEV_KEY_V1_HEX: &str = "1111111111111111111111111111111111111111111111111111111111111111";
/// v2 密钥（hex，32 字节）。模拟「轮换后的现役密钥」。
pub const DEV_KEY_V2_HEX: &str = "2222222222222222222222222222222222222222222222222222222222222222";
/// 盲索引专用密钥（hex，32 字节）。与加密密钥必须分开：用途不同、轮换节奏不同、
/// 泄露的爆炸半径也不同（加密钥泄露=明文泄露，索引钥泄露=可离线撞库验证某凭证是否在库）。
pub const DEV_INDEX_KEY_HEX: &str =
    "3333333333333333333333333333333333333333333333333333333333333333";

// ---------------------------------------------------------------------------
// 错误类型
// ---------------------------------------------------------------------------

/// 字段解密错误。与第 20 章同构，供上层区分「数据损坏」与「密钥不对」。
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum FieldError {
    /// 存储格式不对：不是 `v{n}:nonce:ct` 三段、base64 解不开、nonce 长度不对。
    #[error("stored value malformed, expect `v{{n}}:base64(nonce):base64(ct)`")]
    Malformed,
    /// 密文的版本前缀在密钥环里找不到——通常是老密钥被过早下线了。
    #[error("unknown key version v{0}")]
    UnknownVersion(u8),
    /// 解密失败：密钥不对，或密文/标签被篡改（GCM 里这两者不可区分，也不该区分）。
    #[error("decrypt failed: wrong key or ciphertext tampered")]
    Decrypt,
}

// ---------------------------------------------------------------------------
// 密钥环
// ---------------------------------------------------------------------------

/// 密钥环：版本号 → 32 字节 AES 密钥，外加「当前用哪个版本加密」。
/// 轮换期间环里同时挂着新老两把钥：加密只用 active 版本，解密按密文前缀选版本。
#[derive(Debug, Clone)]
pub struct KeyRing {
    keys: HashMap<u8, [u8; 32]>,
    active_version: u8,
}

impl KeyRing {
    /// 构造密钥环。active 版本必须真的在环里，否则第一次加密就会炸——
    /// 在构造期把配置错误暴露出来，比运行期 panic 好定位。
    pub fn new(keys: HashMap<u8, [u8; 32]>, active_version: u8) -> anyhow::Result<Self> {
        anyhow::ensure!(
            keys.contains_key(&active_version),
            "active_version v{active_version} 不在密钥表中"
        );
        Ok(Self {
            keys,
            active_version,
        })
    }

    fn key(&self, version: u8) -> Option<&[u8; 32]> {
        self.keys.get(&version)
    }
}

// ---------------------------------------------------------------------------
// 加密 / 解密 / 盲索引（纯逻辑，离线可测）
// ---------------------------------------------------------------------------

/// 加密一个字段值，返回可直接入库的文本 `v{n}:{b64(nonce)}:{b64(ct)}`。
/// 每次调用都随机生成新 nonce，同一明文两次加密结果不同（语义安全）。
pub fn encrypt_field(keyring: &KeyRing, plaintext: &str) -> String {
    let key = keyring
        .key(keyring.active_version)
        .expect("KeyRing::new 已保证 active 版本存在");
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ct = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .expect("AES-GCM 加密仅在明文长度溢出时失败，字段值不可能触及");
    format!(
        "v{}:{}:{}",
        keyring.active_version,
        B64.encode(nonce),
        B64.encode(ct)
    )
}

/// 解密一个入库值。按 `v{n}` 前缀在密钥环里选钥，所以轮换期新老行同表共存也各自解得开。
pub fn decrypt_field(keyring: &KeyRing, stored: &str) -> Result<String, FieldError> {
    let (version, nonce, ct) = parse_stored(stored)?;
    let key = keyring
        .key(version)
        .ok_or(FieldError::UnknownVersion(version))?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), ct.as_slice())
        .map_err(|_| FieldError::Decrypt)?;
    String::from_utf8(plaintext).map_err(|_| FieldError::Decrypt)
}

/// 解析入库格式：`v{n}:{b64(nonce)}:{b64(ct)}` → (版本, nonce, 密文)。
fn parse_stored(stored: &str) -> Result<(u8, Vec<u8>, Vec<u8>), FieldError> {
    let mut parts = stored.splitn(3, ':');
    let version = parts
        .next()
        .and_then(|v| v.strip_prefix('v'))
        .and_then(|n| n.parse::<u8>().ok())
        .ok_or(FieldError::Malformed)?;
    let nonce = parts
        .next()
        .and_then(|s| B64.decode(s).ok())
        .ok_or(FieldError::Malformed)?;
    let ct = parts
        .next()
        .and_then(|s| B64.decode(s).ok())
        .ok_or(FieldError::Malformed)?;
    if nonce.len() != 12 {
        return Err(FieldError::Malformed);
    }
    Ok((version, nonce, ct))
}

/// 计算盲索引：`HMAC-SHA256(明文, 索引密钥)` 的 hex。用于「防重复添加同一凭证」的
/// 精确匹配查询——HMAC 不可逆，索引列泄露也还原不出 AK。
pub fn blind_index(index_key: &[u8], plaintext: &str) -> String {
    // 完全限定语法：aes-gcm 的 `KeyInit` 与 hmac 的 `Mac` 都提供 new_from_slice，
    // 两个 trait 同时在作用域时必须显式指明用哪个。
    let mut mac = <HmacSha256 as Mac>::new_from_slice(index_key).expect("HMAC 接受任意长度的密钥");
    mac.update(plaintext.as_bytes());
    hex_encode(&mac.finalize().into_bytes())
}

/// 字节 → 小写 hex 文本。
fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// 64 位 hex → 32 字节密钥。长度或字符不对就报错——密钥配置错误必须在启动期炸出来。
fn hex_decode_32(hex: &str) -> anyhow::Result<[u8; 32]> {
    anyhow::ensure!(
        hex.len() == 64,
        "密钥应为 64 个 hex 字符（32 字节），实际 {} 个",
        hex.len()
    );
    let mut out = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let pair = std::str::from_utf8(chunk).map_err(|_| anyhow::anyhow!("非法 hex"))?;
        out[i] =
            u8::from_str_radix(pair, 16).map_err(|_| anyhow::anyhow!("非法 hex 字符: {pair:?}"))?;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Keyring：加密钥环 + 索引钥的「打包」，方便在 AppState / 函数签名里整体传递
// ---------------------------------------------------------------------------

/// 把「加密密钥环」与「盲索引密钥」打包成一个可 Clone 的门面。
///
/// 为什么打包：本 crate 里凡是碰凭证的地方都同时需要这两把钥（加密 secret + 算盲索引），
/// 拆成两个参数到处传既啰嗦又容易传错顺序。打包后 AppState 只需持有一个 `Keyring`，
/// 业务代码调 `keyring.encrypt(...)` / `keyring.blind_index(...)` 即可，感知不到底层两把钥。
#[derive(Debug, Clone)]
pub struct Keyring {
    ring: KeyRing,
    index_key: [u8; 32],
}

impl Keyring {
    /// 直接构造（测试用，不读环境变量——单测不应受外部环境影响）。
    pub fn new(ring: KeyRing, index_key: [u8; 32]) -> Self {
        Self { ring, index_key }
    }

    /// 从环境变量装配：FIELD_KEY_V1 / FIELD_KEY_V2 / FIELD_INDEX_KEY（各 64 位 hex），
    /// 缺省用教学密钥。active 版本固定 v2（模拟已完成一次轮换、v1 只留着解老数据）。
    /// 生产版应是：拿 DEK 密文 → 调 KMS Decrypt → 明文 DEK 只存内存。
    pub fn from_env() -> anyhow::Result<Self> {
        let v1 = std::env::var("FIELD_KEY_V1").unwrap_or_else(|_| DEV_KEY_V1_HEX.into());
        let v2 = std::env::var("FIELD_KEY_V2").unwrap_or_else(|_| DEV_KEY_V2_HEX.into());
        let idx = std::env::var("FIELD_INDEX_KEY").unwrap_or_else(|_| DEV_INDEX_KEY_HEX.into());
        let mut keys = HashMap::new();
        keys.insert(1u8, hex_decode_32(&v1)?);
        keys.insert(2u8, hex_decode_32(&v2)?);
        Ok(Self::new(KeyRing::new(keys, 2)?, hex_decode_32(&idx)?))
    }

    /// 加密一个明文字段，返回入库文本。
    pub fn encrypt(&self, plaintext: &str) -> String {
        encrypt_field(&self.ring, plaintext)
    }

    /// 解密一个入库文本。
    pub fn decrypt(&self, stored: &str) -> Result<String, FieldError> {
        decrypt_field(&self.ring, stored)
    }

    /// 计算盲索引（用索引钥，不是加密钥）。
    pub fn blind_index(&self, plaintext: &str) -> String {
        blind_index(&self.index_key, plaintext)
    }
}

// ---------------------------------------------------------------------------
// 离线单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_keyring() -> Keyring {
        let mut keys = HashMap::new();
        keys.insert(1u8, [0x11; 32]);
        keys.insert(2u8, [0x22; 32]);
        Keyring::new(KeyRing::new(keys, 2).unwrap(), [0x33; 32])
    }

    #[test]
    fn roundtrip() {
        let kr = test_keyring();
        let stored = kr.encrypt("AKID_secret_key_pair");
        assert!(stored.starts_with("v2:"), "密文应带 active 版本前缀: {stored}");
        assert_eq!(kr.decrypt(&stored).unwrap(), "AKID_secret_key_pair");
    }

    #[test]
    fn same_plaintext_different_ciphertext() {
        // 随机 nonce ⇒ 同明文两次加密结果不同（语义安全）。
        let kr = test_keyring();
        let a = kr.encrypt("same");
        let b = kr.encrypt("same");
        assert_ne!(a, b);
        assert_eq!(kr.decrypt(&a).unwrap(), kr.decrypt(&b).unwrap());
    }

    #[test]
    fn tampered_ciphertext_rejected() {
        let kr = test_keyring();
        let stored = kr.encrypt("LTAI_secret");
        let mut chars: Vec<char> = stored.chars().collect();
        let last = chars.len() - 1;
        chars[last] = if chars[last] == 'A' { 'B' } else { 'A' };
        let tampered: String = chars.into_iter().collect();
        // 篡改后要么 base64 校验失败(Malformed)、要么标签校验失败(Decrypt)，
        // 唯一不允许的是「成功解出脏数据」。
        assert!(kr.decrypt(&tampered).is_err());
    }

    #[test]
    fn unknown_version_and_malformed() {
        let kr = test_keyring();
        let good = kr.encrypt("x");
        // 把前缀改成环里没有的 v7。
        let unknown = format!("v7{}", good.strip_prefix("v2").unwrap());
        assert_eq!(kr.decrypt(&unknown), Err(FieldError::UnknownVersion(7)));
        for bad in ["", "v2", "v2:", "v2:!!:!!", "plaintext"] {
            assert_eq!(kr.decrypt(bad), Err(FieldError::Malformed), "input: {bad:?}");
        }
    }

    #[test]
    fn blind_index_deterministic_and_key_separated() {
        let kr = test_keyring();
        let a = kr.blind_index("aliyun:LTAI123");
        // 确定性：同明文永远同索引——这是它能当 WHERE 条件的前提。
        assert_eq!(a, kr.blind_index("aliyun:LTAI123"));
        // 不同明文不同索引。
        assert_ne!(a, kr.blind_index("aliyun:LTAI124"));
        assert_eq!(a.len(), 64); // SHA-256 = 32 字节 = 64 hex
        // 换索引钥则索引值变（没有索引钥的人算不出「某凭证的索引」去撞库）。
        let other = Keyring::new(
            KeyRing::new(
                {
                    let mut m = HashMap::new();
                    m.insert(2u8, [0x22; 32]);
                    m
                },
                2,
            )
            .unwrap(),
            [0x44; 32],
        );
        assert_ne!(a, other.blind_index("aliyun:LTAI123"));
    }

    #[test]
    fn keyring_construction_validates_active() {
        let mut keys = HashMap::new();
        keys.insert(1u8, [0x11; 32]);
        assert!(KeyRing::new(keys, 9).is_err());
    }
}
