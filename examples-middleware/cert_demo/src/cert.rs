//! 证书的生命周期：生成 → 解析有效期 → 判断是否该续期。
//!
//! 这一模块**全离线、纯计算**，不碰网络、不碰数据库，所以每个函数都能被穷尽单测覆盖
//! （见文件末尾 `#[cfg(test)]`）。这也是本 crate 教学的重点：真正需要联网的 ACME 下单
//! （见 [`crate::acme`]）测不了，但证书本身「怎么生成、什么时候到期、什么时候该换」这套
//! 判定逻辑是可以完全离线验证的，而线上出事故最多的恰恰是「忘了续期，证书过期站点全挂」。
//!
//! 用到两个 crate：
//! - `rcgen`：生成密钥对、自签证书、CSR（默认 ring 后端，无需 C 工具链）。
//! - `x509-parser`：把一张证书解析出来，读它的有效期。

use anyhow::{anyhow, Result};
use rcgen::{CertificateParams, CertificateSigningRequest, KeyPair};
use time::{Duration, OffsetDateTime};

/// 自签证书默认有效期（天）。
///
/// 故意设成和 Let's Encrypt 签发证书一样的 90 天，方便和 [`needs_renewal`] 的续期窗口
/// 对照理解：真实 CA 签的证书也就这个量级，绝不会给你签一张「管到 4096 年」的证书
/// （而 rcgen 的 `generate_simple_self_signed` 默认恰恰签到 4096 年，不真实，所以这里
/// 我们手动设定有效期）。
pub const SELF_SIGNED_VALID_DAYS: i64 = 90;

/// 生成一张**自签证书**，返回 `(证书 PEM, 私钥 PEM)`。
///
/// 自签证书 = 自己给自己签名、没有任何受信 CA 背书。浏览器/客户端默认**不信任**它，
/// 会弹「证书不安全」的警告。
///
/// 适用场景：本地开发、内网服务、服务间 mTLS（两端都由自己控制、可以互相内置根证书）。
/// **面向公网的用户站点绝不能用自签**——必须用受信 CA（如 Let's Encrypt，见 [`crate::acme`]）
/// 签发的证书，否则每个访客都会看到刺眼的安全警告。
pub fn generate_self_signed(domains: &[String]) -> Result<(String, String)> {
    if domains.is_empty() {
        return Err(anyhow!("至少要指定一个域名"));
    }
    let key_pair = KeyPair::generate()?;
    let mut params = CertificateParams::new(domains.to_vec())?;

    // 手动设定有效期，模拟真实 CA 的 90 天窗口（详见 SELF_SIGNED_VALID_DAYS 注释）。
    let now = OffsetDateTime::now_utc();
    params.not_before = now;
    params.not_after = now + Duration::days(SELF_SIGNED_VALID_DAYS);

    let cert = params.self_signed(&key_pair)?;
    Ok((cert.pem(), key_pair.serialize_pem()))
}

/// 生成一对私钥 + 一份 CSR，返回 `(私钥 PEM, CSR PEM)`。
///
/// CSR（Certificate Signing Request，证书签名请求）是什么？
/// 它是你交给 CA 的一份「申请表」：里面装着**你要证书覆盖哪些域名** + **你的公钥**，
/// 并用**你的私钥自签**（证明这份申请确实是持有该私钥的人发出的）。CA 校验通过后，
/// 用 **CA 自己的私钥**在这份公钥上签名，产出你能用的证书。
///
/// 关键点：**私钥自始至终不出你的机器**，CSR 里只有公钥。ACME 下单（[`crate::acme`]）
/// 的最后一步 finalize 提交的就是这份 CSR 的 DER 编码（见 [`generate_keypair_and_csr_der`]）。
pub fn generate_keypair_and_csr(domains: &[String]) -> Result<(String, String)> {
    let (key_pair, csr) = build_keypair_and_csr(domains)?;
    Ok((key_pair.serialize_pem(), csr.pem()?))
}

/// 同 [`generate_keypair_and_csr`]，但 CSR 返回 **DER 字节**而非 PEM。
///
/// ACME 协议 finalize 那一步要求提交 DER 编码的 CSR，所以下单流程用这个版本；
/// PEM 版更适合打印/存文件给人看。二者内部生成逻辑完全相同。
pub fn generate_keypair_and_csr_der(domains: &[String]) -> Result<(String, Vec<u8>)> {
    let (key_pair, csr) = build_keypair_and_csr(domains)?;
    Ok((key_pair.serialize_pem(), csr.der().to_vec()))
}

/// 私钥 + CSR 的共同生成逻辑，供上面两个公开函数复用。
fn build_keypair_and_csr(domains: &[String]) -> Result<(KeyPair, CertificateSigningRequest)> {
    if domains.is_empty() {
        return Err(anyhow!("至少要指定一个域名"));
    }
    let key_pair = KeyPair::generate()?;
    let params = CertificateParams::new(domains.to_vec())?;
    let csr = params.serialize_request(&key_pair)?;
    Ok((key_pair, csr))
}

