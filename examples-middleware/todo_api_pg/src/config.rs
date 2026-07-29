//! 配置模块：与 SQLite 版（examples/todo_api）完全同构——启动时读一次环境变量，
//! 其余代码只依赖强类型的 `Config`。变的只是两个默认值，见下。

use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database_url: String,
}

impl Config {
    pub fn from_env() -> Self {
        let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
        // 差异点：SQLite 版默认 3000，这里改用 3001——
        // 两个示例可能同时跑起来对照体验，端口错开才不会互相抢占。
        let port = env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3001);
        // 差异点：SQLite 版默认 `sqlite:todo.db`（本地文件，零依赖）；
        // Postgres 是网络服务，URL 里带上用户/密码/主机/端口/库名，
        // 默认值与 ../docker-compose.yml 的 postgres 服务一一对应。
        let database_url = env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://tutorial:tutorial@localhost:5432/todos".into());
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
