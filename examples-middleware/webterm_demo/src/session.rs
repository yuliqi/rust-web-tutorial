//! 会话录制与访问审计——**堡垒机的灵魂**。
//!
//! 堡垒机存在的根本理由不是「能连服务器」（SSH 客户端也能），而是「所有访问都留痕、
//! 可审计、可回放、可追责」。本模块把一条终端会话拆成两部分持久化到 Postgres：
//!
//! - `terminal_sessions`：一条会话的元数据——**谁**（account_id）在**什么时候**
//!   （started_at/ended_at）从**哪**（client_ip）连了**哪台目标机**（target）。这是
//!   访问审计（access log），回答「有哪些访问发生过」。
//! - `session_events`：会话里每一段带时间戳的 I/O（input/output）。这是**会话录制**
//!   （session recording），把整条 PTY 字节流按时序存下来，事后能**逐帧回放**，
//!   看到当时屏幕上发生的一切。格式思路接近 asciinema 的 .cast：每条事件 = (相对/绝对
//!   时间, 方向, 数据)，回放时按时间顺序重放即可。
//!
//! ## 为什么连输入也要录
//!
//! 只录输出（屏幕内容）不够：追责要知道**是谁敲了那条危险命令**（`rm -rf`、
//! `DROP TABLE`）。输入录像 + 账号 + 时间三者合一，才能在事故后精确定位到人和动作。
//! 这也是合规（等保、SOC2、审计）的硬性要求，和安全取证的第一手证据。
//!
//! ## ⚠️ 生产必做的两件本示例从简的事
//!
//! 1. **敏感信息脱敏**：用户敲密码、`export TOKEN=...` 时，明文会进输入录像。生产要
//!    对录像做脱敏（掩码 password 提示后的输入、过滤已知密钥模式），否则录像本身成了
//!    最大的密码泄露源。
//! 2. **录像加密与防篡改**：审计证据必须防止事后被删改（WORM 存储、加密、哈希链）。
//!    否则内部人可以先作恶再抹掉自己的录像。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::time::{SystemTime, UNIX_EPOCH};

/// I/O 方向。input = 用户敲进去的、output = 服务器吐出来的。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Input,
    Output,
}

impl Direction {
    /// 存库用的稳定字符串（DB 里 direction 列是 TEXT）。
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Input => "input",
            Direction::Output => "output",
        }
    }

    /// 从库里读回的文本解析回枚举；无法识别返回 None（库里存了坏数据时不 panic）。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "input" => Some(Direction::Input),
            "output" => Some(Direction::Output),
            _ => None,
        }
    }
}

/// 当前 unix 毫秒时间戳。事件用绝对时间存，回放时相邻事件之差就是「该等多久」。
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 回放用的一条事件。data 用 lossy 文本呈现，方便直接进 JSON 给前端重放；
/// 底层 BYTEA 存的是原始字节（控制序列、颜色码都在），呈现层才转文本。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionEvent {
    pub direction: Direction,
    pub at_ms: i64,
    pub data: String,
}

/// 纯逻辑：把事件按时间排序，用于回放。稳定排序保证同一毫秒内的事件保持写入顺序。
///
/// 抽出来单独测：回放**顺序错了就是灾难**（把先后颠倒的操作重放出来会误导审计），
/// 而顺序是纯逻辑，不该依赖数据库才能验证。生产里 SQL 的 ORDER BY 已保证顺序，
/// 这个函数是那份保证的可离线单测的镜像。
pub fn sort_events(mut events: Vec<SessionEvent>) -> Vec<SessionEvent> {
    events.sort_by_key(|e| e.at_ms);
    events
}

/// 一条会话的审计摘要（用于 GET /sessions 列表）。时间戳在 SQL 里 `::text` 转成
/// 字符串返回，避免为 TIMESTAMPTZ 引入额外的 sqlx 时间类型 feature——教学从简。
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct SessionSummary {
    pub id: i64,
    pub account_id: i64,
    pub target: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub client_ip: String,
}

/// 建 schema 与表。锁 id 用 48（与 todo_api_pg 的 42、saas 的 43 错开，共库不互斥）。
/// 用事务级咨询锁把 `CREATE ... IF NOT EXISTS` 串行化，理由见 todo_api_pg/src/db.rs。
pub async fn migrate(pool: &PgPool) -> Result<()> {
    let mut tx = pool.begin().await.context("begin migrate tx")?;
    sqlx::query("SELECT pg_advisory_xact_lock(48)")
        .execute(&mut *tx)
        .await
        .context("acquire migrate lock")?;

    sqlx::query("CREATE SCHEMA IF NOT EXISTS bastion")
        .execute(&mut *tx)
        .await
        .context("create bastion schema")?;

    // 会话元数据（访问审计）。ended_at 可空：会话进行中时为 NULL，结束时补上。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS bastion.terminal_sessions (
            id         BIGSERIAL PRIMARY KEY,
            account_id BIGINT NOT NULL,
            target     TEXT NOT NULL,
            started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            ended_at   TIMESTAMPTZ,
            client_ip  TEXT NOT NULL
        )
        "#,
    )
    .execute(&mut *tx)
    .await
    .context("migrate terminal_sessions table")?;

    // 会话事件（会话录制）。direction: input/output；data 存原始字节（BYTEA）；
    // at_ms 是绝对毫秒时间戳，回放按它排序。
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS bastion.session_events (
            id         BIGSERIAL PRIMARY KEY,
            session_id BIGINT NOT NULL REFERENCES bastion.terminal_sessions(id),
            direction  TEXT NOT NULL,
            data       BYTEA NOT NULL,
            at_ms      BIGINT NOT NULL
        )
        "#,
    )
    .execute(&mut *tx)
    .await
    .context("migrate session_events table")?;

    // 回放/审计都按 session_id 取事件，这条索引让「取某会话的全部事件」不必全表扫描。
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_bastion_events_session \
         ON bastion.session_events (session_id, at_ms, id)",
    )
    .execute(&mut *tx)
    .await
    .context("create session_events index")?;

    tx.commit().await.context("commit migrate tx")?;
    Ok(())
}

