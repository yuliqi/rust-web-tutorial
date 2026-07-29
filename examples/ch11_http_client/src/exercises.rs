//! 第 11 章练习：把每个 `todo!()` 换成你的实现。
//!
//! 做题流程：
//! 1. 只跑练习测试（未完成时会失败，这是预期的）：
//!    `cargo test -p ch11_http_client -- --ignored`
//! 2. 全部通过后，对照参考答案 `src/solutions.rs`（先自己做，别偷看）。
//!
//! 本章练习全部离线可测：JSON 解析、URL 拼接、状态码分类，
//! 正是 `reqwest` 调 API 前后你真正要写的那几段代码。
#![allow(dead_code)]
#![allow(unused_variables)]

use serde::Deserialize;

/// 练习用 DTO：对应 `GET /todos` 返回列表里的单个条目。
/// `#[serde(default)]` 让缺失的 `done` 字段回退为 false——
/// 对外部 API 的「字段可能缺」保持宽容是 response DTO 的常见写法。
#[derive(Debug, Deserialize, PartialEq)]
pub struct Todo {
    pub id: u64,
    pub title: String,
    #[serde(default)]
    pub done: bool,
}

/// 练习 1：解析 API 返回的 JSON 数组，取出所有「未完成」条目的标题。
///
/// 要求：把 `json` 反序列化为 `Vec<Todo>`，筛出 `done == false` 的，
/// 按原顺序返回它们的 `title`；JSON 非法时把 serde 错误原样传出去。
///
/// 提示：`serde_json::from_str::<Vec<Todo>>(json)?` 一行即可完成解析——
/// 类型标注驱动反序列化是 serde 的核心用法；真实场景里等价于
/// `resp.json::<Vec<Todo>>().await?`。后半段用迭代器 filter + map 收集。
pub fn pending_titles(json: &str) -> Result<Vec<String>, serde_json::Error> {
    todo!("from_str -> filter(!done) -> map(title) -> collect")
}

/// 练习 2：拼接带查询参数的请求 URL。
///
/// 要求：
/// - `base` 末尾可能带 `/`，要去掉后再接 `path`（`path` 以 `/` 开头）
/// - `params` 为空 → 不要出现 `?`
/// - 否则接 `?k1=v1&k2=v2`（保持切片顺序）
///
/// 提示：`base.trim_end_matches('/')` 处理斜杠；参数用迭代器
/// `map(|(k, v)| format!("{k}={v}"))` 再 `collect::<Vec<_>>().join("&")`。
/// 真实项目里 `reqwest` 的 `.query(&[("page", "2")])` 会帮你做这件事
/// （还带百分号转义），手写一遍是为了理解它到底生成了什么。
pub fn build_query_url(base: &str, path: &str, params: &[(&str, &str)]) -> String {
    todo!("trim_end_matches('/') + format! + join(\"&\")")
}

/// HTTP 状态码分类后的业务结果，练习 3 使用。
#[derive(Debug, PartialEq)]
pub enum ApiOutcome {
    /// 2xx
    Success,
    /// 404
    NotFound,
    /// 429（触发限流，通常应退避重试）
    RateLimited,
    /// 其余 4xx
    ClientError,
    /// 5xx
    ServerError,
    /// 其他（1xx / 3xx 等）
    Unknown,
}

/// 练习 3：把 HTTP 状态码映射为业务枚举 `ApiOutcome`。
///
/// 要求见 `ApiOutcome` 各变体的注释。
///
/// 提示：用 `match` 的区间模式 `200..=299`；注意分支从上到下依次匹配，
/// 具体值（404、429）必须写在 `400..=499` 这种区间之前，否则会被区间
/// 先「吃掉」。真实场景里 `resp.status().as_u16()` 拿到码后就该做这种
/// 翻译，而不是把裸数字散落在业务代码里。
pub fn classify_status(code: u16) -> ApiOutcome {
    todo!("match code：先写 404 / 429，再写 200..=299 等区间")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "练习：完成后运行 cargo test -p ch11_http_client -- --ignored"]
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
    #[ignore = "练习：完成后运行 cargo test -p ch11_http_client -- --ignored"]
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
    #[ignore = "练习：完成后运行 cargo test -p ch11_http_client -- --ignored"]
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
