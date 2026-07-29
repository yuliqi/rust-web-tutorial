//! ACME 下单流程：用 Let's Encrypt 自动签发证书。
//!
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │  ⚠️ 这段代码「能编译、逻辑完整、可直接用于生产改造」，但**在教学环境里跑不通**。 │
//! │                                                                           │
//! │  真跑 ACME 需要三样东西，缺一不可：                                          │
//! │   1. 一个**你能控制 DNS 的公网域名**（不能是 localhost / example.com）；      │
//! │   2. 该域名的 **80 端口公网可达**（CA 要来访问 HTTP-01 挑战）；               │
//! │   3. 先用 **Let's Encrypt staging 环境**（见 [`LETSENCRYPT_STAGING`]）联调，  │
//! │      staging 签的是**不受信**的测试证书，但限流宽松、不会浪费正式配额。        │
//! │      联调通过后再切到 [`LETSENCRYPT_PRODUCTION`] 签真证书。                   │
//! │                                                                           │
//! │  所以本模块只有 `order_certificate` 一个真实完整的异步函数，配套的集成测试     │
//! │  （tests/acme.rs）标了 `#[ignore]`：`cargo test` 默认不跑它，具备上述条件时    │
//! │  用 `cargo test -- --ignored` 手动触发。离线可测的部分在 cert.rs / challenge.rs。│
//! └─────────────────────────────────────────────────────────────────────────┘
//!
//! 关于自动续期的完整形态（本 crate 不实现完整调度，只说清怎么串）：
//! 生产上把「续期」做成一个**每天跑一次的定时任务**（正是第 21 章 scheduler 的活）：
//!   1. 定时任务每天对每个域名，读现有证书的 not_after（[`crate::cert::parse_not_after`]），
//!      调 [`crate::cert::needs_renewal`] 判断是否进入了提前 30 天的续期窗口；
//!   2. 命中的域名走本模块的 [`order_certificate`] 重新签发；
//!   3. 签发成功后**热重载**证书（把新证书喂给正在监听 443 的服务，无需重启进程）。
//!
//! 提前 30 天的大窗口就是给「今天失败了明天再试」留的重试余量，详见 cert.rs 里的解释。

use anyhow::{anyhow, Context, Result};
use instant_acme::{
    Account, AuthorizationStatus, ChallengeType, Identifier, NewAccount, NewOrder, OrderStatus,
    RetryPolicy,
};

use crate::cert::generate_keypair_and_csr_der;
use crate::challenge::ChallengeStore;

/// Let's Encrypt **staging（测试）**目录 URL。联调先用它：限流宽松、签的是不受信证书。
pub const LETSENCRYPT_STAGING: &str = "https://acme-staging-v02.api.letsencrypt.org/directory";

/// Let's Encrypt **正式**目录 URL。签的是浏览器信任的真证书，有严格限流，联调通过后再用。
pub const LETSENCRYPT_PRODUCTION: &str = "https://acme-v02.api.letsencrypt.org/directory";

/// 签发成功后拿到的东西：证书链 + 对应私钥（都是 PEM）。
///
/// 注意私钥是**我们自己生成的**（在提交 CSR 时），从头到尾没离开过本机——CA 只是在我们的
/// 公钥上盖了个章。把这两样喂给监听 443 的服务器（rustls / nginx…）就能提供 HTTPS 了。
#[derive(Debug, Clone)]
pub struct IssuedCert {
    /// 证书链 PEM（叶子证书 + 中间 CA 证书）。
    pub cert_chain_pem: String,
    /// 与证书配对的私钥 PEM。
    pub private_key_pem: String,
}

