//! 第 4 章练习：把每个 `todo!()` 换成你的实现。
//!
//! 做题流程：
//! 1. 只跑练习测试（未完成时会失败，这是预期的）：
//!    `cargo test -p ch04_errors -- --ignored`
//! 2. 全部通过后，对照参考答案 `src/solutions.rs`（先自己做，别偷看）。
#![allow(dead_code)]
#![allow(unused_variables)]

use thiserror::Error;

/// 练习共用的错误枚举。已给出，无需修改。
///
/// 注意 `BadPort` 上的 `#[from]`：thiserror 会自动生成
/// `impl From<std::num::ParseIntError> for ConfigError`，
/// 这正是练习 2 里 `?` 能自动转换错误类型的原因。
#[derive(Debug, Error, PartialEq)]
pub enum ConfigError {
    #[error("empty input")]
    Empty,
    #[error("missing '=' in line: {0}")]
    MissingEquals(String),
    #[error("invalid port: {0}")]
    BadPort(#[from] std::num::ParseIntError),
    #[error("key not found: {0}")]
    KeyNotFound(String),
}

/// 练习 1：解析一行 `key=value`（本章练习 1 的可测试版本）。
///
/// - 整行 trim 后为空 → `Err(ConfigError::Empty)`
/// - 没有 '=' → `Err(ConfigError::MissingEquals(trim 后的整行))`
/// - 否则 → `Ok((key, value))`，key/value 各自 trim 过
///
/// 提示：`line.split_once('=')` 返回 `Option<(&str, &str)>`，
/// 配合 `let-else` 或 `ok_or_else` 转成 Result。返回自定义错误枚举
/// 而不是 `String`，调用方才能用 match 区分「哪种错」并分别处理——
/// 这是「错误即类型」相对字符串错误的核心优势。
pub fn parse_kv(line: &str) -> Result<(String, String), ConfigError> {
    todo!()
}

/// 练习 2：解析端口号，体验 `?` + `#[from]` 的自动错误转换。
///
/// 把 `raw` trim 后解析成 `u16`；解析失败要返回 `ConfigError::BadPort`。
///
/// 提示：`ConfigError` 已通过 `#[from]` 实现了 `From<ParseIntError>`，
/// 而 `?` 在错误类型不一致时会自动调用 `From::from` 做转换——
/// 这就是「错误枚举 + From」搭配 `?` 的价值：不用手写 `map_err`。
/// 想清楚这一点后，函数体一行就能写完。
pub fn parse_port(raw: &str) -> Result<u16, ConfigError> {
    todo!()
}

/// 练习 3：从多行配置文本里取出 `port`，练习 `?` 传播链。
///
/// 规则：
/// - 逐行处理，跳过空行和以 '#' 开头的注释行（先 trim 再判断）
/// - 其余每行用 `parse_kv` 解析，出错时用 `?` 直接上抛
/// - 找到 key 为 "port" 的行，用 `parse_port` 解析 value 并返回
/// - 扫描完没找到 → `Err(ConfigError::KeyNotFound("port".into()))`
///
/// 提示：`parse_kv(line)?` 和 `parse_port(&v)?` 串成一条传播链，
/// 任何一层出错都会立刻带着具体的错误变体返回，主流程不会被
/// 层层 match / if-else 淹没——这就是 `?` 相比手动传错的可读性优势。
pub fn get_port(config_text: &str) -> Result<u16, ConfigError> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "练习：完成后运行 cargo test -p ch04_errors -- --ignored"]
    fn ex1_parse_kv() {
        assert_eq!(
            parse_kv("host = localhost"),
            Ok(("host".to_string(), "localhost".to_string()))
        );
        assert_eq!(parse_kv("   "), Err(ConfigError::Empty));
        assert!(matches!(
            parse_kv("broken"),
            Err(ConfigError::MissingEquals(_))
        ));
    }

    #[test]
    #[ignore = "练习：完成后运行 cargo test -p ch04_errors -- --ignored"]
    fn ex2_parse_port() {
        assert_eq!(parse_port(" 8080 "), Ok(8080));
        assert!(matches!(parse_port("abc"), Err(ConfigError::BadPort(_))));
    }

    #[test]
    #[ignore = "练习：完成后运行 cargo test -p ch04_errors -- --ignored"]
    fn ex3_get_port() {
        let text = "# app config\nhost=localhost\nport=3000\n";
        assert_eq!(get_port(text), Ok(3000));
        assert_eq!(
            get_port("host=localhost"),
            Err(ConfigError::KeyNotFound("port".to_string()))
        );
        assert!(matches!(get_port("port=abc"), Err(ConfigError::BadPort(_))));
        assert!(matches!(
            get_port("broken line"),
            Err(ConfigError::MissingEquals(_))
        ));
    }
}
