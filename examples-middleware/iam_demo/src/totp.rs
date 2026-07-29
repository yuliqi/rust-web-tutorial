//! 两步验证（2FA）核心：基于时间的一次性密码（TOTP，RFC 6238）+ 一次性恢复码。
//!
//! 为什么要两步：密码是「你知道的东西」，一旦泄露（撞库、钓鱼、库被拖）账号就沦陷。
//! TOTP 叠加「你拥有的东西」——手机里 Authenticator 每 30 秒滚动的 6 位码，攻击者
//! 光有密码也进不来。它和服务端共享一个密钥，双方各自用「当前时间 ÷ 30 秒」当计数器
//! 算 HMAC，因此离线也能算出同一个码，不依赖短信（短信可被 SIM 劫持）。
//!
//! ⚠️ 安全注释（呼应第 20 章字段加密）：TOTP secret 是**和密码同级的敏感数据**——
//! 拿到它就能永久生成有效验证码，等于绕过第二步。生产必须像 db_crypto 那样用
//! AES-GCM 加密后再落库（列名里的 `totp_secret_enc` 的 `_enc` 就是这个约定）。
//! 本示例为聚焦授权逻辑而从简，明文存但用列名和注释点明红线。

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rand::Rng;
use totp_rs::{Algorithm, Secret, TOTP};

/// TOTP 参数：SHA1 + 6 位 + 30 秒步长，是 RFC 6238 的默认组合，也是所有主流
/// Authenticator（Google/Microsoft/1Password）默认识别的一组，改了它们就扫不出来。
const ALGORITHM: Algorithm = Algorithm::SHA1;
const DIGITS: usize = 6;
const STEP_SECS: u64 = 30;
/// skew=1：校验时额外接受「前一个 + 后一个」时间窗口的码，容忍手机与服务器之间
/// 最多约 ±30 秒的时钟漂移。放大它更宽容，但也把一个码的可用寿命拉长、削弱安全性——
/// 1 是安全与体验的通行折中。
const SKEW: u8 = 1;

/// 用给定密钥构造一个 TOTP 实例。issuer/account 只影响 otpauth URI 的展示，
/// 不参与验证码计算，所以校验路径可以随便填占位值。
fn build(secret_b32: &str, issuer: &str, account: &str) -> anyhow::Result<TOTP> {
    // Secret::Encoded 表示「这串是 base32 文本」，to_bytes() 解回原始密钥字节。
    let bytes = Secret::Encoded(secret_b32.to_string())
        .to_bytes()
        .map_err(|e| anyhow::anyhow!("decode base32 secret: {e:?}"))?;
    TOTP::new(
        ALGORITHM,
        DIGITS,
        SKEW,
        STEP_SECS,
        bytes,
        Some(issuer.to_string()),
        account.to_string(),
    )
    .map_err(|e| anyhow::anyhow!("build TOTP: {e}"))
}

/// 生成一枚随机 TOTP 密钥，返回 base32 文本（Authenticator 手动录入用的就是这串）。
/// 每个用户一枚，开启 2FA 时生成、加密落库。
pub fn generate_secret() -> String {
    Secret::generate_secret().to_encoded().to_string()
}

/// 生成 otpauth:// URI——把它塞进二维码，Authenticator 扫一下即可添加账号。
///
/// 格式：`otpauth://totp/{issuer}:{account}?secret=...&issuer=...`
/// - scheme 固定 `otpauth`、type 固定 `totp`（区别于计数型的 `hotp`）；
/// - path 里的 `issuer:account` 是 Authenticator 列表中显示的名字；
/// - query 里的 secret 才是真正共享的密钥。
///
/// digits/period/algorithm 我们用的都是 RFC 默认值（6/30/SHA1），按规范可省略，
/// Authenticator 缺省即取这组默认——所以 totp-rs 生成的 URI 里看不到它们。
/// 这是 Google 定义、已成事实标准的格式，各家 Authenticator 都认。
pub fn otpauth_uri(issuer: &str, account: &str, secret: &str) -> anyhow::Result<String> {
    Ok(build(secret, issuer, account)?.get_url())
}

/// 校验一枚验证码：给定密钥、用户输入的码、以及「此刻」的 unix 秒。
/// 传入 at_unix 而不是内部读时钟，是为了让单元测试可控、可复现（见下方 tests）。
/// 内部按 skew=1 检查前后各一个 30 秒窗口，任一命中即通过。
pub fn verify_code(secret: &str, code: &str, at_unix: u64) -> bool {
    match build(secret, "issuer", "account") {
        // check(token, time)：库内部按 step/skew 枚举合法窗口比对，常数时间防时序侧信道。
        Ok(totp) => totp.check(code, at_unix),
        // 密钥坏了当作校验失败，不给调用方区分「码错」还是「密钥坏」的机会。
        Err(_) => false,
    }
}

/// 当前时刻应当呈现的验证码。仅用于 main.rs 自测演示与测试，生产服务端不需要
/// 「生成」码（码由用户手机生成、服务端只校验）。
pub fn current_code(secret: &str, at_unix: u64) -> anyhow::Result<String> {
    Ok(build(secret, "issuer", "account")?.generate(at_unix))
}

// ---------- 一次性恢复码（backup codes）----------

