//! 字段加密的纯逻辑层：密钥环、AES-256-GCM 加解密、盲索引、密钥轮换。
//! 不碰数据库、不碰网络，所以全部能离线单测（见文件底部 tests）。
//!
//! ## 存储格式
//!
//! 密文列统一存文本：`v{n}:{base64(nonce)}:{base64(ciphertext)}`
//! - `v{n}` 是**密钥版本前缀**：解密时按前缀选钥，这是轮换能「新老共存」的全部秘密;
//! - nonce 是 GCM 的 12 字节随机数，**每次加密新造**，明文携带（nonce 不是秘密，
//!   只要求同一把钥下不重复；重复即灾难——两段密文异或即泄露明文异或）；
//! - ciphertext 尾部自带 GCM 的 16 字节认证标签，被改一个比特解密即报错。
//!
//! ## 为什么盲索引不用「确定性加密」？
//!
//! 加密后就没法 `WHERE phone = ?` 了——密文每次都不同（随机 nonce）。
//! 一个诱人的「捷径」是确定性加密（固定 nonce，同明文→同密文），密文列自己就能当索引。
//! 但这等于把「相等关系」白送给拿到数据的人：哪些行手机号相同一目了然，
//! 再配合频率分析（常见号段、区号分布）就能反推明文。而且这把「能解密的钥」
//! 一旦泄露，索引列直接变明文。
//!
//! 折衷方案是**盲索引**：另存一列 `HMAC-SHA256(规范化明文, 索引专用密钥)` 的 hex。
//! - HMAC 不可逆：索引列泄露也还原不出手机号（攻击者最多拿常见号码离线试撞，
//!   而没有索引密钥连试撞都做不到——这正是必须用带密钥的 HMAC 而不是裸 SHA-256 的原因）;
//! - 只支持**精确匹配**，模糊查询/范围查询做不了——这是接受的代价；
//! - 索引密钥**必须独立于加密密钥**：两者用途不同、轮换节奏不同
//!   （加密钥可以轮换，索引钥一换全表索引作废必须重建）、泄露的爆炸半径不同——
//!   加密钥泄露 = 明文泄露，索引钥泄露 = 可离线撞库验证「某号码是否在库里」。
//!   一钥两用会把两种风险绑死在一起。

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
// 生产环境绝不允许：密钥应来自 KMS（信封加密解出的 DEK）或至少是
// 部署系统注入的环境变量/secret，且绝不进代码库与镜像。
// 这里给默认值只是为了教程「clone 下来就能跑」。
/// v1 密钥（hex，32 字节）。模拟「上线初期用的第一代密钥」。
pub const DEV_KEY_V1_HEX: &str = "1111111111111111111111111111111111111111111111111111111111111111";
/// v2 密钥（hex，32 字节）。模拟「轮换后的现役密钥」。
pub const DEV_KEY_V2_HEX: &str = "2222222222222222222222222222222222222222222222222222222222222222";
/// 盲索引专用密钥（hex，32 字节）。与加密密钥必须分开，理由见模块头注释。
pub const DEV_INDEX_KEY_HEX: &str =
    "3333333333333333333333333333333333333333333333333333333333333333";

// ---------------------------------------------------------------------------
// 密钥环
// ---------------------------------------------------------------------------

/// 密钥环：版本号 → 32 字节 AES 密钥，外加「当前用哪个版本加密」。
///
/// 轮换期间环里同时挂着新老两把钥：**加密只用 active 版本，解密按密文前缀选版本**。
/// 老密钥要一直保留到全表轮换完成（[`super::store::rotate_all`]）才能下线。
#[derive(Debug, Clone)]
pub struct KeyRing {
    keys: HashMap<u8, [u8; 32]>,
    active_version: u8,
}

impl KeyRing {
    /// 构造密钥环。active 版本必须真的在环里，否则第一次加密就会炸——
    /// 在构造期把这个配置错误暴露出来，比运行期 panic 好定位得多。
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

