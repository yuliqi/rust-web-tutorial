//! 集成测试：真正起一个 axum 服务，用真实客户端验证「服务端推的消息能到达」。
//!
//! 本 crate 不依赖任何外部服务（无 Postgres/Redis），所以这些测试**默认就跑**，
//! 无需 `docker compose`。跑法：`cargo test -p realtime_demo`。
//!
//! - WebSocket 用例：用 tokio-tungstenite 当客户端连 `/ws`，触发一次广播，
//!   断言客户端收到那条推送。
//! - SSE 用例：用 reqwest 读 `/sse` 的事件流，同样验证广播能到达。

use std::net::SocketAddr;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use realtime_demo::app;
use realtime_demo::hub::Hub;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

/// 在随机可用端口上起服务，返回其地址。测试各自独立起服务、互不干扰。
async fn spawn_server() -> SocketAddr {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = app(Hub::new());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    // 给服务一瞬间进入 accept 循环。
    tokio::time::sleep(Duration::from_millis(50)).await;
    addr
}

/// WebSocket：客户端连上后，经 HTTP `/publish` 触发一条广播，客户端应收到它。
#[tokio::test]
async fn ws_client_receives_broadcast() {
    let addr = spawn_server().await;

    // 1) 连上 /ws。
    let url = format!("ws://{addr}/ws");
    let (mut socket, _resp) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("WebSocket 握手应成功");

    // 2) 订阅生效需要一点点时间（服务端 handler 里 subscribe 后才收得到）。
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 3) 经 HTTP 端点触发一次广播。
    reqwest::get(format!("http://{addr}/publish?msg=hello-ws"))
        .await
        .expect("/publish 请求应成功");

    // 4) 客户端应在超时内收到那条推送。
    let msg = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Text(t))) => return t.to_string(),
                Some(Ok(_)) => continue, // 跳过 Ping 等控制帧
                other => panic!("连接意外结束：{other:?}"),
            }
        }
    })
    .await
    .expect("应在超时内收到广播");

    let v: serde_json::Value = serde_json::from_str(&msg).expect("推送应是合法 JSON");
    assert_eq!(v["kind"], "publish");
    assert_eq!(v["payload"]["text"], "hello-ws");
}

/// WebSocket：客户端上行的消息会被广播回所有连接（含它自己），验证双向 + 广播。
#[tokio::test]
async fn ws_client_message_is_broadcast_back() {
    let addr = spawn_server().await;
    let url = format!("ws://{addr}/ws");
    let (mut socket, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    socket
        .send(Message::Text(r#"{"text":"ping-from-client"}"#.into()))
        .await
        .expect("上行发送应成功");

    let msg = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Text(t))) => return t.to_string(),
                Some(Ok(_)) => continue,
                other => panic!("连接意外结束：{other:?}"),
            }
        }
    })
    .await
    .expect("应收到回灌的广播");

    let v: serde_json::Value = serde_json::from_str(&msg).unwrap();
    assert_eq!(v["kind"], "ws");
    assert_eq!(v["payload"]["text"], "ping-from-client");
}

/// SSE：读 `/sse` 事件流，触发广播后应能在流里读到那条 data。
#[tokio::test]
async fn sse_client_receives_broadcast() {
    let addr = spawn_server().await;

    // 用 reqwest 打开 SSE 流（就是一条不结束的 HTTP 响应）。
    let resp = reqwest::get(format!("http://{addr}/sse"))
        .await
        .expect("/sse 应可连接");
    assert!(
        resp.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .starts_with("text/event-stream"),
        "SSE 响应应是 text/event-stream"
    );

    let mut stream = resp.bytes_stream();

    // 触发一次广播。
    reqwest::get(format!("http://{addr}/publish?msg=hello-sse"))
        .await
        .expect("/publish 应成功");

    // 从字节流里累积，直到看到我们发的内容。
    let found = tokio::time::timeout(Duration::from_secs(5), async {
        let mut buf = String::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.expect("读流不应出错");
            buf.push_str(&String::from_utf8_lossy(&chunk));
            if buf.contains("hello-sse") {
                return true;
            }
        }
        false
    })
    .await
    .expect("应在超时内读到事件");

    assert!(found, "SSE 流里应出现广播内容 hello-sse");
}
