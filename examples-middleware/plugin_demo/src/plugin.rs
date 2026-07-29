//! 插件机制：让第三方扩展面板本身。
//!
//! # 三种主流方案对比
//!
//! 让别人的代码跑进你的软件里，Rust 生态（其实各语言都类似）主要有三条路：
//!
//! 1. **子进程 + stdio（本示例）**：把插件编译成独立可执行文件，宿主 spawn 它，
//!    通过它的 stdin/stdout 收发消息（这里用 JSON-RPC）。
//!    - 优点：**语言无关**（插件用任何语言写，只要会读写 stdio）；**崩溃隔离**
//!      （插件段错误只死它自己的进程，宿主不受影响）；**易沙箱**（可套 seccomp/命名空间/容器限权）。
//!    - 缺点：每次调用要走 IPC（序列化 + 进程间管道），有开销；要管理子进程生命周期。
//!
//! 2. **WASM（如 wasmtime）**：插件编译成 WebAssembly，宿主在**进程内**用运行时执行。
//!    - 优点：默认沙箱（线性内存隔离、能力需显式授予）、in-process 调用快、跨平台字节码。
//!    - 缺点：插件得能编译成 wasm（部分 crate/系统调用受限）、宿主要内嵌 wasmtime 增大体积。
//!      本 crate 特意**不引 wasmtime**，把它留作进阶备选——若要沙箱又要低延迟，它是首选。
//!
//! 3. **动态库 .so/.dll/.dylib（dlopen + C ABI）**：把插件编成动态库，宿主运行时加载、按符号调用。
//!    - 优点：**最快**（就是普通函数调用，无 IPC、无序列化）。
//!    - 缺点：**ABI 脆弱**（Rust 无稳定 ABI，得走 `extern "C"` 手写 FFI 边界，版本不匹配直接 UB）；
//!      **不安全**（插件与宿主同地址空间，能读写宿主内存）；**崩溃拖垮宿主**（插件 panic/段错误 = 宿主一起挂）。
//!
//! 面板类软件要装的是「别人写的、不完全可信的」插件，隔离性与安全性优先级高于极致延迟，
//! 所以本示例选**子进程 + stdio**。样例插件见本 crate 的第二个 bin：`src/bin/sample_plugin.rs`。
//!
//! # 协议
//!
//! 极简 JSON-RPC，一行一条消息（NDJSON）：
//! - 宿主 → 插件 stdin：`{"id":1,"method":"ping","params":null}`
//! - 插件 → 宿主 stdout：`{"id":1,"result":"pong"}` 或 `{"id":1,"error":{"code":-32601,"message":"..."}}`
//!
//! 用「一行一条」是因为管道是字节流、没有天然的消息边界，靠换行切分最简单可靠。

use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

/// JSON-RPC 请求。`id` 用来把响应和请求配对（本示例串行调用，但带上 id 是好习惯）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    pub id: u64,
    pub method: String,
    /// 方法参数，形态由各 method 自定。缺省为 null。
    #[serde(default)]
    pub params: Value,
}