/// 走完整套 ACME 流程，为 `domains` 签发一张证书。
///
/// `acme_dir_url` 传 [`LETSENCRYPT_STAGING`] 或 [`LETSENCRYPT_PRODUCTION`]。
/// `challenge_store` 必须和正在对外提供 `/.well-known/acme-challenge/{token}` 应答的
/// 那个 [`ChallengeStore`] 是**同一个**（`Clone` 即共享），否则 CA 来探测时查不到答案。
///
/// 流程对应 ACME（RFC 8555）的七步，代码里逐段标了序号：
pub async fn order_certificate(
    domains: &[String],
    acme_dir_url: &str,
    challenge_store: ChallengeStore,
) -> Result<IssuedCert> {
    if domains.is_empty() {
        return Err(anyhow!("至少要指定一个域名"));
    }

    // ── 1. 创建账户 ─────────────────────────────────────────────────
    // 首次会就地生成一把账户密钥（ACME 账户身份，和证书私钥是两码事）。
    // 生产上应把返回的 credentials 序列化存好，下次用 Account::from_credentials 复用，
    // 避免每次都新建账户（CA 对新建账户也有限流）。
    let (account, _credentials) = Account::builder()?
        .create(
            &NewAccount {
                contact: &[],
                terms_of_service_agreed: true,
                only_return_existing: false,
            },
            acme_dir_url.to_owned(),
            None,
        )
        .await
        .context("创建 ACME 账户失败（网络不通？staging URL 写错？）")?;

    // ── 2. 下 order（一份订单覆盖这些域名）───────────────────────────
    let identifiers: Vec<Identifier> =
        domains.iter().map(|d| Identifier::Dns(d.clone())).collect();
    let mut order = account
        .new_order(&NewOrder::new(&identifiers))
        .await
        .context("创建 order 失败")?;

    // ── 3. 拿 authorizations，为每个域名完成 HTTP-01 挑战 ─────────────
    // 每个域名对应一个 authorization。对每个还处于 Pending 的授权，取出 HTTP-01 挑战，
    // 把 key_authorization 放进 ChallengeStore（这样 CA 来 GET token 时我们答得出来），
    // 再 set_ready 通知 CA「我准备好了，来验吧」。
    let mut authorizations = order.authorizations();
    while let Some(result) = authorizations.next().await {
        let mut authz = result?;
        match authz.status {
            AuthorizationStatus::Pending => {}
            // 之前验过、还在有效期内的授权，CA 直接标 Valid，跳过即可。
            AuthorizationStatus::Valid => continue,
            other => return Err(anyhow!("非预期的授权状态：{other:?}")),
        }

        let mut challenge = authz
            .challenge(ChallengeType::Http01)
            .ok_or_else(|| anyhow!("该授权没有提供 HTTP-01 挑战"))?;

        // token 是 URL 里的那一段；key_authorization 是要返回的响应体（token.账户公钥指纹）。
        let token = challenge.token.clone();
        let key_authorization = challenge.key_authorization().as_str().to_string();
        challenge_store.insert(token, key_authorization);

        challenge.set_ready().await.context("通知 CA 挑战就绪失败")?;
    }

    // ── 4. 等 CA 验证（指数退避轮询，直到 Ready 或 Invalid）───────────
    let status = order
        .poll_ready(&RetryPolicy::default())
        .await
        .context("等待 order 就绪失败")?;
    if status != OrderStatus::Ready {
        return Err(anyhow!("order 未能就绪，最终状态：{status:?}"));
    }

    // ── 5. 提交 CSR（finalize）────────────────────────────────────────
    // 我们自己生成私钥 + CSR（私钥不出本机），把 CSR 的 DER 交给 CA。
    let (private_key_pem, csr_der) = generate_keypair_and_csr_der(domains)?;
    order
        .finalize_csr(&csr_der)
        .await
        .context("提交 CSR（finalize）失败")?;

    // ── 6 & 7. 轮询到证书就绪并下载证书链 ─────────────────────────────
    let cert_chain_pem = order
        .poll_certificate(&RetryPolicy::default())
        .await
        .context("下载证书失败")?;

    Ok(IssuedCert {
        cert_chain_pem,
        private_key_pem,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // 离线可测的只有「参数校验」这一点点；真正的下单在 tests/acme.rs 里标了 #[ignore]。
    #[tokio::test]
    async fn empty_domains_rejected_without_network() {
        let store = ChallengeStore::new();
        let err = order_certificate(&[], LETSENCRYPT_STAGING, store)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("至少要指定一个域名"));
    }

    #[test]
    fn staging_and_prod_urls_are_distinct() {
        assert_ne!(LETSENCRYPT_STAGING, LETSENCRYPT_PRODUCTION);
        assert!(LETSENCRYPT_STAGING.contains("staging"));
    }
}
