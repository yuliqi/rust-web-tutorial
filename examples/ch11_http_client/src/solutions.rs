//! 第 11 章练习参考答案。先自己完成 `exercises.rs`，再来对照。
//!
//! 本文件的测试与练习测试断言完全相同，且默认随 `cargo test` 运行，
//! 保证参考答案本身始终是对的。
#![allow(dead_code)]

use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
pub struct Todo {
    pub id: u64,
    pub title: String,
    #[serde(default)]
    pub done: bool,
}

/// 练习 1 参考答案：`from_str::<Vec<Todo>>` 由返回类型驱动反序列化，
/// `?` 把 serde 错误直接向上传；`into_iter` 拿走所有权，`title` 就能
/// 直接 move 出来，不用 clone。缺失的 `done` 靠 `#[serde(default)]`
/// 落成 false，所以 id=2 也算未完成。
pub fn pending_titles(json: &str) -> Result<Vec<String>, serde_json::Error> {
    let todos: Vec<Todo> = serde_json::from_str(json)?;
    Ok(todos
        .into_iter()
        .filter(|t| !t.done)
        .map(|t| t.title)
        .collect())
}

/// 练习 2 参考答案：`trim_end_matches('/')` 消除 base 尾部斜杠与
/// path 头部斜杠的重复；参数拼接交给 `join("&")`，它天然处理了
/// 「空集合不加分隔符」的边界，剩下只需判断要不要加 `?`。
pub fn build_query_url(base: &str, path: &str, params: &[(&str, &str)]) -> String {
    let mut url = format!("{}{}", base.trim_end_matches('/'), path);
    if !params.is_empty() {
        let query: Vec<String> = params.iter().map(|(k, v)| format!("{k}={v}")).collect();
        url.push('?');
        url.push_str(&query.join("&"));
    }
    url
}

#[derive(Debug, PartialEq)]
pub enum ApiOutcome {
    Success,
    NotFound,
    RateLimited,
    ClientError,
    ServerError,
    Unknown,
}

/// 练习 3 参考答案：match 从上到下匹配，所以 404 / 429 这两个「更具体」
/// 的分支必须排在 `400..=499` 区间之前；区间模式让 2xx/4xx/5xx 的分组
/// 一目了然，比一串 `if code >= 400 && code < 500` 可读得多。
pub fn classify_status(code: u16) -> ApiOutcome {
    match code {
        200..=299 => ApiOutcome::Success,
        404 => ApiOutcome::NotFound,
        429 => ApiOutcome::RateLimited,
        400..=499 => ApiOutcome::ClientError,
        500..=599 => ApiOutcome::ServerError,
        _ => ApiOutcome::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ex1_pending_titles() {
        let json = r#"[
            {"id": 1, "title": "learn serde", "done": true},
            {"id": 2, "title": "learn reqwest"},
            {"id": 3, "title": "learn axum", "done": false}
        ]"#;
        assert_eq!(
            pending_titles(json).unwrap(),
            vec!["learn reqwest", "learn axum"]
        );
        assert!(pending_titles("not json").is_err());
    }

    #[test]
    fn ex2_build_query_url() {
        assert_eq!(
            build_query_url(
                "https://api.example.com/",
                "/todos",
                &[("page", "2"), ("done", "false")]
            ),
            "https://api.example.com/todos?page=2&done=false"
        );
        assert_eq!(
            build_query_url("https://api.example.com", "/todos", &[]),
            "https://api.example.com/todos"
        );
    }

    #[test]
    fn ex3_classify_status() {
        assert_eq!(classify_status(200), ApiOutcome::Success);
        assert_eq!(classify_status(204), ApiOutcome::Success);
        assert_eq!(classify_status(404), ApiOutcome::NotFound);
        assert_eq!(classify_status(429), ApiOutcome::RateLimited);
        assert_eq!(classify_status(400), ApiOutcome::ClientError);
        assert_eq!(classify_status(503), ApiOutcome::ServerError);
        assert_eq!(classify_status(302), ApiOutcome::Unknown);
    }
}
