//! 第 4 章：错误处理
//!
//! 练习入口：`src/exercises.rs`（做题）→ `src/solutions.rs`（对照答案）
//! 只跑练习测试：`cargo test -p ch04_errors -- --ignored`

mod exercises;
mod solutions;

use anyhow::{bail, Context, Result as AnyResult};
use std::fmt::Write as _;
use thiserror::Error;

#[derive(Debug, Error)]
enum ConfigError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid format: {0}")]
    Format(String),
}

fn parse_kv_line(line: &str) -> Result<(String, String), ConfigError> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Err(ConfigError::Format("empty or comment".into()));
    }
    let Some((k, v)) = line.split_once('=') else {
        return Err(ConfigError::Format(format!("missing '=': {line}")));
    };
    let k = k.trim();
    let v = v.trim();
    if k.is_empty() {
        return Err(ConfigError::Format("empty key".into()));
    }
    Ok((k.to_string(), v.to_string()))
}

fn load_config_text(text: &str) -> Result<Vec<(String, String)>, ConfigError> {
    let mut out = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        match parse_kv_line(line) {
            Ok(pair) => out.push(pair),
            Err(ConfigError::Format(msg)) if msg.starts_with("empty or comment") => {}
            Err(e) => {
                return Err(ConfigError::Format(format!("line {}: {e}", idx + 1)));
            }
        }
    }
    Ok(out)
}

fn demo_anyhow(pathish: &str) -> AnyResult<String> {
    // 这里用内存字符串模拟文件，展示 context 风格
    let raw = if pathish.ends_with("empty.toml") {
        String::new()
    } else {
        "host=127.0.0.1\nport=3000\n".to_string()
    };

    if raw.is_empty() {
        bail!("config is empty: {pathish}");
    }

    let pairs = load_config_text(&raw).with_context(|| format!("parse {pathish}"))?;
    let mut rendered = String::new();
    for (k, v) in pairs {
        let _ = writeln!(&mut rendered, "{k} => {v}");
    }
    Ok(rendered)
}

fn main() -> AnyResult<()> {
    match parse_kv_line("host=localhost") {
        Ok((k, v)) => println!("kv: {k}={v}"),
        Err(e) => println!("err: {e}"),
    }

    match parse_kv_line("broken") {
        Ok(_) => {}
        Err(e) => println!("expected format error: {e}"),
    }

    let ok = demo_anyhow("app.toml")?;
    print!("loaded:\n{ok}");

    if let Err(e) = demo_anyhow("empty.toml") {
        println!("anyhow error: {e:#}");
    }

    Ok(())
}
