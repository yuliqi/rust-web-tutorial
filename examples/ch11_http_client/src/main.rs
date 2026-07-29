//! 第 11 章：HTTP 客户端与 JSON
//!
//! 练习入口：`src/exercises.rs`（做题）→ `src/solutions.rs`（对照答案）
//! 只跑练习测试：`cargo test -p ch11_http_client -- --ignored`

mod exercises;
mod solutions;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct CreateTodo {
    title: String,
    #[serde(default)]
    done: bool,
}

#[derive(Debug, Deserialize)]
struct HttpBinResponse {
    json: Option<CreateTodo>,
}

async fn demo_http() -> Result<()> {
    let client = reqwest::Client::new();
    let payload = CreateTodo {
        title: "learn axum".into(),
        done: false,
    };

    let resp = client
        .post("https://httpbin.org/post")
        .json(&payload)
        .send()
        .await
        .context("send request")?
        .error_for_status()
        .context("bad status")?;

    let body: HttpBinResponse = resp.json().await.context("decode json")?;
    println!("httpbin echo: {:?}", body.json);
    Ok(())
}

fn demo_local_json() -> Result<()> {
    let payload = CreateTodo {
        title: "offline demo".into(),
        done: true,
    };
    let text = serde_json::to_string_pretty(&payload)?;
    println!("local json:\n{text}");
    let back: CreateTodo = serde_json::from_str(&text)?;
    println!("roundtrip title={}", back.title);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    demo_local_json()?;

    match demo_http().await {
        Ok(()) => {}
        Err(e) => {
            // 网络受限时不让示例失败退出
            eprintln!("http demo skipped: {e:#}");
        }
    }

    Ok(())
}
