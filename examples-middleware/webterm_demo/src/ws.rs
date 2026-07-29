//! 终端 WebSocket：把浏览器和服务端 PTY 用一条全双工连接桥起来，全程录制 + 审计。
//!
//! ## 数据流（一条连接的一生）
//!
//! ```text
//! 浏览器 ──input──▶ WS ──▶ 记录 input ──▶ 写 PTY ──▶ 子进程 stdin
//! 浏览器 ◀─output── WS ◀── 记录 output ◀── 读 PTY ◀── 子进程 stdout
//! ```
//!
//! 鉴权（[`AuthUser`]）通过后才升级为 WebSocket；升级后：
//! 1. `start_session` 落一条审计记录（谁、从哪、连哪台、何时开始）；
//! 2. 双向桥接，**每一段 I/O 都 `record_event` 落库**（这就是录制）；
//! 3. 连接断开 / PTY 退出 → `end_session` 收尾。
//!
//! ## 协议：数据帧 vs 控制帧必须分开
//!
//! 终端连接上跑的不只是「屏幕字节」，还有「窗口变大了」这类**控制信息**。若混在一起，
//! 服务端没法区分「用户输入的字节」和「调整尺寸的指令」。所以用 JSON 包一层，靠
//! `type` 字段区分（`input`/`resize`/`output`）——这正是 xterm.js 等生产终端前端
//! 与后端约定协议的通行做法（真实项目里 output 常用二进制帧 + base64 以无损承载
//! 控制序列，本示例为教学清晰用文本）。

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

use crate::auth::AuthUser;
use crate::pty::{PtyBridge, DEFAULT_COLS, DEFAULT_ROWS};
use crate::session;
use crate::AppState;

/// 浏览器 → 服务端 的消息。`#[serde(tag = "type")]` 让 JSON 里的 `type` 字段决定变体。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ClientMsg {
    /// 用户输入的按键（文本）。
    Input { data: String },
    /// 窗口尺寸变化。
    Resize { rows: u16, cols: u16 },
}

/// 服务端 → 浏览器 的消息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ServerMsg {
    /// 一段终端输出。
    Output { data: String },
    /// 出错提示（如无法解析的客户端消息）。
    Error { message: String },
}

impl ServerMsg {
    fn into_text(self) -> String {
        serde_json::to_string(&self).unwrap_or_else(|_| "{}".to_string())
    }
}

/// `GET /terminal/ws`：先过 [`AuthUser`] 鉴权（红线：终端必须鉴权），再升级 WebSocket。
///
/// `ConnectInfo` 拿客户端 IP 写进审计——「从哪连的」是审计的关键字段。它要求服务用
/// `into_make_service_with_connect_info::<SocketAddr>()` 启动（见 main.rs）。
pub async fn terminal_ws(
    ws: WebSocketUpgrade,
    user: AuthUser,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state, user.account_id, peer.ip().to_string()))
}