    /// 从环境变量装配：FIELD_KEY_V1 / FIELD_KEY_V2（64 位 hex），缺省用教学密钥。
    /// active 版本固定为 v2——模拟「已经完成一次轮换、v1 只留着解老数据」的状态。
    ///
    /// 生产版的这个函数应该是：拿 DEK 密文 → 调 KMS Decrypt → 明文 DEK 只存内存。
    pub fn from_env() -> anyhow::Result<Self> {
        let v1 = std::env::var("FIELD_KEY_V1").unwrap_or_else(|_| DEV_KEY_V1_HEX.into());
        let v2 = std::env::var("FIELD_KEY_V2").unwrap_or_else(|_| DEV_KEY_V2_HEX.into());
        let mut keys = HashMap::new();
        keys.insert(1u8, hex_decode_32(&v1)?);
        keys.insert(2u8, hex_decode_32(&v2)?);
        Self::new(keys, 2)
    }

    /// 当前用于加密的版本号。
    pub fn active_version(&self) -> u8 {
        self.active_version
    }

    /// 复制一个「active 版本不同」的密钥环。演示轮换时用：
    /// 同一批密钥，先以 v1 为 active 写入老数据，再以 v2 为 active 做轮换。
    pub fn with_active_version(&self, version: u8) -> anyhow::Result<Self> {
        Self::new(self.keys.clone(), version)
    }

    fn key(&self, version: u8) -> Option<&[u8; 32]> {
        self.keys.get(&version)
    }
}

/// 盲索引密钥：FIELD_INDEX_KEY（64 位 hex），缺省用教学密钥。
/// 与 [`KeyRing`] 分开加载、分开存放——见模块头「索引密钥必须独立」的讨论。
pub fn index_key_from_env() -> anyhow::Result<[u8; 32]> {
    let hex = std::env::var("FIELD_INDEX_KEY").unwrap_or_else(|_| DEV_INDEX_KEY_HEX.into());
    hex_decode_32(&hex)
}

// ---------------------------------------------------------------------------
// 错误类型
// ---------------------------------------------------------------------------

/// 字段解密/轮换错误。给测试和轮换任务区分用；
/// 若将来接 HTTP 层，对外应统一口径（参考第 19 章 api_crypto 里 Padding Oracle 的教训）。
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
// 加密 / 解密 / 轮换
// ---------------------------------------------------------------------------

/// 加密一个字段值，返回可直接入库的文本 `v{n}:{b64(nonce)}:{b64(ct)}`。
///
/// 每次调用都随机生成新 nonce，所以**同一明文两次加密的结果不同**——
/// 这是故意的（语义安全），也正是「密文列没法直接当查询索引」的原因，
/// 查询靠 [`blind_index`] 那一列。
pub fn encrypt_field(keyring: &KeyRing, plaintext: &str) -> String {
    let key = keyring
        .key(keyring.active_version)
        .expect("KeyRing::new 已保证 active 版本存在");
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    // OsRng 取自操作系统 CSPRNG；12 字节随机 nonce 在单钥加密次数 < 2^32 时碰撞概率可忽略。
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

/// 解密一个入库值。按 `v{n}` 前缀在密钥环里选钥——
/// 所以轮换期间 v1 老行、v2 新行**同表共存**也能各自解开。
pub fn decrypt_field(keyring: &KeyRing, stored: &str) -> Result<String, FieldError> {
    let (version, nonce, ct) = parse_stored(stored)?;
    let key = keyring
        .key(version)
        .ok_or(FieldError::UnknownVersion(version))?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    // GCM 是 AEAD：解密与完整性校验一步完成，密文被改任何一比特都在这里报错，
    // 不存在「解出一串脏数据」的中间状态。
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), ct.as_slice())
        .map_err(|_| FieldError::Decrypt)?;
    // 我们只加密过合法 UTF-8，解出来不是 UTF-8 说明数据被动过（且骗过标签的概率可忽略），
    // 归入 Decrypt 一类即可。
    String::from_utf8(plaintext).map_err(|_| FieldError::Decrypt)
}

