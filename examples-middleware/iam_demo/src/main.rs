//! 二进制入口 + 一段「跑一遍就懂」的演示。
//!
//! 分两部分：
//! 1. 纯逻辑演示（不连库、永远能跑）：TOTP 自测 + can()/effective_grants 授权判定；
//! 2. 若能连上 Postgres，再走一遍真实数据流：建组织主账号 → 开 2FA → 建两个子账号
//!    分别授权云账号 A / B → 演示跨账号访问被拒、子账号权限不超父。
//!
//! 连不上库不报错退出，只打印 docker 提示，让读者先看懂纯逻辑那半。

use iam_demo::authz::{self, can};
use iam_demo::config::Config;
use iam_demo::model::{Grant, GrantSpec};
use iam_demo::{store, totp};
use std::time::{SystemTime, UNIX_EPOCH};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info,iam_demo=info")
        .init();

    demo_totp()?;
    demo_authz_pure();

    // ---- 需要数据库的部分 ----
    let config = Config::from_env();
    match store::connect(&config.database_url).await {
        Ok(pool) => demo_with_db(&pool).await?,
        Err(e) => {
            println!("\n[跳过数据库演示] 连不上 Postgres：{e:#}");
            println!("先在 examples-middleware/ 下执行 `docker compose up -d postgres` 再重跑。");
        }
    }
    Ok(())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// 演示一：TOTP 全流程自测。开 2FA 时打印 otpauth URI（可转二维码给 Authenticator 扫），
/// 并用「当前时间」算一个有效码，立刻自校验通过——证明生成/校验两端参数一致。
fn demo_totp() -> anyhow::Result<()> {
    println!("=== 演示一：两步验证（TOTP）===");
    let secret = totp::generate_secret();
    let uri = totp::otpauth_uri("IamDemo", "owner@acme.test", &secret)?;
    println!("otpauth URI（扫码添加到 Authenticator）：\n  {uri}");

    let now = unix_now();
    let code = totp::current_code(&secret, now)?;
    println!("当前时刻验证码：{code}");
    println!("同刻校验：{}", totp::verify_code(&secret, &code, now));
    println!(
        "5 分钟后再用同一个码：{}（应为 false——码已过期）",
        totp::verify_code(&secret, &code, now + 300)
    );

    let backups = totp::generate_backup_codes(3);
    println!("一次性恢复码（手机丢了用，每个只能用一次）：{backups:?}");
    Ok(())
}

/// 演示二：授权判定纯逻辑（无需数据库）。这才是本章的核心不变量所在。
fn demo_authz_pure() {
    println!("\n=== 演示二：资源级授权判定（纯逻辑）===");
    let g = |rid: &str, action: &str| Grant {
        id: 0,
        account_id: 0,
        resource_type: "cloud-account".into(),
        resource_id: rid.into(),
        action: action.into(),
    };

    // 子账号 A：只被授权读云账号 A 的资产。
    let child_a = vec![g("A", "read")];
    println!(
        "子账号A 读云账号A：{}（允许）",
        can(&child_a, "cloud-account", "A", "read")
    );
    println!(
        "子账号A 读云账号B：{}（默认拒绝——只授了 A）",
        can(&child_a, "cloud-account", "B", "read")
    );

    // 「子账号权限不超父」：父账号只有 A 的全部动作，子账号却想要 B——被裁掉。
    let parent = vec![g("A", "*")];
    let over = vec![g("A", "read"), g("B", "read")];
    let eff = authz::effective_grants(&over, &parent);
    println!(
        "子账号自称拥有 [A:read, B:read]，父账号只有 A:* → 有效权限裁剪为：{:?}",
        eff.iter()
            .map(|x| format!("{}:{}", x.resource_id, x.action))
            .collect::<Vec<_>>()
    );
}

/// 演示三：真实数据流（连库后）。建组织 → 开 2FA → 两个子账号分别管 A / B →
/// 跨账号访问被拒 → 子账号越权授权被拒。用随机后缀避免与历史数据撞 email。
async fn demo_with_db(pool: &sqlx::PgPool) -> anyhow::Result<()> {
    println!("\n=== 演示三：层级账号 + 资源级授权（数据库）===");
    let tag = unix_now();
    let owner_email = format!("owner+{tag}@acme.test");

    let (org_id, owner_id) =
        store::create_org_owner(pool, &format!("acme-{tag}"), "pro", &owner_email, "password123")
            .await?;
    println!("建组织 org_id={org_id}，主账号 owner_id={owner_id}（{owner_email}）");

    // owner 开启 2FA，并自测登录第二步。
    let secret = totp::generate_secret();
    store::enable_totp(pool, org_id, owner_id, &secret).await?;
    let code = totp::current_code(&secret, unix_now())?;
    println!(
        "owner 开启 2FA，用当前码登录第二步校验：{}",
        store::verify_login_totp(pool, owner_id, &code, unix_now()).await?
    );

    // 两个子账号，分别只能管云账号 A / B。
    let sub_a = store::create_sub_account(
        pool,
        org_id,
        owner_id,
        &format!("ops-a+{tag}@acme.test"),
        "password123",
        "member",
        &[GrantSpec::new("cloud-account", "A", "*")],
    )
    .await?;
    let sub_b = store::create_sub_account(
        pool,
        org_id,
        owner_id,
        &format!("ops-b+{tag}@acme.test"),
        "password123",
        "member",
        &[GrantSpec::new("cloud-account", "B", "*")],
    )
    .await?;
    println!("子账号 sub_a={sub_a}（管云账号 A）、sub_b={sub_b}（管云账号 B）");

    // 跨账号资源隔离：sub_a 的授权里访问 B，判定拒绝。
    let grants_a = store::load_grants(pool, sub_a).await?;
    println!(
        "sub_a 访问云账号 B：{}（应为 false——跨账号被拒）",
        can(&grants_a, "cloud-account", "B", "read")
    );
    println!(
        "sub_a 访问云账号 A：{}（允许）",
        can(&grants_a, "cloud-account", "A", "read")
    );

    // 子账号权限不超父：sub_a 自己只有 A，现在让它往下再开一个孙账号——
    // 孙账号索要 B（sub_a 根本没有的资源）必须被拒（sub_a 的授权上限是 [A:*]）。
    match store::create_sub_account(
        pool,
        org_id,
        sub_a, // 以 sub_a 为父
        &format!("ops-a-child+{tag}@acme.test"),
        "password123",
        "member",
        &[GrantSpec::new("cloud-account", "B", "read")],
    )
    .await
    {
        Err(e) => println!("sub_a 想给孙账号授 B:read（越父）被拒：{e}"),
        Ok(_) => println!("[异常] 越权授权竟然成功了，检查 store 逻辑"),
    }
    // 而孙账号只要 A:read（在 sub_a 的 A:* 范围内）则允许。
    let grandchild = store::create_sub_account(
        pool,
        org_id,
        sub_a,
        &format!("ops-a-ok+{tag}@acme.test"),
        "password123",
        "member",
        &[GrantSpec::new("cloud-account", "A", "read")],
    )
    .await?;
    println!("sub_a 给孙账号授 A:read（在父范围内）成功：grandchild={grandchild}");

    Ok(())
}
