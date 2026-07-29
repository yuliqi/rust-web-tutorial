//! 第 6 章练习：把每个 `todo!()` 换成你的实现。
//!
//! 做题流程：
//! 1. 只跑练习测试（未完成时会失败，这是预期的）：
//!    `cargo test -p ch06_traits -- --ignored`
//! 2. 全部通过后，对照参考答案 `src/solutions.rs`（先自己做，别偷看）。
#![allow(dead_code)]
#![allow(unused_variables)]

use std::fmt;

/// 本章练习共用的 Todo 类型（结构与 `main.rs` 中的示例相同）。
#[derive(Debug, Clone, PartialEq)]
pub struct Todo {
    pub id: u64,
    pub title: String,
    pub done: bool,
}

/// 练习 1：为 `Todo` 实现 `Display`。
///
/// 格式要求：完成的显示为 `[x] #1 learn traits`，未完成的显示为 `[ ] #2 write tests`
/// （方括号内是 `x` 或空格，然后是 `#id`，再空格接标题）。
///
/// 提示：`Display` 是标准库定义的「如何面向用户展示」的 trait，
/// 实现它之后 `{}` 占位符和 `.to_string()` 就自动可用了——
/// 这是 trait 作为「能力接口」的典型例子。
/// 在 `fmt` 方法里用 `write!(f, "...")?` 或直接返回 `write!(...)` 的结果。
impl fmt::Display for Todo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("write!(f, \"[{{}}] #{{}} {{}}\", ...)")
    }
}

/// 练习 2：定义 `FromRow` 风格的 trait —— 从一行「数据库元组」构造出类型。
///
/// 这是 ORM/数据库驱动（如 sqlx 的 `FromRow`）的核心思路：
/// 用 trait 把「怎么从原始行数据构造出我」抽象出来，
/// 泛型代码就能对任何实现了它的类型统一处理。
pub trait FromRow {
    /// 从 `(id, title, done)` 元组构造 Self。
    fn from_row(row: (u64, String, bool)) -> Self;
}

/// 练习 2（续）：为 `Todo` 实现 `FromRow`。
///
/// 提示：`from_row` 没有 `self` 参数，是 trait 里的「关联函数」（类似构造器），
/// 调用方式是 `Todo::from_row(...)` 或泛型上下文里的 `T::from_row(...)`。
/// 可以用元组解构 `let (id, title, done) = row;` 让代码更清楚。
impl FromRow for Todo {
    fn from_row(row: (u64, String, bool)) -> Self {
        todo!()
    }
}

/// 练习 3：泛型分页函数 `paginate<T: Clone>`。
///
/// 返回第 `page` 页（从 1 开始）的元素，每页 `size` 个；
/// `page == 0` 或 `size == 0` 时返回空 Vec；越过末尾的页返回空 Vec。
///
/// 提示：为什么需要 `T: Clone` 这个 trait bound？
/// 因为入参是借用的切片 `&[T]`，而返回值 `Vec<T>` 需要拥有元素，
/// 只能通过克隆得到——没有这个 bound，编译器无法保证 T 可克隆。
/// 主体可用迭代器组合 `iter().skip(...).take(size).cloned().collect()`，
/// skip 过头就是空集合，「越过末尾」不用特判。但注意：`page == 0` 必须
/// 先单独处理——`page - 1` 对 usize 是下溢，debug 构建会直接 panic。
pub fn paginate<T: Clone>(items: &[T], page: usize, size: usize) -> Vec<T> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "练习：完成后运行 cargo test -p ch06_traits -- --ignored"]
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
    #[ignore = "练习：完成后运行 cargo test -p ch06_traits -- --ignored"]
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
    #[ignore = "练习：完成后运行 cargo test -p ch06_traits -- --ignored"]
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
