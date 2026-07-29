//! 第 25 章软件许可授权演示：一个 main 串起离线签名授权与在线激活两套流程，全内存运行。
//!
//! 运行：`cargo run -p license_demo`

use license_demo::activation::{ActivationError, ActivationStore, LEASE_SECS};
use license_demo::license::{
    client_verifying_key, issue, machine_fingerprint, vendor_signing_key, verify, License,
    LicenseError,
};

fn main() {
    println!("==== 一、离线签名授权 ====\n");

    // --- 厂商签发机上（持有私钥）---
    let signing_key = vendor_signing_key();

    // 客户端当前机器指纹（真实实现综合主机名 / CPU id / MAC 等；这里给定原料以便复现）。
    let machine_fp = machine_fingerprint("customer-host-01", "cpu-serial-42");

    // 教学固定「现在」时间，避免示例随真实时间漂移影响可读性。
    let now: i64 = 1_800_000_000;
    let thirty_days = 30 * 24 * 3600;

    let lic = License {
        licensee: "示例科技有限公司".to_string(),
        product: "AcmeDB".to_string(),
        edition: "pro".to_string(),
        features: vec!["cluster".to_string(), "audit_log".to_string()],
        machine_fingerprint: machine_fp.clone(),
        issued_at: now,
        expires_at: now + thirty_days,
    };
    let signed = issue(&signing_key, lic).expect("签发失败");
    println!("厂商已签发 pro 版 license，绑定机器指纹 {}", &machine_fp[..16]);
    println!("签名（base64）：{}...\n", &signed.signature_b64[..24]);

    // --- 客户端上（只持有公钥）---
    let vk = client_verifying_key();

    match verify(&vk, &signed, now, &machine_fp) {
        Ok(()) => println!("[验证通过] 签名有效、未过期、机器匹配"),
        Err(e) => println!("[意外失败] {e}"),
    }
    // 功能位解锁：同一个二进制按 features 决定开哪些功能。
    println!(
        "  功能 cluster = {}，功能 geo_replication = {}",
        signed.license.has_feature("cluster"),
        signed.license.has_feature("geo_replication")
    );

    println!("\n-- 三种失败演示 --");

    // ① 篡改内容：偷偷升到 enterprise，签名对不上。
    let mut tampered = signed.clone();
    tampered.license.edition = "enterprise".to_string();
    print_verify("① 篡改版本为 enterprise", &vk, &tampered, now, &machine_fp);

    // ② 改过期时间为过去：注意这里改的是签名覆盖的字段，会先在签名这步就被拦下；
    //    要单纯演示 Expired，用一份「合法但已过期」的 license。
    let expired = issue(
        &signing_key,
        License {
            expires_at: now - 1, // 已过期
            ..signed.license.clone()
        },
    )
    .unwrap();
    print_verify("② 合法但已过期", &vk, &expired, now, &machine_fp);

    // ③ 换机器：license 绑的是 customer-host-01，拿到别的机器上验。
    let other_fp = machine_fingerprint("attacker-host", "cpu-serial-99");
    print_verify("③ 换一台机器", &vk, &signed, now, &other_fp);

    // ------------------------------------------------------------------
    println!("\n==== 二、在线激活 ====\n");

    let store = ActivationStore::new();
    let key = "ACME-PRO-8F2A-1234";
    store.register_key(key, 2); // 这个 key 卖的是「限 2 台机器」
    println!("厂商登记 key={key}，机器数配额 = 2");

    let t0 = 1_800_000_000i64;

    // 客户端激活。
    let act = store.activate(key, &machine_fp, t0).expect("激活失败");
    println!(
        "[activate] 拿到令牌 {}...，租约到 {}（+{}s）",
        &act.activation_token[..8],
        act.lease_expires,
        LEASE_SECS
    );

    // 心跳续租成功。
    let hb = store.heartbeat(&act.activation_token, t0 + 60).unwrap();
    println!("[heartbeat] 续租成功，新租约到 {}", hb.lease_expires);

    // 厂商远程吊销该 key（客户退款 / 违约 / 盗版）。
    store.revoke(key);
    println!("厂商吊销了 key={key}");

    // 客户端下一次心跳被告知失效 —— 这正是在线方案相对离线的核心优势。
    match store.heartbeat(&act.activation_token, t0 + 120) {
        Err(ActivationError::Revoked) => {
            println!("[heartbeat] 已被吊销，客户端应立即停止服务")
        }
        other => println!("[意外结果] {other:?}"),
    }

    println!("\n对比：离线签名可断网、自证有效，但吊销只能等它过期；");
    println!("在线激活能远程吊销与计量，但依赖网络与激活服务。生产常两者结合。");
}

fn print_verify(
    label: &str,
    vk: &ed25519_dalek::VerifyingKey,
    signed: &license_demo::license::SignedLicense,
    now: i64,
    fp: &str,
) {
    let r = verify(vk, signed, now, fp);
    let tag = match &r {
        Err(LicenseError::BadSignature) => "BadSignature",
        Err(LicenseError::Expired) => "Expired",
        Err(LicenseError::MachineMismatch) => "MachineMismatch",
        Err(LicenseError::Malformed) => "Malformed",
        Ok(()) => "Ok(意外通过)",
    };
    println!("{label} -> {tag}");
}
