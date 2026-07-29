//! 第 27 章证书管理演示（`cargo run -p cert_demo`），全程**离线**运行：
//!
//!   1. 生成一张自签证书（模拟 90 天有效期）；
//!   2. 从证书里解析出到期时间；
//!   3. 用 needs_renewal 演示「还早、不用续」和「快到期、该续」两种判定；
//!   4. 打印一段「怎么用 ACME 真签发」的操作指引；
//!   5. 起一个 HTTP 服务（默认 PORT=3007），暴露 `/cert/info` 和 HTTP-01 挑战路由，
//!      方便用 curl 摸一摸接口长什么样。
//!
//! 真正联网签发证书的流程见 cert_demo::acme（能编译，真跑要公网域名，说明见 README）。

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use cert_demo::acme::{LETSENCRYPT_PRODUCTION, LETSENCRYPT_STAGING};
use cert_demo::cert::{generate_self_signed, needs_renewal, parse_not_after, SELF_SIGNED_VALID_DAYS};
use cert_demo::challenge::{ChallengeStore, CHALLENGE_PREFIX};
use cert_demo::{app, AppState, CertInfo};

/// 续期阈值：到期前 30 天开始续（和 cert.rs 的解释一致）。
const RENEW_THRESHOLD_DAYS: i64 = 30;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,cert_demo=debug".into()),
        )
        .init();

    let domains = vec!["localhost".to_string(), "dev.local".to_string()];

    // ── 1. 生成自签证书 ────────────────────────────────────────────────
    let (cert_pem, _key_pem) = generate_self_signed(&domains).context("生成自签证书失败")?;
    println!("==== 一、证书生命周期（离线）====\n");
    println!(
        "已生成自签证书，覆盖域名 {:?}，有效期设定 {} 天。",
        domains, SELF_SIGNED_VALID_DAYS
    );
    println!("（自签仅供本地/内网；面向公网用户必须用 Let's Encrypt 等受信 CA 签发）\n");

    // ── 2. 解析到期时间 ────────────────────────────────────────────────
    let not_after = parse_not_after(&cert_pem).context("解析证书到期时间失败")?;
    println!("解析出证书到期时间 not_after = {not_after}（Unix 秒）");

    // ── 3. needs_renewal 两种判定 ──────────────────────────────────────
    let day = 24 * 3600;
    // 场景 A：证书刚签发（此刻距到期 90 天），还早，不用续。
    let now_fresh = not_after - SELF_SIGNED_VALID_DAYS * day;
    // 场景 B：证书快到期了（此刻距到期 10 天），进入 30 天窗口，该续。
    let now_soon = not_after - 10 * day;

    println!("\n续期判定（阈值 = 提前 {RENEW_THRESHOLD_DAYS} 天）：");
    println!(
        "  刚签发（距到期 {} 天）-> needs_renewal = {}（还早，不用动）",
        SELF_SIGNED_VALID_DAYS,
        needs_renewal(not_after, now_fresh, RENEW_THRESHOLD_DAYS)
    );
    println!(
        "  快到期（距到期 10 天）-> needs_renewal = {}（进入窗口，该续了）",
        needs_renewal(not_after, now_soon, RENEW_THRESHOLD_DAYS)
    );

    // ── 4. ACME 真签发操作指引 ─────────────────────────────────────────
    println!("\n==== 二、怎么用 ACME 真签发（教学环境跑不了，仅指引）====\n");
    println!("真签发需要三样，缺一不可：");
    println!("  1) 你能控制 DNS 的公网域名（不能是 localhost）；");
    println!("  2) 该域名 80 端口公网可达（CA 要来验 HTTP-01 挑战）；");
    println!("  3) 先用 Let's Encrypt staging 联调，通过后再切正式。");
    println!("     staging  = {LETSENCRYPT_STAGING}");
    println!("     production = {LETSENCRYPT_PRODUCTION}");
    println!("对应代码：cert_demo::acme::order_certificate(domains, staging_url, store).await");
    println!("集成测试：cargo test -p cert_demo -- --ignored（需具备上述条件）\n");

    // ── 5. 起服务，暴露 /cert/info 与挑战路由 ──────────────────────────
    let now_live = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
    let state = AppState {
        challenges: ChallengeStore::new(),
        cert: Arc::new(CertInfo {
            domains: domains.clone(),
            not_after,
            now: now_live,
            needs_renewal: needs_renewal(not_after, now_live, RENEW_THRESHOLD_DAYS),
        }),
    };

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3007);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("端口 {port} 被占用？换个 PORT 再试"))?;

    println!("==== 三、HTTP 服务已启动 ====");
    println!("  http://{addr}/cert/info                       当前证书信息");
    println!("  http://{addr}{CHALLENGE_PREFIX}<token>   HTTP-01 挑战应答（当前无 token，返回 404）");
    println!("Ctrl-C 退出。");

    axum::serve(listener, app(state))
        .await
        .context("HTTP 服务异常退出")?;

    Ok(())
}
