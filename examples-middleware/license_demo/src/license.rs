//! 离线签名授权（offline license）。
//!
//! 核心思想：厂商用**私钥**对一份 license（授权给谁、什么产品/版本、开哪些功能、
//! 绑哪台机器、什么时候到期）签名，客户端只内置**公钥**做验签。
//! 因为验签不需要联网，所以**断网也能校验**——这就是私有化/内网部署软件常用的授权方式。
//!
//! 为什么用非对称签名而不是「一段密码 / HMAC」？
//! - HMAC 需要客户端也持有同一把密钥，密钥一旦从客户端二进制里被扒出来，
//!   盗版者就能自己签发任意 license。
//! - Ed25519 是非对称的：私钥只在厂商签发机上，客户端只有公钥；
//!   公钥泄露也无所谓（本来就是公开的），没有私钥就伪造不出有效签名。

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// 教学用固定密钥对
// ---------------------------------------------------------------------------
//
// 真实工程里：
// - 私钥 SIGNING_KEY_SEED 只存在于厂商的「签发机」（离线金库 / HSM），
//   **绝不**编进任何发给客户的二进制；
// - 客户端二进制里只烧录 VERIFYING_KEY_BYTES 这一个公钥。
// 这里为了教学把两者放在同一个文件，读者要理解它们在生产中是物理隔离的。

/// 厂商私钥种子（32 字节）。Ed25519 的私钥就是这 32 字节种子，签名逻辑内部再派生。
///
/// ⚠️ 注意：这串种子正是 RFC 8032 的公开测试向量（Test 1），**任何人都能查到**。
/// 仅用于教学演示，绝不可复用到真实产品——生产要用 CSPRNG 现场生成并妥善保管。
pub const SIGNING_KEY_SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];

/// 客户端内置的厂商公钥（32 字节），与上面的私钥配对。由 SIGNING_KEY_SEED 派生而来，
/// 见 tests 里的 `keypair_matches` 断言：改了私钥忘了同步公钥，测试会立刻报错。
pub const VERIFYING_KEY_BYTES: [u8; 32] = [
    0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07, 0x3a,
    0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07, 0x51, 0x1a,
];

/// 厂商签发机上构造私钥。仅在厂商侧调用。
pub fn vendor_signing_key() -> SigningKey {
    SigningKey::from_bytes(&SIGNING_KEY_SEED)
}

/// 客户端侧构造公钥。生产中这一个常量就是烧进客户端二进制的全部密钥材料。
pub fn client_verifying_key() -> VerifyingKey {
    // 常量是我们自己派生的合法压缩点，unwrap 不会 panic。
    VerifyingKey::from_bytes(&VERIFYING_KEY_BYTES).expect("内置公钥字节必须是合法的 Ed25519 公钥")
}

// ---------------------------------------------------------------------------
// License 数据结构
// ---------------------------------------------------------------------------

/// 一份授权的全部内容。所有字段都参与签名，改任何一个字段签名都会失效。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct License {
    /// 授权给谁（公司 / 客户名）。
    pub licensee: String,
    /// 产品标识。
    pub product: String,
    /// 版本档位，如 "pro" / "enterprise"。仅作展示，真正的功能开关看 features。
    pub edition: String,
    /// 功能位：这份 license 解锁了哪些功能点（见 has_feature）。
    pub features: Vec<String>,
    /// 绑定的机器指纹：这份 license 只能在指纹匹配的机器上用（防止一份授权到处拷贝）。
    pub machine_fingerprint: String,
    /// 签发时间（Unix 秒）。
    pub issued_at: i64,
    /// 过期时间（Unix 秒）。now > expires_at 即失效。
    pub expires_at: i64,
}

impl License {
    /// 规范 JSON：序列化时字段顺序固定，签发端与验签端必须字节一致，
    /// 否则同样的内容算出的签名对不上。serde 按结构体字段声明顺序输出，
    /// 只要两端用同一个结构体、同一个 serde_json，就天然规范化。
    ///
    /// 生产中若担心跨语言/跨版本差异，会改用显式排序的 canonical JSON 或
    /// 直接对固定字段拼接后再签，这里教学用 serde_json 已足够稳定。
    pub fn canonical_json(&self) -> Result<Vec<u8>, LicenseError> {
        serde_json::to_vec(self).map_err(|_| LicenseError::Malformed)
    }

