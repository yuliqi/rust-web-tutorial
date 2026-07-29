//! webhook 验签（第 18 章）：HMAC-SHA256 的计算与恒定时间校验。
//!
//! 为什么 webhook 必须验签：/webhooks/billing 是公网可达的**裸端点**——
//! 支付商的服务器没法先登录再回调，所以它不能挂 JWT；任何人都能对它发 POST。
//! 验签是唯一防线：支付商用双方共享的密钥对请求体算 HMAC 放进头里，
//! 我们用同一密钥重算一遍比对。改一个字节、换一个密钥，签名都对不上。
//! Stripe/GitHub/PayPal 的 webhook 全是这个套路（头名各异，原理相同）。

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// 对 body 计算 HMAC-SHA256，输出小写 hex——发送方（或测试）用它生成 X-Signature。
pub fn sign_hex(secret: &[u8], body: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC 接受任意长度的密钥");
    mac.update(body);
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// 校验签名。关键点：**恒定时间比较**。
/// 如果用 `==` 逐字节短路比较，攻击者可以测量响应耗时逐字节猜出正确签名
/// （timing attack）。这里把 hex 解码回字节后交给 `Mac::verify_slice`——
/// 它内部用 subtle crate 做恒定时间比较，比较耗时与「错在第几个字节」无关。
pub fn verify_hex(secret: &[u8], body: &[u8], signature_hex: &str) -> bool {
    let Some(signature) = hex_decode(signature_hex) else {
        // 不是合法 hex 直接拒绝（长度校验由 verify_slice 兜底）。
        return false;
    };
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC 接受任意长度的密钥");
    mac.update(body);
    mac.verify_slice(&signature).is_ok()
}

/// 极简 hex 解码（大小写都认）。不引入 hex crate：教学项目能省一个依赖是一个。
fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            s.get(i..i + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
        })
        .collect()
}

// 离线单测：HMAC 是纯计算。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify() {
        let sig = sign_hex(b"secret", b"{\"event\":\"x\"}");
        // HMAC-SHA256 输出 32 字节 = 64 个 hex 字符。
        assert_eq!(sig.len(), 64);
        assert!(verify_hex(b"secret", b"{\"event\":\"x\"}", &sig));
    }

    #[test]
    fn tampered_body_rejected() {
        let sig = sign_hex(b"secret", b"plan=free");
        // body 被篡改（free → pro）：签名失配——这正是攻击者想干而干不成的事。
        assert!(!verify_hex(b"secret", b"plan=pro", &sig));
        // 密钥不同也失配。
        assert!(!verify_hex(b"other", b"plan=free", &sig));
    }

    #[test]
    fn malformed_signature_rejected() {
        assert!(!verify_hex(b"secret", b"body", "not-hex"));
        assert!(!verify_hex(b"secret", b"body", "abc")); // 奇数长度
        assert!(!verify_hex(b"secret", b"body", "deadbeef")); // 长度不足 32 字节
    }

    #[test]
    fn hex_decode_case_insensitive() {
        assert_eq!(hex_decode("DEadBEef"), Some(vec![0xde, 0xad, 0xbe, 0xef]));
    }
}