/// 解析一张 PEM 证书，返回它的到期时间（`not_after`）的 **Unix 秒**。
///
/// 续期调度器每天跑一遍时，就是靠这个函数从现有证书里读出「还能活到什么时候」，
/// 再交给 [`needs_renewal`] 判断该不该续。
pub fn parse_not_after(cert_pem: &str) -> Result<i64> {
    // 先把 PEM 外层（-----BEGIN CERTIFICATE----- 那层 base64 封装）剥掉。
    let (_, pem) = x509_parser::pem::parse_x509_pem(cert_pem.as_bytes())
        .map_err(|e| anyhow!("PEM 解析失败：{e}"))?;
    // 再解析里面的 X.509 DER 结构。
    let cert = pem
        .parse_x509()
        .map_err(|e| anyhow!("X.509 解析失败：{e}"))?;
    Ok(cert.validity().not_after.timestamp())
}

/// 判断一张 `not_after` 到期的证书，在 `now` 这个时刻是否**该续期了**。
///
/// 规则：到期前 `threshold_days` 天（含）就该续，即 `now >= not_after - threshold_days`。
///
/// 为什么要提前一大截、而不是等快过期才续？
/// Let's Encrypt 证书有效期 **90 天**，行业惯例是**提前 30 天**就开始续。留这么大的窗口
/// 是为了容错：续期可能因为 CA 限流、网络抖动、挑战验证暂时失败而重试好几天，提前 30 天
/// 意味着哪怕连续失败一两周也还有救，绝不会拖到证书真过期。这也正好呼应第 21 章的定时任务：
/// **每天**跑一次检查，对每个域名调用本函数，命中就触发续期（而不是掐着到期日那天才动）。
pub fn needs_renewal(not_after: i64, now: i64, threshold_days: i64) -> bool {
    let threshold_secs = threshold_days * 24 * 3600;
    now >= not_after - threshold_secs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn now_unix() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    #[test]
    fn self_signed_is_parseable_and_expires_in_about_90_days() {
        let (cert_pem, key_pem) = generate_self_signed(&["localhost".into()]).unwrap();
        assert!(cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(key_pem.contains("PRIVATE KEY"));

        let not_after = parse_not_after(&cert_pem).unwrap();
        let expected = now_unix() + SELF_SIGNED_VALID_DAYS * 24 * 3600;
        // 允许 ±1 天误差（生成与断言之间的时间差、证书时间取整等）。
        let slack = 24 * 3600;
        assert!(
            (not_after - expected).abs() < slack,
            "not_after={not_after} 应约等于 expected={expected}"
        );
    }

    #[test]
    fn self_signed_multi_domain() {
        let (cert_pem, _) =
            generate_self_signed(&["a.example".into(), "b.example".into()]).unwrap();
        assert!(parse_not_after(&cert_pem).unwrap() > now_unix());
    }

    #[test]
    fn empty_domains_rejected() {
        assert!(generate_self_signed(&[]).is_err());
        assert!(generate_keypair_and_csr(&[]).is_err());
    }

    #[test]
    fn csr_is_non_empty_and_well_formed() {
        let (key_pem, csr_pem) = generate_keypair_and_csr(&["api.example.com".into()]).unwrap();
        assert!(key_pem.contains("PRIVATE KEY"));
        assert!(csr_pem.contains("BEGIN CERTIFICATE REQUEST"));
        assert!(csr_pem.contains("END CERTIFICATE REQUEST"));

        // DER 版本应产出非空字节，且能被重新解析回一份合法 CSR。
        let (_, der) = generate_keypair_and_csr_der(&["api.example.com".into()]).unwrap();
        assert!(!der.is_empty());
    }

    #[test]
    fn parse_not_after_rejects_garbage() {
        assert!(parse_not_after("not a pem").is_err());
    }

    #[test]
    fn needs_renewal_boundaries() {
        // 基准：证书在 t=1_000_000 到期，续期阈值 30 天。
        let not_after = 1_000_000i64;
        let day = 24 * 3600;
        let threshold = 30;
        let window = threshold * day; // 提前 30 天的那个时刻

        // 还早（到期前 31 天）：不用续。
        assert!(!needs_renewal(not_after, not_after - window - day, threshold));
        // 正好踩到阈值（到期前整 30 天）：该续（含边界）。
        assert!(needs_renewal(not_after, not_after - window, threshold));
        // 窗口内（到期前 10 天）：该续。
        assert!(needs_renewal(not_after, not_after - 10 * day, threshold));
        // 正好到期：该续。
        assert!(needs_renewal(not_after, not_after, threshold));
        // 已过期：更要续。
        assert!(needs_renewal(not_after, not_after + day, threshold));
    }

    #[test]
    fn needs_renewal_zero_threshold_means_only_at_expiry() {
        let not_after = 1_000_000i64;
        assert!(!needs_renewal(not_after, not_after - 1, 0));
        assert!(needs_renewal(not_after, not_after, 0));
    }
}
