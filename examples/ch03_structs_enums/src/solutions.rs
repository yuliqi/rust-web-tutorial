//! 第 3 章练习参考答案。先自己完成 `exercises.rs`，再来对照。
//!
//! 本文件的测试与练习测试断言完全相同，且默认随 `cargo test` 运行，
//! 保证参考答案本身始终是对的。
#![allow(dead_code)]

/// 任务状态（与 `exercises.rs` 相同，含 `Cancelled`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Doing,
    Done,
    Cancelled,
}

/// 练习用的任务结构体（与 `exercises.rs` 相同）。
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

/// 练习 1 参考答案：`&str` 直接 match 字符串字面量，兜底 `_ => None`。
/// 用 `Option` 表达「可能解析失败」，调用方被类型强制处理 None 分支，
/// 比 panic 或返回哨兵值安全得多。
pub fn parse_status(raw: &str) -> Option<TaskStatus> {
    match raw {
        "pending" => Some(TaskStatus::Pending),
        "doing" => Some(TaskStatus::Doing),
        "done" => Some(TaskStatus::Done),
        "cancelled" => Some(TaskStatus::Cancelled),
        _ => None,
    }
}

impl TaskStatus {
    /// 练习 2 参考答案：元组匹配 + 守卫。分支顺序很关键——
    /// `(s, t) if s == t` 放最前面让「原地不动」总是合法；
    /// 终态用 `(Done, _) | (Cancelled, _)` 一次拦截；其余交给 `_` 放行。
    /// 注意取舍：`_` 兜底让代码短，但也放弃了穷尽性保护——将来新增
    /// 状态时编译器不会提醒你补规则。规则复杂后应逐组合显式列出。
    pub fn transition(self, next: TaskStatus) -> Result<TaskStatus, String> {
        use TaskStatus::*;
        match (self, next) {
            (s, t) if s == t => Ok(t),
            (Done, _) | (Cancelled, _) => {
                Err(format!("invalid transition: {self:?} -> {next:?}"))
            }
            _ => Ok(next),
        }
    }
}

/// 练习 3 参考答案：`find` 定位 + `map` 转换，两个 Option 组合子串联。
/// 找不到时 `find` 返回 `None`，`map` 原样传递——不用写任何 if/else。
/// 返回 `&str` 借用元素的 title，避免 clone。
pub fn find_task_title(tasks: &[Task], id: u64) -> Option<&str> {
    tasks.iter().find(|t| t.id == id).map(|t| t.title.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ex1_parse_status() {
        assert_eq!(parse_status("pending"), Some(TaskStatus::Pending));
        assert_eq!(parse_status("doing"), Some(TaskStatus::Doing));
        assert_eq!(parse_status("done"), Some(TaskStatus::Done));
        assert_eq!(parse_status("cancelled"), Some(TaskStatus::Cancelled));
        assert_eq!(parse_status("unknown"), None);
    }

    #[test]
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
    fn ex3_find_task_title() {
        let tasks = vec![Task::new(1, "learn structs"), Task::new(2, "learn enums")];
        assert_eq!(find_task_title(&tasks, 2), Some("learn enums"));
        assert_eq!(find_task_title(&tasks, 99), None);
    }
}
