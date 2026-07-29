//! 配置模块：环境变量集中读一次，其余代码只依赖强类型 `Config`（与其他示例同构）。

use std::env;

use crate::pty::PtyConfig;

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    /// 与其他示例连同一个物理库（docker-compose 的 postgres 服务），
    /// 但表放在独立的 `bastion` schema 里，互不踩踏——见 session.rs。
    pub database_url: String,
    /// 演示用的访问令牌（Bearer token）。
    /// ⚠️ 这是**极度简化**的鉴权，仅供教学：真实堡垒机必须用第 17 章的
    /// JWT + 密码哈希 + RBAC（见 todo_api_saas/src/auth.rs），再叠加第 23 章
    /// 的授权（谁能连哪台目标机）。Web shell 是能直接在服务器上执行任意命令的
    /// 能力，鉴权一旦失守等于把服务器拱手让人。默认值仅本地教学可用。
    pub demo_token: String,
    /// PTY 里要 spawn 的命令。默认是本机登录 shell，让演示页能真的敲命令。
    /// ⚠️ 真实堡垒机这里**不是**本地 shell，而是「ssh 到被授权的目标机」，
    /// 并叠加命令黑白名单——见 pty.rs 的安全注释。
    pub pty: PtyConfig,
}

impl Config {
    pub fn from_env() -> Self {
        let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
        // 端口错开其他示例：realtime_demo 用 3005，本示例用 3006，可同时跑起来对照。
        let port = env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3006);
        let database_url = env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://tutorial:tutorial@localhost:5432/todos".into());
        let demo_token = env::var("WEBTERM_TOKEN").unwrap_or_else(|_| "dev-terminal-token".into());
        // WEBTERM_SHELL 可覆盖默认 shell（如设成 "/bin/bash"）。
        let pty = match env::var("WEBTERM_SHELL") {
            Ok(program) if !program.is_empty() => PtyConfig::program(program, Vec::new()),
            _ => PtyConfig::login_shell(),
        };
        Self {
            host,
            port,
            database_url,
            demo_token,
            pty,
        }
    }

    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}
