//! API 报文加密示例（第 19 章配套）：TLS 之上的应用层混合加密。
//!
//! 威胁模型一句话：TLS 常在网关/代理/WAF 终止，之后的 HTTP 明文会进
//! 访问日志与抓包审计；本示例让中间节点只能看到密文信封。
//! 方案与边界的完整讨论见 [`envelope`] 模块头注释——先读那里。
//!
//! 数据流：浏览器(Web Crypto 加密) → HTTP(密文信封) → routes(拆信封/封信封)
//!         → 业务逻辑只碰明文 JSON，对加密无感知。
//! 组装放 lib 而不是 main，集成测试(tests/api.rs)才能直接拿 `app()` 在内存中发请求。

pub mod envelope;
pub mod routes;

use std::sync::Arc;

use axum::Router;
use rsa::{RsaPrivateKey, RsaPublicKey};
use tower_http::trace::TraceLayer;

/// 教学用固定密钥对，编译期嵌入。
/// !!! 仅限教学：生产环境私钥必须从密钥管理系统(KMS/Vault)或环境变量加载，
/// 绝不能进代码库；公钥可以公开（它本来就是要下发给所有客户端的）。
const DEV_PRIVATE_PEM: &str = include_str!("../keys/dev_private.pem");

/// 全应用共享状态。
/// RsaPrivateKey 解析一次后放进 Arc 共享——RSA 私钥解析（PEM→大整数）不便宜，
/// 不能每个请求做一遍；spki_der 同理，启动时算好缓存。
#[derive(Clone)]
pub struct AppState {
    /// 服务端 RSA 私钥：唯一能解开请求信封里 `ek` 字段的东西。
    pub private_key: Arc<RsaPrivateKey>,
    /// 公钥的 SPKI DER 字节，`GET /crypto/public-key` 直接下发它的 base64，
    /// 前端 Web Crypto `importKey("spki", ...)` 原样导入。
    pub spki_der: Arc<Vec<u8>>,
}

impl AppState {
    /// 从内嵌的教学密钥构建状态。密钥损坏属于「程序装错了」而非运行时错误，
    /// 所以直接 panic 快速失败，而不是把 Result 层层往外传。
    pub fn from_dev_keys() -> Self {
        let private_key =
            envelope::load_private_key(DEV_PRIVATE_PEM).expect("内嵌教学私钥应当总能解析");
        // 公钥可以从私钥推导出来，不必再解析一遍 public pem——两个 PEM 文件
        // 里公钥部分本来就是同一份数据（keys/dev_public.pem 供前端/文档参照）。
        let public_key = RsaPublicKey::from(&private_key);
        let spki_der =
            envelope::public_key_spki_der(&public_key).expect("公钥转 SPKI DER 不应失败");
        Self {
            private_key: Arc::new(private_key),
            spki_der: Arc::new(spki_der),
        }
    }
}

/// 路由、中间件、状态拼成完整 Router。
/// 顺序有讲究：`.layer()` 只作用于在它之前注册的路由，所以 merge 在前、layer 在后。
pub fn app(state: AppState) -> Router {
    Router::new()
        .merge(routes::router())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
