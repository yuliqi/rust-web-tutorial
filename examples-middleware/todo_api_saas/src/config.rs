//! 配置模块：所有环境变量集中在这里读一次，其余代码只依赖强类型的 `Config`。
//! 与 todo_api_pg 同构，SaaS 版多出三个配置项：REDIS_URL、JWT_SECRET、WEBHOOK_SECRET。
//!
//! 为什么密钥也走环境变量？十二要素应用（12-factor）的惯例：配置与代码分离，
//! 同一份镜像靠环境变量适配开发/测试/生产。但注意——环境变量只是「不进代码仓库」
//! 的底线，生产上密钥应交给密钥管理系统（Vault、AWS Secrets Manager、k8s Secret），
//! 由它注入并支持轮换。

use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub redis_url: String,
    /// JWT 签名密钥（HS256 对称密钥：签发方和校验方是同一个服务，对称足够）。
    /// ⚠️ 默认值仅供本地教学。生产必须换成高熵随机值并用密钥管理系统托管——
    /// 拿到这个密钥的人可以伪造任意用户、任意租户的 token，等于拿到整个系统。
    pub jwt_secret: String,
    /// 计费 webhook 的验签密钥（与支付商后台配置的那份共享密钥一致）。
    /// ⚠️ 同上：默认值仅供教学，泄露即可伪造「升级到 pro」之类的计费事件。
    pub webhook_secret: String,
}

impl Config {
    pub fn from_env() -> Self {
        let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
        // 端口错开：todo_api(3000)/todo_api_pg(3001)/redis_cache(3002)，SaaS 版用 3003，
        // 几个示例可以同时跑起来对照体验。
        let port = env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3003);
        // 与 todo_api_pg 连同一个物理库（docker-compose 的 postgres 服务），
        // 但表放在独立的 `saas` schema 里，互不踩踏——见 db.rs。
        let database_url = env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://tutorial:tutorial@localhost:5432/todos".into());
        let redis_url =
            env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/".into());
        let jwt_secret = env::var("JWT_SECRET").unwrap_or_else(|_| "dev-secret-change-me".into());
        let webhook_secret =
            env::var("WEBHOOK_SECRET").unwrap_or_else(|_| "dev-webhook-secret".into());
        Self {
            host,
            port,
            database_url,
            redis_url,
            jwt_secret,
            webhook_secret,
        }
    }

    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}
