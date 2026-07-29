//! 第 6 章练习参考答案。先自己完成 `exercises.rs`，再来对照。
//!
//! 本文件的测试与练习测试断言完全相同，且默认随 `cargo test` 运行，
//! 保证参考答案本身始终是对的。
#![allow(dead_code)]

use std::fmt;

/// 与练习中相同的 Todo 类型（solutions 模块独立定义一份）。
#[derive(Debug, Clone, PartialEq)]
pub struct Todo {
    pub id: u64,
    pub title: String,
    pub done: bool,
}

/// 练习 1 参考答案：实现 `Display` 后，`{}` 与 `.to_string()` 自动可用
/// （标准库对所有 `T: Display` 提供了 `ToString` 的覆盖实现）。
/// 用一个 if 表达式先算出标记字符，`write!` 一次写完，避免多次调用。
impl fmt::Display for Todo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mark = if self.done { "x" } else { " " };
        write!(f, "[{mark}] #{} {}", self.id, self.title)
    }
}

/// 练习 2 参考答案的 trait 定义（与练习一致）。
pub trait FromRow {
    /// 从 `(id, title, done)` 元组构造 Self。
    fn from_row(row: (u64, String, bool)) -> Self;
}

/// 练习 2 参考答案：`from_row` 是无 `self` 的关联函数，扮演「构造器」角色。
/// 用元组解构一次拿到三个字段，比 `row.0`/`row.1` 的下标访问更可读；
/// 这正是 sqlx 等库 `FromRow` 派生宏帮你生成的那类代码。
impl FromRow for Todo {
    fn from_row(row: (u64, String, bool)) -> Self {
        let (id, title, done) = row;
        Todo { id, title, done }
    }
}

/// 练习 3 参考答案：`T: Clone` 是必须的——入参只是借用 `&[T]`，
/// 返回 `Vec<T>` 需要拥有元素，只能克隆。
/// `skip` + `take` 的组合天然容忍越界（skip 过头得到空迭代器），
/// 所以只需要显式处理 `page == 0` / `size == 0` 这两个退化输入。
pub fn paginate<T: Clone>(items: &[T], page: usize, size: usize) -> Vec<T> {
    if page == 0 || size == 0 {
        return Vec::new();
    }
    items
        .iter()
        .skip((page - 1) * size)
        .take(size)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ex1_display_for_todo() {
        let done = Todo {
            id: 1,
            title: "learn traits".into(),
            done: true,
        };
        let open = Todo {
            id: 2,
            title: "write tests".into(),
            done: false,
        };
        assert_eq!(done.to_string(), "[x] #1 learn traits");
        assert_eq!(open.to_string(), "[ ] #2 write tests");
    }

    #[test]
    fn ex2_from_row() {
        let todo = Todo::from_row((7, "from db".to_string(), false));
        assert_eq!(
            todo,
            Todo {
                id: 7,
                title: "from db".to_string(),
                done: false,
            }
        );
    }

    #[test]
    fn ex3_paginate() {
        let nums = [1, 2, 3, 4, 5];
        assert_eq!(paginate(&nums, 1, 2), vec![1, 2]);
        assert_eq!(paginate(&nums, 2, 2), vec![3, 4]);
        assert_eq!(paginate(&nums, 3, 2), vec![5]);
        assert_eq!(paginate(&nums, 4, 2), Vec::<i32>::new());
        assert_eq!(paginate(&nums, 0, 2), Vec::<i32>::new());
        assert_eq!(paginate(&nums, 1, 0), Vec::<i32>::new());
    }
}
