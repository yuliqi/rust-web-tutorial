//! 配置模块：把「从环境变量读配置」集中到一处，只在 main 启动时调用一次。
//! 好处：其余代码只依赖强类型的 `Config`，不用到处 `env::var`，也方便部署时覆盖。

use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database_url: String,
}

impl Config {
    /// 缺省值策略：本地开发零配置即可跑起来；生产环境用环境变量覆盖。
    /// 注意 PORT 解析失败时静默回退到 3000，教学项目从简；
    /// 生产代码通常会选择报错退出，避免配置写错却没被发现。
    pub fn from_env() -> Self {
        let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
        let port = env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3000);
        let database_url =
            env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:todo.db".into());
        Self {
            host,
            port,
            database_url,
        }
    }

    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}