/// 生成 n 个一次性恢复码。手机丢了 / 换机没迁移 Authenticator 时，用户还能靠
/// 这些码登录——否则 2FA 反而把自己锁死在门外。每个码**用一次即作废**。
///
/// 返回的是明文，只在生成的这一刻展示给用户（让 TA 抄下来存好），
/// **服务端只存哈希**（见 `hash_backup_code`）：库被拖走也无法反推出可用的码。
pub fn generate_backup_codes(n: usize) -> Vec<String> {
    // 无歧义字符集：去掉了 0/O、1/I/L 这类肉眼易混的，用户手抄不容易错。
    const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let mut rng = rand::thread_rng();
    (0..n)
        .map(|_| {
            let code: String = (0..8)
                .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
                .collect();
            // 中间加个连字符，形如 ABCD-2345，便于阅读与录入。
            format!("{}-{}", &code[..4], &code[4..])
        })
        .collect()
}

/// 恢复码的哈希：和密码同级的敏感凭证，所以复用第 17 章 auth.rs 的 argon2 思路
/// （随机盐 + 抗 GPU 暴力）而不是裸 SHA256。落库存这个字符串。
pub fn hash_backup_code(code: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(code.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hash backup code: {e}"))?;
    Ok(hash.to_string())
}

/// 校验一枚恢复码：在「尚未用过的哈希列表」里找匹配项，命中则返回它的下标，
/// 调用方据此把这一条标记为已用（一次性的关键——用过就从可用集合里剔除）。
/// 返回 None 表示没有任何一条匹配。
pub fn verify_backup_code(unused_hashes: &[String], code: &str) -> Option<usize> {
    unused_hashes.iter().position(|stored| {
        PasswordHash::new(stored)
            .map(|parsed| {
                Argon2::default()
                    .verify_password(code.as_bytes(), &parsed)
                    .is_ok()
            })
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // 用固定时间戳让测试可复现：不依赖「跑测试的那一刻」的真实时钟。
    const T0: u64 = 1_700_000_000;

    #[test]
    fn secret_is_base32_and_decodable() {
        let secret = generate_secret();
        // base32 只含 A-Z2-7；能被 build() 解码就说明格式对。
        assert!(secret
            .chars()
            .all(|c| c.is_ascii_uppercase() || ('2'..='7').contains(&c)));
        assert!(build(&secret, "i", "a").is_ok());
    }

    #[test]
    fn generated_code_verifies_at_same_instant() {
        let secret = generate_secret();
        let code = current_code(&secret, T0).unwrap();
        assert_eq!(code.len(), 6);
        // 同一时刻算出的码，校验必过——这是 TOTP「两端各自算、结果一致」的根据。
        assert!(verify_code(&secret, &code, T0));
    }

    #[test]
    fn wrong_code_rejected() {
        let secret = generate_secret();
        let code = current_code(&secret, T0).unwrap();
        // 把码改一位数字，必被拒。
        let tampered = if code.starts_with('0') { "199999" } else { "000000" };
        // 极小概率 tampered 恰好等于真码，换一个错码规避。
        let bad = if tampered == code { "111111" } else { tampered };
        assert!(!verify_code(&secret, bad, T0));
    }

    #[test]
    fn code_from_far_future_window_rejected() {
        let secret = generate_secret();
        // 在 T0 生成的码，拿到 5 分钟后（远超 skew=1 的 ±30 秒容忍窗）校验必须失效——
        // 这正是「一次性、短寿命」的体现：偷到一个旧码也用不了。
        let code = current_code(&secret, T0).unwrap();
        assert!(!verify_code(&secret, &code, T0 + 300));
    }

    #[test]
    fn adjacent_window_tolerated() {
        let secret = generate_secret();
        // 前一个窗口（-30s）生成的码，因 skew=1 仍被接受，容忍时钟漂移。
        let code = current_code(&secret, T0 - STEP_SECS).unwrap();
        assert!(verify_code(&secret, &code, T0));
    }

    #[test]
    fn backup_codes_generated_and_formatted() {
        let codes = generate_backup_codes(10);
        assert_eq!(codes.len(), 10);
        for c in &codes {
            // 形如 ABCD-2345：4 + 1 + 4。
            assert_eq!(c.len(), 9);
            assert_eq!(c.as_bytes()[4], b'-');
        }
        // 随机生成基本不会撞车（去重后仍是 10 个）。
        let mut uniq = codes.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), 10);
    }

    #[test]
    fn backup_code_is_one_time() {
        let codes = generate_backup_codes(3);
        let mut hashes: Vec<String> =
            codes.iter().map(|c| hash_backup_code(c).unwrap()).collect();

        // 用第 2 个码：能命中，返回下标 1。
        let idx = verify_backup_code(&hashes, &codes[1]).unwrap();
        assert_eq!(idx, 1);
        // 一次性：调用方用后即从可用集合剔除。
        hashes.remove(idx);
        // 同一个码再来一次——已不在集合里，必被拒。
        assert!(verify_backup_code(&hashes, &codes[1]).is_none());
        // 其它未用的码不受影响。
        assert!(verify_backup_code(&hashes, &codes[0]).is_some());
    }

    #[test]
    fn wrong_backup_code_rejected() {
        let codes = generate_backup_codes(2);
        let hashes: Vec<String> = codes.iter().map(|c| hash_backup_code(c).unwrap()).collect();
        assert!(verify_backup_code(&hashes, "ZZZZ-9999").is_none());
    }

    #[test]
    fn otpauth_uri_format() {
        let secret = generate_secret();
        let uri = otpauth_uri("IamDemo", "alice@acme.test", &secret).unwrap();
        // scheme/type 固定，path 是 issuer:account，query 带 secret 与 issuer。
        assert!(uri.starts_with("otpauth://totp/IamDemo:"));
        assert!(uri.contains(&format!("secret={secret}")));
        assert!(uri.contains("issuer=IamDemo"));
    }
}