/// 开一条会话，返回会话 id。这一行落库就是审计的起点：从此刻起这个账号的所有 I/O
/// 都会挂到这个 id 下。
pub async fn start_session(
    pool: &PgPool,
    account_id: i64,
    target: &str,
    client_ip: &str,
) -> Result<i64> {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO bastion.terminal_sessions (account_id, target, client_ip) \
         VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(account_id)
    .bind(target)
    .bind(client_ip)
    .fetch_one(pool)
    .await
    .context("start session")?;
    Ok(id)
}

/// 记录一段 I/O。这是「录制」的最小单元：每次 PTY 吐出输出、或用户敲入输入，
/// 都调一次，带上方向和时间戳。
///
/// 注意：教学里这是**逐段同步落库**，写在 ws 的热路径上。生产要考虑吞吐——
/// 高频小写会压垮数据库，通常先在内存/本地缓冲聚合，再批量/异步落库或落对象存储。
pub async fn record_event(
    pool: &PgPool,
    session_id: i64,
    direction: Direction,
    data: &[u8],
    at_ms: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO bastion.session_events (session_id, direction, data, at_ms) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(session_id)
    .bind(direction.as_str())
    .bind(data)
    .bind(at_ms)
    .execute(pool)
    .await
    .context("record session event")?;
    Ok(())
}

/// 结束会话：补上 ended_at。只更新还没结束的那条（ended_at IS NULL），幂等。
pub async fn end_session(pool: &PgPool, session_id: i64) -> Result<()> {
    sqlx::query(
        "UPDATE bastion.terminal_sessions SET ended_at = now() \
         WHERE id = $1 AND ended_at IS NULL",
    )
    .bind(session_id)
    .execute(pool)
    .await
    .context("end session")?;
    Ok(())
}

/// 列出会话（审计视图）。最近的排前面。
/// ⚠️ 生产里这个视图只有审计员/管理员能看（RBAC）——审计日志本身是敏感数据。
pub async fn list_sessions(pool: &PgPool) -> Result<Vec<SessionSummary>> {
    let rows = sqlx::query_as::<_, SessionSummary>(
        "SELECT id, account_id, target, \
                started_at::text AS started_at, \
                ended_at::text   AS ended_at, \
                client_ip \
         FROM bastion.terminal_sessions \
         ORDER BY id DESC \
         LIMIT 200",
    )
    .fetch_all(pool)
    .await
    .context("list sessions")?;
    Ok(rows)
}

/// 取一条会话的全部事件用于回放，按时间顺序。SQL 里已 ORDER BY，
/// 但仍过一遍 [`sort_events`]（防御性 + 与那份可测逻辑对齐）。
pub async fn replay(pool: &PgPool, session_id: i64) -> Result<Vec<SessionEvent>> {
    let rows: Vec<(String, Vec<u8>, i64)> = sqlx::query_as(
        "SELECT direction, data, at_ms FROM bastion.session_events \
         WHERE session_id = $1 ORDER BY at_ms ASC, id ASC",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await
    .context("replay session")?;

    let events = rows
        .into_iter()
        .filter_map(|(dir, data, at_ms)| {
            Direction::parse(&dir).map(|direction| SessionEvent {
                direction,
                at_ms,
                data: String::from_utf8_lossy(&data).into_owned(),
            })
        })
        .collect();
    Ok(sort_events(events))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_str_roundtrip() {
        assert_eq!(Direction::parse(Direction::Input.as_str()), Some(Direction::Input));
        assert_eq!(Direction::parse(Direction::Output.as_str()), Some(Direction::Output));
        assert_eq!(Direction::parse("garbage"), None);
    }

    #[test]
    fn direction_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&Direction::Input).unwrap(), "\"input\"");
        assert_eq!(serde_json::to_string(&Direction::Output).unwrap(), "\"output\"");
    }

    #[test]
    fn replay_sorts_by_time() {
        // 故意乱序构造，模拟从多来源汇聚、时间戳交错的情形。
        let ev = |at_ms, d: Direction, s: &str| SessionEvent {
            direction: d,
            at_ms,
            data: s.to_string(),
        };
        let unsorted = vec![
            ev(30, Direction::Output, "third"),
            ev(10, Direction::Input, "first"),
            ev(20, Direction::Output, "second"),
        ];
        let sorted = sort_events(unsorted);
        let order: Vec<&str> = sorted.iter().map(|e| e.data.as_str()).collect();
        assert_eq!(order, ["first", "second", "third"]);
    }

    #[test]
    fn sort_is_stable_within_same_timestamp() {
        // 同一毫秒的多条事件必须保持写入顺序（稳定排序），否则回放会错乱。
        let ev = |data: &str| SessionEvent {
            direction: Direction::Output,
            at_ms: 100,
            data: data.to_string(),
        };
        let input = vec![ev("a"), ev("b"), ev("c")];
        let sorted = sort_events(input);
        let order: Vec<&str> = sorted.iter().map(|e| e.data.as_str()).collect();
        assert_eq!(order, ["a", "b", "c"]);
    }
}