    /// 功能位判定：同一个二进制，按 license 的 features 决定放不放某个功能。
    /// 这就是「企业版 / 专业版」在技术上的实现——不是发不同的安装包，
    /// 而是同一份程序读授权里的功能位来解锁不同能力。
    pub fn has_feature(&self, name: &str) -> bool {
        self.features.iter().any(|f| f == name)
    }
}

/// 签好名的 license：license 本体 + 对其规范 JSON 的 Ed25519 签名（base64）。
/// 这就是最终发给客户的「license 文件」的内容。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedLicense {
    pub license: License,
    pub signature_b64: String,
}

// ---------------------------------------------------------------------------
// 错误分类
// ---------------------------------------------------------------------------

/// 验证失败的原因。分开是为了让客户端能给出不同提示：
/// 过期可引导续费，机器不符可引导重新绑定，签名坏 / 格式坏则是被篡改或伪造。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LicenseError {
    /// 签名无效：内容被篡改，或根本不是厂商私钥签的（伪造）。
    #[error("license 签名无效（内容被篡改或伪造）")]
    BadSignature,
    /// 已过期。
    #[error("license 已过期")]
    Expired,
    /// 机器指纹不匹配：这份 license 不是发给当前机器的。
    #[error("机器指纹不匹配（license 绑定的是另一台机器）")]
    MachineMismatch,
    /// 格式错误：JSON、base64 或签名长度不对。
    #[error("license 格式错误")]
    Malformed,
}

// ---------------------------------------------------------------------------
// 厂商侧：签发
// ---------------------------------------------------------------------------

/// 厂商用私钥签发一份 license。只在厂商签发机上运行。
pub fn issue(signing_key: &SigningKey, license: License) -> Result<SignedLicense, LicenseError> {
    let msg = license.canonical_json()?;
    let sig: Signature = signing_key.sign(&msg);
    Ok(SignedLicense {
        license,
        signature_b64: B64.encode(sig.to_bytes()),
    })
}

// ---------------------------------------------------------------------------
// 客户端侧：验证
// ---------------------------------------------------------------------------

