//! 集成测试：真 spawn 样例插件子进程，走真 stdio JSON-RPC 通信。默认运行（无外部依赖）。
//!
//! 关键便利：`env!("CARGO_BIN_EXE_sample_plugin")`。cargo 在编译**集成测试**时会把本 crate
//! 每个 bin 的最终可执行文件路径注入成 `CARGO_BIN_EXE_<bin名>` 环境变量，`env!` 在编译期读到它。
//! 于是测试能精确拿到「刚编出来的那个 sample_plugin」的路径，不必猜 target 目录、不必手拼路径。
//! （注意这个变量只在 test/bench 里有；普通 bin 构建时没有——所以 main.rs 改用 current_exe 找兄弟文件。）
//!
//! 若某些 CI/沙箱环境无法 spawn 子进程导致这里不稳，可给这些用例加 `#[ignore]` 并注明原因；
//! 目前子进程通信在常见环境稳定，故默认开启。

use plugin_demo::plugin::{PluginError, PluginHost, PluginManifest};
use serde_json::json;

fn manifest() -> PluginManifest {
    PluginManifest {
        name: "sample".into(),
        version: "0.1.0".into(),
        entry: env!("CARGO_BIN_EXE_sample_plugin").to_string(),
        capabilities: vec!["read_text".into(), "transform_text".into()],
    }
}

#[tokio::test]
async fn ping_returns_pong() {
    let mut host = PluginHost::spawn(&manifest()).await.expect("spawn 插件");
    let r = host.call("ping", json!(null)).await.expect("call ping");
    assert_eq!(r, json!("pong"));
    host.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn uppercase_transforms_text() {
    let mut host = PluginHost::spawn(&manifest()).await.expect("spawn 插件");
    let r = host.call("uppercase", json!("abc")).await.expect("call");
    assert_eq!(r, json!("ABC"));

    // 复用同一个进程连续调用，验证 id 自增与逐条配对都正确。
    let r2 = host.call("uppercase", json!("MixEd")).await.expect("call");
    assert_eq!(r2, json!("MIXED"));

    host.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn unknown_method_returns_rpc_error() {
    let mut host = PluginHost::spawn(&manifest()).await.expect("spawn 插件");
    let err = host.call("no_such_method", json!(null)).await.unwrap_err();
    match err {
        // 样例插件用 JSON-RPC 标准的 -32601（method not found）。
        PluginError::Rpc { code, message } => {
            assert_eq!(code, -32601);
            assert!(message.contains("no_such_method"), "message = {message}");
        }
        other => panic!("期望 Rpc 错误，得到 {other:?}"),
    }
    host.shutdown().await.expect("shutdown");
}
