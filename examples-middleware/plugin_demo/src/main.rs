//! 第 29 章演示：一个 main 串起「应用商店安装」与「插件调用」两条主线，最后起 HTTP 服务。
//!
//! 运行：`cargo run -p plugin_demo`（默认监听 127.0.0.1:3009，全程无需外部服务）。
//!
//! 流程：
//!   一、应用商店：列目录 → 挑 postgres → 填参数 → 打印渲染出的 compose
//!       （提示：把它交给 `docker compose up -d` 就装好了）。
//!   二、插件机制：spawn 样例插件 → call ping / uppercase → 优雅关闭。
//!   三、起 axum 服务，把上面两套能力暴露成 HTTP 端点，方便 curl 体验。

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Context;
use plugin_demo::appstore;
use plugin_demo::config::Config;
use plugin_demo::plugin::{authorize, PluginHost, PluginManifest};
use plugin_demo::{app, AppState};
use serde_json::json;

/// 定位样例插件可执行文件。
///
/// 注意：`env!("CARGO_BIN_EXE_sample_plugin")` 这个便利只在**集成测试/bench**里由 cargo 注入
/// （见 tests/plugin.rs），主二进制构建时并没有。所以这里退而用 `current_exe()` 找同目录的兄弟文件——
/// cargo 会把同 crate 的多个 bin 输出到同一个 target 目录（如 target/debug/）。
fn sample_plugin_path() -> anyhow::Result<PathBuf> {
    let mut p = std::env::current_exe().context("拿不到 current_exe")?;
    p.pop(); // 去掉当前可执行文件名，留下所在目录
    let name = if cfg!(windows) {
        "sample_plugin.exe"
    } else {
        "sample_plugin"
    };
    p.push(name);
    Ok(p)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    // ===================== 一、应用商店 =====================
    println!("==== 一、应用商店（1Panel 式：填参数 → 渲染 compose 模板 → 起容器）====\n");
    for m in appstore::catalog() {
        println!("  - [{}] {} v{}：{}", m.id, m.name, m.version, m.description);
    }

    let manifest = appstore::find("postgres").expect("内置目录里有 postgres");
    println!("\n用户选择安装 [{}]，填入参数：", manifest.id);

    let mut values: HashMap<String, String> = HashMap::new();
    values.insert("password".into(), "S3cret!".into()); // 必填
    values.insert("port".into(), "6543".into()); // 覆盖默认 5432
    for (k, v) in &values {
        println!("    {k} = {v}");
    }
    println!("    （instance / db 用默认值）");

    // render 的错误类型是 AppError（面向 HTTP），在 main 里转成 anyhow 打印即可。
    let compose = appstore::render_compose(&manifest, &values)
        .map_err(|e| anyhow::anyhow!("渲染 compose 失败: {e:?}"))?;
    println!("\n渲染出的 docker-compose：\n");
    for line in compose.lines() {
        println!("    {line}");
    }
    println!(
        "\n>>> 一键安装的第一步（渲染）完成。真流程只需把上面这段交给 \
         `docker compose up -d`，容器就起来了。"
    );

    // ===================== 二、插件机制 =====================
    println!("\n==== 二、插件机制（子进程 + stdio JSON-RPC）====\n");

    let plugin_manifest = PluginManifest {
        name: "echo-tools".into(),
        version: "0.1.0".into(),
        entry: sample_plugin_path()?.to_string_lossy().into_owned(),
        capabilities: vec!["read_text".into(), "transform_text".into()],
    };

    // 宿主本次授予的能力。插件声明的能力必须是它的子集，否则 authorize 拒绝（最小权限）。
    let granted = ["read_text", "transform_text", "net"];
    authorize(&plugin_manifest, &granted).context("插件能力授权未通过")?;
    println!(
        "已授权插件 {}（能力：{:?}）",
        plugin_manifest.name, plugin_manifest.capabilities
    );

    let mut host = PluginHost::spawn(&plugin_manifest)
        .await
        .context("spawn 样例插件失败")?;
    println!("已 spawn 插件子进程，开始通信：");

    let pong = host.call("ping", json!(null)).await?;
    println!("  call ping        -> {pong}");

    let up = host.call("uppercase", json!("hello, plugin")).await?;
    println!("  call uppercase   -> {up}");

    // 演示插件返回的错误如何冒泡到宿主（未知方法）。
    match host.call("does_not_exist", json!(null)).await {
        Err(e) => println!("  call 未知方法    -> 如期报错：{e}"),
        Ok(v) => println!("  call 未知方法    -> 意外成功：{v}"),
    }

    host.shutdown().await?;
    println!("插件已优雅关闭（关 stdin → 插件读到 EOF 自行退出）。");

    // ===================== 三、起 HTTP 服务 =====================
    let config = Config::from_env();

    // 给 HTTP 服务再装一个插件实例（上面那个演示用的已经关了）。
    let state = AppState::default();
    state
        .plugins
        .install(plugin_manifest, &granted)
        .await
        .context("为 HTTP 服务安装插件失败")?;

    let addr = config.bind_addr();
    println!("\n==== 三、HTTP 服务 ====");
    println!("监听 http://{addr}，试试：");
    println!("  curl http://{addr}/apps");
    println!("  curl -X POST http://{addr}/apps/postgres/install \\");
    println!("       -H 'content-type: application/json' \\");
    println!("       -d '{{\"instance_name\":\"pg1\",\"values\":{{\"password\":\"x\"}}}}'");
    println!("  curl http://{addr}/plugins");
    println!("  curl -X POST http://{addr}/plugins/echo-tools/call \\");
    println!("       -H 'content-type: application/json' \\");
    println!("       -d '{{\"method\":\"uppercase\",\"params\":\"abc\"}}'");

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("绑定 {addr} 失败"))?;
    axum::serve(listener, app(state)).await?;
    Ok(())
}
