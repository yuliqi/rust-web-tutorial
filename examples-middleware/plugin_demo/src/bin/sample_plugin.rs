//! 样例插件——本 crate 的第二个可执行文件。
//!
//! 它演示「子进程 + stdio JSON-RPC」方案里**插件那一侧**长什么样：从 stdin 一行行读 JSON-RPC 请求，
//! 处理后把响应写回 stdout（同样一行一条）。除了共用一份协议类型，它对宿主一无所知——
//! 换成 Python/Go 写、只要遵守这个协议，宿主照样能用。这正是子进程方案「语言无关」的体现。
//!
//! 实现两个方法：
//! - `ping`              → `"pong"`
//! - `uppercase`（字符串或 `{text}`）→ 转大写
//!
//! 其它方法 → JSON-RPC「method not found」(-32601) 错误。
//!
//! 读到 EOF（宿主关闭了我们的 stdin）就退出主循环——这就是 [`PluginHost::shutdown`] 的收尾方式。
//!
//! 用 std 阻塞式 io 而非 tokio：插件逻辑是「读一条、处理、写一条」的简单串行循环，
//! 不需要异步运行时，越轻越好。
//!
//! [`PluginHost::shutdown`]: plugin_demo::plugin::PluginHost::shutdown

use std::io::{self, BufRead, Write};

use plugin_demo::plugin::{RpcRequest, RpcResponse};
use serde_json::{json, Value};

/// JSON-RPC 标准错误码：解析失败 / 方法不存在 / 参数非法。
const PARSE_ERROR: i64 = -32700;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break, // 读管道出错，等同断开
        };
        if line.trim().is_empty() {
            continue;
        }

        let resp = handle_line(&line);
        // 写回一行 JSON 并立即 flush——不 flush 的话响应可能滞留在缓冲里，宿主 read_line 会一直等。
        let _ = writeln!(out, "{}", serde_json::to_string(&resp).unwrap_or_default());
        let _ = out.flush();
    }
}

/// 解析一行请求并派发。解析失败时尽量还原 id（拿不到就用 0）以便宿主配对。
fn handle_line(line: &str) -> RpcResponse {
    let req: RpcRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            let id = serde_json::from_str::<Value>(line)
                .ok()
                .and_then(|v| v.get("id").and_then(Value::as_u64))
                .unwrap_or(0);
            return RpcResponse::err(id, PARSE_ERROR, format!("parse error: {e}"));
        }
    };

    match req.method.as_str() {
        "ping" => RpcResponse::ok(req.id, json!("pong")),
        "uppercase" => match extract_text(&req.params) {
            Some(text) => RpcResponse::ok(req.id, json!(text.to_uppercase())),
            None => RpcResponse::err(
                req.id,
                INVALID_PARAMS,
                "uppercase 需要一个字符串参数，或形如 {\"text\":\"...\"} 的对象",
            ),
        },
        other => RpcResponse::err(req.id, METHOD_NOT_FOUND, format!("method not found: {other}")),
    }
}

/// params 允许两种形态：直接是字符串 `"abc"`，或对象 `{"text":"abc"}`。
fn extract_text(params: &Value) -> Option<String> {
    params
        .as_str()
        .or_else(|| params.get("text").and_then(Value::as_str))
        .map(str::to_string)
}