/// JSON-RPC 响应：result 与 error 二选一。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl RpcResponse {
    pub fn ok(id: u64, result: Value) -> Self {
        Self {
            id,
            result: Some(result),
            error: None,
        }
    }
    pub fn err(id: u64, code: i64, message: impl Into<String>) -> Self {
        Self {
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

/// 插件清单：宿主据此启动并授权插件。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    /// 插件可执行文件路径（子进程方案里就是要 spawn 的程序）。
    pub entry: String,
    /// 插件声明它需要的权限（能力）。宿主据此授权，遵循**最小权限**原则：
    /// 插件没声明的能力，宿主一律不给；即便声明了，宿主也可拒绝授予（见 [`authorize`]）。
    pub capabilities: Vec<String>,
}

/// 宿主认得的全部能力。真实系统会更细（如 `fs_read:/data`、`net:api.example.com`）。
pub const KNOWN_CAPABILITIES: &[&str] = &["read_text", "transform_text", "net", "fs_read"];

/// 授权检查：在 spawn 插件之前跑。
/// - 插件声明了宿主不认识的能力 → 拒绝（`UnknownCapability`），防止「偷偷夹带」。
/// - 插件声明的能力不在宿主本次授予的 `granted` 集合里 → 拒绝（`CapabilityDenied`）。
///
/// 这就是最小权限落地：宿主明确列出「这次允许什么」，插件只能在交集内活动。
pub fn authorize(manifest: &PluginManifest, granted: &[&str]) -> Result<(), PluginError> {
    for cap in &manifest.capabilities {
        if !KNOWN_CAPABILITIES.contains(&cap.as_str()) {
            return Err(PluginError::UnknownCapability(cap.clone()));
        }
        if !granted.contains(&cap.as_str()) {
            return Err(PluginError::CapabilityDenied(cap.clone()));
        }
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("无法启动插件进程: {0}")]
    Spawn(String),
    #[error("插件进程 I/O 失败: {0}")]
    Io(#[from] std::io::Error),
    #[error("插件已关闭 stdout（进程可能已退出）")]
    Closed,
    #[error("协议错误: {0}")]
    Protocol(String),
    #[error("插件返回错误 [{code}]: {message}")]
    Rpc { code: i64, message: String },
    #[error("JSON 编解码失败: {0}")]
    Codec(#[from] serde_json::Error),
    #[error("能力未授权：插件申请 {0}，但宿主未授予")]
    CapabilityDenied(String),
    #[error("未知能力: {0}")]
    UnknownCapability(String),
}

/// 一个已启动的插件进程 + 与它通信的句柄。
///
/// 持有子进程、它的 stdin（宿主写请求）、它的 stdout（宿主读响应，包一层 BufReader 按行读）。
/// stderr 继承宿主终端，插件的日志/panic 直接打到屏幕，方便教学观察。
pub struct PluginHost {
    pub name: String,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    /// 自增的请求 id。
    next_id: u64,
}

impl PluginHost {
    /// 按清单启动插件进程，接好 stdin/stdout 管道。
    pub async fn spawn(manifest: &PluginManifest) -> Result<Self, PluginError> {
        let mut child = Command::new(&manifest.entry)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // stderr 继承：插件的日志与崩溃信息直达宿主终端。
            .stderr(Stdio::inherit())
            // kill_on_drop：宿主意外 drop 掉 PluginHost 时也别留下僵尸子进程。
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| PluginError::Spawn(format!("{}（entry={}）", e, manifest.entry)))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| PluginError::Spawn("拿不到子进程 stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| PluginError::Spawn("拿不到子进程 stdout".into()))?;

        Ok(Self {
            name: manifest.name.clone(),
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
        })
    }

    /// 调用插件的一个方法：写一行请求，读一行响应，配对 id，拆开 result/error。
    pub async fn call(&mut self, method: &str, params: Value) -> Result<Value, PluginError> {
        let id = self.next_id;
        self.next_id += 1;

        // 写请求（NDJSON：一条消息一行）。
        let req = RpcRequest {
            id,
            method: method.to_string(),
            params,
        };
        let mut line = serde_json::to_string(&req)?;
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.flush().await?;

        // 读一行响应。读到 0 字节 = 对端关了 stdout（插件退出/崩溃）。
        let mut resp_line = String::new();
        let n = self.stdout.read_line(&mut resp_line).await?;
        if n == 0 {
            return Err(PluginError::Closed);
        }

        let resp: RpcResponse = serde_json::from_str(resp_line.trim())?;
        if resp.id != id {
            return Err(PluginError::Protocol(format!(
                "响应 id 不匹配：期望 {id}，收到 {}",
                resp.id
            )));
        }
        if let Some(err) = resp.error {
            return Err(PluginError::Rpc {
                code: err.code,
                message: err.message,
            });
        }
        resp.result
            .ok_or_else(|| PluginError::Protocol("响应既无 result 也无 error".into()))
    }

    /// 优雅关闭：先 drop 掉 stdin 让插件读到 EOF 自行退出，再等它收尾；
    /// 超时（默认 2 秒）还没退就强杀——插件耍赖也不能拖住宿主。
    pub async fn shutdown(self) -> Result<(), PluginError> {
        let PluginHost {
            mut child,
            stdin,
            stdout,
            ..
        } = self;
        drop(stdin); // 关闭写端 → 插件 read 到 EOF → 主循环退出
        drop(stdout);
        match tokio::time::timeout(Duration::from_secs(2), child.wait()).await {
            Ok(Ok(_status)) => Ok(()),
            Ok(Err(e)) => Err(PluginError::Io(e)),
            Err(_) => {
                // 崩溃/挂死隔离：超时强杀。
                let _ = child.start_kill();
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_serializes_flat() {
        let req = RpcRequest {
            id: 7,
            method: "uppercase".into(),
            params: json!("abc"),
        };
        let v: Value = serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
        assert_eq!(v["id"], 7);
        assert_eq!(v["method"], "uppercase");
        assert_eq!(v["params"], "abc");
    }

    #[test]
    fn response_ok_omits_error_field() {
        let v: Value =
            serde_json::from_str(&serde_json::to_string(&RpcResponse::ok(1, json!("pong"))).unwrap())
                .unwrap();
        assert_eq!(v["result"], "pong");
        assert!(v.get("error").is_none(), "result 响应不应带 error 字段");
    }

    #[test]
    fn response_err_omits_result_field() {
        let v: Value = serde_json::from_str(
            &serde_json::to_string(&RpcResponse::err(2, -32601, "method not found")).unwrap(),
        )
        .unwrap();
        assert_eq!(v["error"]["code"], -32601);
        assert!(v.get("result").is_none(), "error 响应不应带 result 字段");
    }

    #[test]
    fn request_defaults_params_to_null_when_absent() {
        let req: RpcRequest = serde_json::from_str(r#"{"id":3,"method":"ping"}"#).unwrap();
        assert_eq!(req.params, Value::Null);
    }

    fn manifest(caps: &[&str]) -> PluginManifest {
        PluginManifest {
            name: "p".into(),
            version: "0".into(),
            entry: "/bin/true".into(),
            capabilities: caps.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn authorize_passes_when_all_caps_granted() {
        let m = manifest(&["read_text", "transform_text"]);
        assert!(authorize(&m, &["read_text", "transform_text", "net"]).is_ok());
    }

    #[test]
    fn authorize_denies_ungranted_cap() {
        let m = manifest(&["net"]);
        let err = authorize(&m, &["read_text"]).unwrap_err();
        assert!(matches!(err, PluginError::CapabilityDenied(c) if c == "net"));
    }

    #[test]
    fn authorize_rejects_unknown_cap() {
        let m = manifest(&["mine_bitcoin"]);
        let err = authorize(&m, &["mine_bitcoin"]).unwrap_err();
        assert!(matches!(err, PluginError::UnknownCapability(c) if c == "mine_bitcoin"));
    }
}
