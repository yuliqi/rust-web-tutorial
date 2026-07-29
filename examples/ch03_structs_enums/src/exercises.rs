//! 第 3 章练习：把每个 `todo!()` 换成你的实现。
//!
//! 做题流程：
//! 1. 只跑练习测试（未完成时会失败，这是预期的）：
//!    `cargo test -p ch03_structs_enums -- --ignored`
//! 2. 全部通过后，对照参考答案 `src/solutions.rs`（先自己做，别偷看）。
#![allow(dead_code)]
#![allow(unused_variables)]

/// 任务状态：比 `main.rs` 的 `TodoStatus` 多一个 `Cancelled`
/// （对应本章练习 1「扩展枚举」）。已给出，无需修改。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Doing,
    Done,
    Cancelled,
}

/// 练习用的任务结构体。已给出，无需修改。
#[derive(Debug, Clone)]
pub struct Task {
    pub id: u64,
    pub title: String,
    pub status: TaskStatus,
}

impl Task {
    pub fn new(id: u64, title: impl Into<String>) -> Self {
        Self {
            id,
            title: title.into(),
            status: TaskStatus::Pending,
        }
    }
}

/// 练习 1：把字符串解析成 `TaskStatus`。
///
/// - "pending" / "doing" / "done" / "cancelled" → 对应的 `Some(状态)`
/// - 其他任何输入 → `None`
///
/// 提示：`&str` 可以直接 `match` 字符串字面量，记得兜底分支 `_ => None`。
/// 返回 `Option` 而不是 panic，是因为「输入不合法」是正常业务情况，
/// 应该用类型把它交给调用方处理，而不是让程序崩溃。
pub fn parse_status(raw: &str) -> Option<TaskStatus> {
    todo!()
}

impl TaskStatus {
    /// 练习 2：状态机 transition（本章练习 2 的扩展版，多了 `Cancelled`）。
    ///
    /// 规则：
    /// - 转到自身总是允许（返回 `Ok`）
    /// - `Done` 和 `Cancelled` 是终态，不允许再转到其他状态
    /// - 其余转换全部允许
    ///
    /// 失败时返回 `Err(String)`，错误信息里带上两个状态（用 `{:?}` 格式化）。
    ///
    /// 提示：用 `match (self, next)` 元组匹配把规则写成显式分支；
    /// 「转到自身」可以用带守卫的分支 `(s, t) if s == t => ...` 放在最前面。
    /// 想一想：如果最后用 `_` 兜底放行，穷尽性检查还能在新增状态时提醒你吗？
    pub fn transition(self, next: TaskStatus) -> Result<TaskStatus, String> {
        todo!()
    }
}

/// 练习 3：在任务列表中按 id 查找并返回标题（内存版 Repo::get 的核心逻辑）。
///
/// 提示：`tasks.iter().find(|t| ...)` 返回 `Option<&Task>`，
/// 再用 `.map(|t| t.title.as_str())` 变成 `Option<&str>`。
/// 用 Option 组合子串联，比手写 for + return 更简洁，
/// 「找不到」的 `None` 会沿着组合子自动传递，不需要单独写分支。
pub fn find_task_title(tasks: &[Task], id: u64) -> Option<&str> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "练习：完成后运行 cargo test -p ch03_structs_enums -- --ignored"]
    fn ex1_parse_status() {
        assert_eq!(parse_status("pending"), Some(TaskStatus::Pending));
        assert_eq!(parse_status("doing"), Some(TaskStatus::Doing));
        assert_eq!(parse_status("done"), Some(TaskStatus::Done));
        assert_eq!(parse_status("cancelled"), Some(TaskStatus::Cancelled));
        assert_eq!(parse_status("unknown"), None);
    }

    #[test]
    #[ignore = "练习：完成后运行 cargo test -p ch03_structs_enums -- --ignored"]
    fn ex2_transition() {
        use TaskStatus::*;
        assert_eq!(Pending.transition(Doing), Ok(Doing));
        assert_eq!(Doing.transition(Done), Ok(Done));
        assert_eq!(Pending.transition(Cancelled), Ok(Cancelled));
        assert_eq!(Done.transition(Done), Ok(Done));
        assert!(Done.transition(Pending).is_err());
        assert!(Cancelled.transition(Doing).is_err());
    }

    #[test]
    #[ignore = "练习：完成后运行 cargo test -p ch03_structs_enums -- --ignored"]
    fn ex3_find_task_title() {
        let tasks = vec![Task::new(1, "learn structs"), Task::new(2, "learn enums")];
        assert_eq!(find_task_title(&tasks, 2), Some("learn enums"));
        assert_eq!(find_task_title(&tasks, 99), None);
    }
}
