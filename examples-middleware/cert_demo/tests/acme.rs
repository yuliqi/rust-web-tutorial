//! ACME 真实下单流程的集成测试。
//!
//! 标了 `#[ignore]`：`cargo test` **默认不跑**，因为它必须联网、且需要一个你能控制的公网域名 +
//! 80 端口可达（详见 cert_demo::acme 模块头注释）。教学 / CI 环境不具备这些条件，跑必失败，
//! 所以默认跳过；真要验证时，具备条件后用 `cargo test -p cert_demo -- --ignored` 手动触发。
//!
//! 下面的用例把「起挑战服务 → 调 order_certificate → 校验拿到证书」串了一遍，演示真跑时
//! 各组件怎么协作（把 DOMAIN 换成你自己的域名，并确保这台机器的 80 端口就是该域名的 80 端口）。

use cert_demo::acme::{order_certificate, LETSENCRYPT_STAGING};
use cert_demo::cert::parse_not_after;
use cert_demo::challenge::router;
use cert_demo::challenge::ChallengeStore;

#[tokio::test]
#[ignore = "需要公网域名 + 80 端口可达 + Let's Encrypt staging，见模块注释"]
async fn order_from_letsencrypt_staging() {
    // 换成你自己的、DNS 指向本机的公网域名。
    let domain = std::env::var("ACME_TEST_DOMAIN")
        .expect("设置 ACME_TEST_DOMAIN=你的公网域名 再跑此测试");
    let domains = vec![domain];

    // 共享同一个 ChallengeStore：一份交给挑战服务应答 CA，一份交给下单流程写入 token。
    let store = ChallengeStore::new();

    // 在 80 端口起挑战服务（CA 会来 GET /.well-known/acme-challenge/{token}）。
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", 80))
        .await
        .expect("绑定 80 端口失败（需要相应权限，且端口未被占用）");
    let serve_store = store.clone();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router(serve_store)).await;
    });

    // 走完整下单流程，用 staging 环境。
    let issued = order_certificate(&domains, LETSENCRYPT_STAGING, store)
        .await
        .expect("ACME 下单失败");

    // 拿到的证书链应能解析出未来的到期时间；私钥非空。
    assert!(issued.cert_chain_pem.contains("BEGIN CERTIFICATE"));
    assert!(issued.private_key_pem.contains("PRIVATE KEY"));
    let not_after = parse_not_after(&issued.cert_chain_pem).expect("解析证书到期时间失败");
    assert!(not_after > 0);
}
