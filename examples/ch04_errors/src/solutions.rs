//! 第 4 章练习参考答案。先自己完成 `exercises.rs`，再来对照。
//!
//! 本文件的测试与练习测试断言完全相同，且默认随 `cargo test` 运行，
//! 保证参考答案本身始终是对的。
#![allow(dead_code)]

use thiserror::Error;

/// 与 `exercises.rs` 相同的错误枚举；`#[from]` 生成的 `From` 实现
/// 让练习 2 的 `?` 可以把 `ParseIntError` 自动升级成 `ConfigError`。
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

/// 练习 1 参考答案：先 trim 处理空输入，再用 `let-else` + `split_once`
/// 处理缺 '=' 的行。每种失败都是一个独立的枚举变体，
/// 调用方可以 match 出「哪种错」，而不是从字符串里猜。
pub fn parse_kv(line: &str) -> Result<(String, String), ConfigError> {
    let line = line.trim();
    if line.is_empty() {
        return Err(ConfigError::Empty);
    }
    let Some((k, v)) = line.split_once('=') else {
        return Err(ConfigError::MissingEquals(line.to_string()));
    };
    Ok((k.trim().to_string(), v.trim().to_string()))
}

/// 练习 2 参考答案：一行搞定。`parse::<u16>()` 失败产生 `ParseIntError`，
/// `?` 发现它与返回类型的错误不一致，就调用 `#[from]` 生成的
/// `From::from` 转成 `ConfigError::BadPort`——不需要任何 `map_err`。
pub fn parse_port(raw: &str) -> Result<u16, ConfigError> {
    Ok(raw.trim().parse()?)
}

/// 练习 3 参考答案：`parse_kv(...)?` 加上直接 `return parse_port(&v)`
/// 构成一条传播链（尾调用处返回 Result 本身，效果等同再写一个 `?`），
/// 任何一层出错都带着具体变体立刻返回；正常路径保持一条直线，
/// 「没找到 port」则显式返回 `KeyNotFound`——不吞错、不层层 match。
pub fn get_port(config_text: &str) -> Result<u16, ConfigError> {
    for line in config_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let (k, v) = parse_kv(trimmed)?;
        if k == "port" {
            return parse_port(&v);
        }
    }
    Err(ConfigError::KeyNotFound("port".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
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
    fn ex2_parse_port() {
        assert_eq!(parse_port(" 8080 "), Ok(8080));
        assert!(matches!(parse_port("abc"), Err(ConfigError::BadPort(_))));
    }

    #[test]
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
