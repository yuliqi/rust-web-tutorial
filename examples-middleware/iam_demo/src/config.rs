//! 配置：与前面章节同构，只读一次环境变量，其余代码依赖强类型 Config。
//! 连的是 docker-compose 同一个 Postgres，但表放独立的 `iam` schema（见 store.rs）。

use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    /// JWT 签名密钥（HS256）。⚠️ 默认值仅供本地教学，生产必须换高熵随机值并交
    /// 密钥管理系统托管——拿到它就能伪造任意账号的 token。
    pub jwt_secret: String,
}

impl Config {
    pub fn from_env() -> Self {
        let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
        // 端口继续错开：SaaS 版用到 3003，本示例用 3004，可与其它示例并存。
        let port = env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3004);
        let database_url = env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://tutorial:tutorial@localhost:5432/todos".into());
        let jwt_secret = env::var("JWT_SECRET").unwrap_or_else(|_| "dev-secret-change-me".into());
        Self {
            host,
            port,
            database_url,
            jwt_secret,
        }
    }

    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}