/// 客户端离线验证 license，依次三步。**顺序很重要**：
///
/// 1. **先验签名**：一切以「内容没被改过」为前提。如果先看 expires_at 再验签名，
///    盗版者随便把过期时间改成 2999 年，你还没验签就已经信了这个假日期。
///    所以必须先用公钥确认 license.canonical_json() 的签名成立，
///    才能相信里面每个字段都是厂商签发时的原值。
/// 2. **再看是否过期**：now 必须 ≤ expires_at。now 由调用方传入（便于测试与统一时钟）。
/// 3. **最后核机器指纹**：license 绑定的指纹必须等于当前机器指纹，
///    防止把一份合法 license 拷到别的机器上用。
///
/// 三步全过才返回 Ok。任一失败返回对应的 LicenseError。
pub fn verify(
    verifying_key: &VerifyingKey,
    signed: &SignedLicense,
    now: i64,
    machine_fp: &str,
) -> Result<(), LicenseError> {
    // 第 1 步：签名。
    let sig_bytes = B64
        .decode(&signed.signature_b64)
        .map_err(|_| LicenseError::Malformed)?;
    let sig_arr: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| LicenseError::Malformed)?;
    let sig = Signature::from_bytes(&sig_arr);
    let msg = signed.license.canonical_json()?;
    verifying_key
        .verify(&msg, &sig)
        .map_err(|_| LicenseError::BadSignature)?;

    // 第 2 步：有效期（签名已确认 expires_at 是原值，现在才能信它）。
    if now > signed.license.expires_at {
        return Err(LicenseError::Expired);
    }

    // 第 3 步：机器指纹。
    if signed.license.machine_fingerprint != machine_fp {
        return Err(LicenseError::MachineMismatch);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// 机器指纹
// ---------------------------------------------------------------------------

/// 由若干「能稳定拿到」的机器特征拼起来做 SHA-256，得到一串定长指纹。
///
/// 这里把原料作为参数注入（而不是在函数内部直接读主机名 / 网卡），有两个好处：
/// - **可测**：测试能传入固定原料，得到确定的指纹值；
/// - **可控**：真实实现里到底采集哪些特征（主机名、CPU id、MAC、主板序列号……）
///   是策略问题，交给调用方决定。
///
/// 真实工程的两个坑（教学提示，不在本函数处理）：
/// - 特征太少易碰撞、太严则换根内存条就失效——通常综合多项并允许「小变动容错」
///   （比如 5 项对上 4 项就算同一台机器）；
/// - 虚拟机 / 容器里很多特征不稳定，需要额外策略。
pub fn fingerprint_from(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for p in parts {
        hasher.update(p.as_bytes());
        // 用 0 分隔，避免 ["ab","c"] 与 ["a","bc"] 拼出同一串。
        hasher.update([0u8]);
    }
    let digest = hasher.finalize();
    B64.encode(digest)
}

/// 便捷封装：用主机名等常见特征算指纹。真实实现会再加 CPU id / MAC 等。
pub fn machine_fingerprint(hostname: &str, cpu_id: &str) -> String {
    fingerprint_from(&[hostname, cpu_id])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_license(expires_at: i64, fp: &str) -> License {
        License {
            licensee: "示例科技有限公司".to_string(),
            product: "AcmeDB".to_string(),
            edition: "pro".to_string(),
            features: vec!["cluster".to_string(), "audit_log".to_string()],
            machine_fingerprint: fp.to_string(),
            issued_at: 1_700_000_000,
            expires_at,
        }
    }

    #[test]
    fn keypair_matches() {
        // 私钥派生的公钥必须等于内置公钥常量，防止改了 seed 忘了同步公钥。
        let derived = vendor_signing_key().verifying_key();
        assert_eq!(derived.to_bytes(), VERIFYING_KEY_BYTES);
    }

    #[test]
    fn issue_then_verify_ok() {
        let sk = vendor_signing_key();
        let vk = client_verifying_key();
        let fp = "machine-abc";
        let signed = issue(&sk, sample_license(2_000_000_000, fp)).unwrap();
        assert_eq!(verify(&vk, &signed, 1_800_000_000, fp), Ok(()));
    }

    #[test]
    fn tampered_content_bad_signature() {
        let sk = vendor_signing_key();
        let vk = client_verifying_key();
        let fp = "machine-abc";
        let mut signed = issue(&sk, sample_license(2_000_000_000, fp)).unwrap();
        // 篡改：偷偷把版本升到 enterprise，签名立刻对不上。
        signed.license.edition = "enterprise".to_string();
        assert_eq!(
            verify(&vk, &signed, 1_800_000_000, fp),
            Err(LicenseError::BadSignature)
        );
    }

    #[test]
    fn tampered_expiry_bad_signature() {
        let sk = vendor_signing_key();
        let vk = client_verifying_key();
        let fp = "machine-abc";
        let mut signed = issue(&sk, sample_license(1_000, fp)).unwrap();
        // 盗版者想把过期时间改到很久以后——但这也在签名覆盖范围内，先验签就拦住了。
        signed.license.expires_at = 9_999_999_999;
        assert_eq!(
            verify(&vk, &signed, 1_800_000_000, fp),
            Err(LicenseError::BadSignature)
        );
    }

    #[test]
    fn expired_license() {
        let sk = vendor_signing_key();
        let vk = client_verifying_key();
        let fp = "machine-abc";
        // 合法签发，但 now 已超过 expires_at。
        let signed = issue(&sk, sample_license(1_000, fp)).unwrap();
        assert_eq!(
            verify(&vk, &signed, 2_000, fp),
            Err(LicenseError::Expired)
        );
    }

    #[test]
    fn machine_mismatch() {
        let sk = vendor_signing_key();
        let vk = client_verifying_key();
        let signed = issue(&sk, sample_license(2_000_000_000, "machine-abc")).unwrap();
        // license 绑的是 machine-abc，却拿到 machine-xyz 上验。
        assert_eq!(
            verify(&vk, &signed, 1_800_000_000, "machine-xyz"),
            Err(LicenseError::MachineMismatch)
        );
    }

    #[test]
    fn has_feature_hit_and_miss() {
        let lic = sample_license(2_000_000_000, "machine-abc");
        assert!(lic.has_feature("cluster"));
        assert!(lic.has_feature("audit_log"));
        assert!(!lic.has_feature("geo_replication"));
    }

    #[test]
    fn fingerprint_deterministic() {
        // 同样的原料每次都得到同一串指纹。
        let a = fingerprint_from(&["host-1", "cpu-9"]);
        let b = fingerprint_from(&["host-1", "cpu-9"]);
        assert_eq!(a, b);
        // 原料不同则指纹不同。
        let c = fingerprint_from(&["host-1", "cpu-8"]);
        assert_ne!(a, c);
        // 分隔符防拼接歧义：["ab","c"] 与 ["a","bc"] 不应撞。
        assert_ne!(
            fingerprint_from(&["ab", "c"]),
            fingerprint_from(&["a", "bc"])
        );
    }
}