/// 处理一条已升级的终端连接。见文件头「数据流」。
async fn handle_socket(socket: WebSocket, state: AppState, account_id: i64, client_ip: String) {
    let (mut sender, mut receiver) = socket.split();

    // 1) 开 PTY。失败就告知前端并结束——绝不静默。
    let mut pty = match PtyBridge::spawn(&state.config.pty, DEFAULT_ROWS, DEFAULT_COLS) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = %e, "开 PTY 失败");
            let _ = sender
                .send(Message::Text(
                    ServerMsg::Error {
                        message: "failed to open terminal".into(),
                    }
                    .into_text()
                    .into(),
                ))
                .await;
            return;
        }
    };

    // 2) 起一条审计会话。target 记录「连到了什么」——本示例是本地命令，真实堡垒机是目标机地址。
    let target = state.config.pty.program.clone();
    let session_id = match session::start_session(&state.pool, account_id, &target, &client_ip).await
    {
        Ok(id) => id,
        Err(e) => {
            // 审计开不了，就不该放行会话——「无法审计的访问」本身违背堡垒机的存在意义。
            tracing::error!(error = %e, "开审计会话失败，拒绝终端连接");
            let _ = sender
                .send(Message::Text(
                    ServerMsg::Error {
                        message: "audit unavailable, connection refused".into(),
                    }
                    .into_text()
                    .into(),
                ))
                .await;
            pty.shutdown();
            return;
        }
    };
    tracing::info!(account_id, %client_ip, %target, session_id, "终端会话开始");

    // 3) 双向桥接。
    loop {
        tokio::select! {
            // —— 下行：PTY 输出 → 记录 output → 推浏览器 ——
            out = pty.next_output() => {
                match out {
                    Some(bytes) => {
                        // 先录后发：审计优先，确保「屏幕上出现过的」一定进了录像。
                        if let Err(e) = session::record_event(
                            &state.pool, session_id, session::Direction::Output,
                            &bytes, session::now_ms(),
                        ).await {
                            tracing::error!(error = %e, session_id, "记录 output 失败");
                        }
                        let text = String::from_utf8_lossy(&bytes).into_owned();
                        if sender.send(Message::Text(
                            ServerMsg::Output { data: text }.into_text().into(),
                        )).await.is_err() {
                            break; // 浏览器已断开
                        }
                    }
                    None => break, // PTY 结束（子进程退出）
                }
            }

            // —— 上行：浏览器消息 ——
            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<ClientMsg>(&text) {
                            Ok(ClientMsg::Input { data }) => {
                                // 输入也必须录（谁敲了什么危险命令，见 session.rs）。
                                if let Err(e) = session::record_event(
                                    &state.pool, session_id, session::Direction::Input,
                                    data.as_bytes(), session::now_ms(),
                                ).await {
                                    tracing::error!(error = %e, session_id, "记录 input 失败");
                                }
                                pty.write_input(data.into_bytes());
                            }
                            Ok(ClientMsg::Resize { rows, cols }) => {
                                pty.resize(rows, cols); // 尺寸是控制信息，不进录像
                            }
                            Err(_) => {
                                // 不可解析的消息：提示但不断开（对客户端 bug 宽容）。
                                let _ = sender.send(Message::Text(
                                    ServerMsg::Error { message: "bad message".into() }
                                        .into_text().into(),
                                )).await;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) => break, // 浏览器优雅关闭
                    Some(Ok(_)) => {}                     // Ping/Pong/Binary：本示例不特别处理
                    Some(Err(_)) => break,                // 连接出错
                    None => break,                        // 流结束 = 断开
                }
            }
        }
    }

    // 4) 收尾：杀 PTY 子进程 + 结束审计会话（补 ended_at）。同生共死，不留僵尸。
    pty.shutdown();
    if let Err(e) = session::end_session(&state.pool, session_id).await {
        tracing::error!(error = %e, session_id, "结束审计会话失败");
    }
    let _ = sender.close().await;
    tracing::info!(session_id, "终端会话结束");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_input_message() {
        let msg: ClientMsg = serde_json::from_str(r#"{"type":"input","data":"ls\n"}"#).unwrap();
        assert_eq!(msg, ClientMsg::Input { data: "ls\n".to_string() });
    }

    #[test]
    fn parse_resize_message() {
        let msg: ClientMsg =
            serde_json::from_str(r#"{"type":"resize","rows":40,"cols":120}"#).unwrap();
        assert_eq!(msg, ClientMsg::Resize { rows: 40, cols: 120 });
    }

    #[test]
    fn reject_unknown_message_type() {
        // 未知 type 必须解析失败（走 ws 里的 bad message 分支），而不是被当成某种默认。
        assert!(serde_json::from_str::<ClientMsg>(r#"{"type":"exec","data":"x"}"#).is_err());
    }

    #[test]
    fn server_output_serializes_with_type_tag() {
        let text = ServerMsg::Output { data: "hi".into() }.into_text();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["type"], "output");
        assert_eq!(v["data"], "hi");
    }
}