/// 密钥轮换：老版本密文 → 解密 → 用 active 版本重加密。
///
/// **幂等**：已经是 active 版本的密文原样返回（不重新加密）。这让全表轮换任务
/// 可以安全重跑——中断后从头再来，已轮换的行是 no-op，不产生多余写放大。
pub fn rotate_field(keyring: &KeyRing, stored: &str) -> Result<String, FieldError> {
    let (version, _, _) = parse_stored(stored)?;
    if version == keyring.active_version {
        return Ok(stored.to_string());
    }
    let plaintext = decrypt_field(keyring, stored)?;
    Ok(encrypt_field(keyring, &plaintext))
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

// ---------------------------------------------------------------------------
// 盲索引
// ---------------------------------------------------------------------------

/// 计算盲索引：规范化明文后取 `HMAC-SHA256(明文, 索引密钥)` 的 hex。
///
/// 规范化在算索引**之前**做，否则 `"138 0000 1234"` 和 `"13800001234"`
/// 会落到两个不同的索引值上，用户换个输入格式就查不到自己——
/// 盲索引只认字节级完全相等，容错必须靠先归一化。
/// 写入和查询必须走**同一个**规范化函数（本函数内部保证）。
pub fn blind_index(index_key: &[u8], plaintext: &str) -> String {
    let normalized = normalize(plaintext);
    // 完全限定语法：aes-gcm 的 `KeyInit` 与 hmac 的 `Mac` 都提供 new_from_slice，
    // 两个 trait 同时在作用域时必须显式指明用哪个。
    let mut mac = <HmacSha256 as Mac>::new_from_slice(index_key).expect("HMAC 接受任意长度的密钥");
    mac.update(normalized.as_bytes());
    hex_encode(&mac.finalize().into_bytes())
}

/// 规范化：trim + 去掉所有空白与连字符。
/// 手机号/证件号的常见「格式噪音」就这两类；业务上还可按需加大小写折叠、全半角转换等。
fn normalize(plaintext: &str) -> String {
    plaintext
        .trim()
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect()
}

// ---------------------------------------------------------------------------
// hex 辅助（避免为两个小函数引入 hex crate）
// ---------------------------------------------------------------------------

/// 字节 → 小写 hex 文本。盲索引列存 hex 而不是 base64：
/// 大小写不敏感、无特殊字符，肉眼比对与手写 SQL 都更省心。
pub fn hex_encode(bytes: &[u8]) -> String {
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
// 离线单测：纯逻辑不依赖数据库，`cargo test -p db_crypto` 默认全部执行
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用密钥环直接构造，不读环境变量——单测不应受外部环境影响。
    fn test_keyring() -> KeyRing {
        let mut keys = HashMap::new();
        keys.insert(1u8, [0x11; 32]);
        keys.insert(2u8, [0x22; 32]);
        KeyRing::new(keys, 2).unwrap()
    }

    #[test]
    fn roundtrip() {
        let kr = test_keyring();
        let stored = encrypt_field(&kr, "13800001234");
        assert!(
            stored.starts_with("v2:"),
            "密文应带 active 版本前缀: {stored}"
        );
        assert_eq!(decrypt_field(&kr, &stored).unwrap(), "13800001234");
    }

    #[test]
    fn same_plaintext_different_ciphertext() {
        // 随机 nonce ⇒ 同明文两次加密结果不同（语义安全）。
        // 这也正是为什么需要另一列盲索引来做相等查询。
        let kr = test_keyring();
        let a = encrypt_field(&kr, "13800001234");
        let b = encrypt_field(&kr, "13800001234");
        assert_ne!(a, b);
        // 但都能解回同一明文。
        assert_eq!(
            decrypt_field(&kr, &a).unwrap(),
            decrypt_field(&kr, &b).unwrap()
        );
    }

    #[test]
    fn tampered_ciphertext_rejected() {
        let kr = test_keyring();
        let stored = encrypt_field(&kr, "110101199001011234");
        // 篡改密文段最后一个 base64 字符（保证与原字符不同且仍是合法 base64 字母表）。
        let mut chars: Vec<char> = stored.chars().collect();
        let last = chars.len() - 1;
        chars[last] = if chars[last] == 'A' { 'B' } else { 'A' };
        let tampered: String = chars.into_iter().collect();
        // 可能因 base64 padding 校验失败判为 Malformed，或标签校验失败判为 Decrypt，
        // 唯一不允许的是「成功解出脏数据」。
        assert!(decrypt_field(&kr, &tampered).is_err());
    }

    #[test]
    fn version_prefix_parsing_and_unknown_version() {
        let kr = test_keyring();
        // v1 加密的数据，v2 为 active 的环也能按前缀选钥解开（轮换期共存的关键）。
        let kr_v1 = kr.with_active_version(1).unwrap();
        let old = encrypt_field(&kr_v1, "old-data");
        assert!(old.starts_with("v1:"));
        assert_eq!(decrypt_field(&kr, &old).unwrap(), "old-data");

        // 环里没有 v7：把合法密文的前缀改成 v7，应报 UnknownVersion(7)。
        let unknown = format!("v7{}", old.strip_prefix("v1").unwrap());
        assert_eq!(
            decrypt_field(&kr, &unknown),
            Err(FieldError::UnknownVersion(7))
        );

        // 各种格式垃圾都应是 Malformed，而不是 panic。
        for bad in [
            "",
            "v2",
            "v2:",
            "v2:!!:!!",
            "x2:AAAA:AAAA",
            "v300:AAAA:AAAA",
            "plaintext",
        ] {
            assert_eq!(
                decrypt_field(&kr, bad),
                Err(FieldError::Malformed),
                "input: {bad:?}"
            );
        }
    }

    #[test]
    fn blind_index_deterministic_and_normalized() {
        let key = [0x33u8; 32];
        let a = blind_index(&key, "13800001234");
        // 确定性：同明文同密钥永远同索引——这是它能当 WHERE 条件的前提。
        assert_eq!(a, blind_index(&key, "13800001234"));
        // 规范化：空格/连字符/首尾空白都不影响索引值。
        assert_eq!(a, blind_index(&key, "138 0000 1234"));
        assert_eq!(a, blind_index(&key, "138-0000-1234"));
        assert_eq!(a, blind_index(&key, "  13800001234  "));
        // 不同明文不同索引。
        assert_ne!(a, blind_index(&key, "13800001235"));
        // 输出是 64 位小写 hex（SHA-256 = 32 字节）。
        assert_eq!(a.len(), 64);
        assert!(
            a.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }

    #[test]
    fn blind_index_key_separation() {
        // 不同索引密钥 ⇒ 不同索引值：没有密钥的人无法离线算出「某号码的索引」去撞库。
        let a = blind_index(&[0x33u8; 32], "13800001234");
        let b = blind_index(&[0x44u8; 32], "13800001234");
        assert_ne!(a, b);
    }

    #[test]
    fn rotate_field_upgrades_and_is_idempotent() {
        let kr = test_keyring();
        let kr_v1 = kr.with_active_version(1).unwrap();
        let old = encrypt_field(&kr_v1, "13800001234");

        // v1 → v2：前缀升级，明文不变。
        let rotated = rotate_field(&kr, &old).unwrap();
        assert!(
            rotated.starts_with("v2:"),
            "轮换后应是 active 版本: {rotated}"
        );
        assert_eq!(decrypt_field(&kr, &rotated).unwrap(), "13800001234");

        // 幂等：已是 active 版本 ⇒ 原样返回（连 nonce 都不变，说明没有重新加密）。
        assert_eq!(rotate_field(&kr, &rotated).unwrap(), rotated);
    }

    #[test]
    fn keyring_construction_validates_active() {
        let mut keys = HashMap::new();
        keys.insert(1u8, [0x11; 32]);
        // active 指向不存在的版本，构造期就报错。
        assert!(KeyRing::new(keys, 9).is_err());
    }

    #[test]
    fn hex_helpers() {
        assert_eq!(hex_encode(&[0x00, 0xff, 0x1a]), "00ff1a");
        assert_eq!(hex_decode_32(DEV_KEY_V1_HEX).unwrap(), [0x11; 32]);
        assert!(hex_decode_32("abcd").is_err()); // 长度不对
        assert!(hex_decode_32(&"zz".repeat(32)).is_err()); // 非法字符
    }
}
