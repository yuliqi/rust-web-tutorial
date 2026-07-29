//! 第 5 章：集合与迭代器
//!
//! 练习入口：`src/exercises.rs`（做题）→ `src/solutions.rs`（对照答案）
//! 只跑练习测试：`cargo test -p ch05_collections -- --ignored`

mod exercises;
mod solutions;

use std::collections::HashMap;

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct Todo {
    id: u64,
    title: String,
    done: bool,
}

fn filter_todos(items: &[Todo], q: Option<&str>, done: Option<bool>) -> Vec<Todo> {
    items
        .iter()
        .filter(|t| done.is_none_or(|d| t.done == d))
        .filter(|t| q.is_none_or(|query| t.title.contains(query)))
        .cloned()
        .collect()
}

fn count_status_codes(lines: &[&str]) -> HashMap<u16, usize> {
    let mut map = HashMap::new();
    for line in lines {
        // 简化：取最后一个 token 作为状态码
        if let Some(code) = line.split_whitespace().last().and_then(|s| s.parse().ok()) {
            *map.entry(code).or_insert(0) += 1;
        }
    }
    map
}

fn paginate<T: Clone>(items: &[T], page: usize, page_size: usize) -> Vec<T> {
    if page == 0 || page_size == 0 {
        return Vec::new();
    }
    let start = (page - 1) * page_size;
    items.iter().skip(start).take(page_size).cloned().collect()
}

fn main() {
    let todos = vec![
        Todo {
            id: 1,
            title: "learn vec".into(),
            done: true,
        },
        Todo {
            id: 2,
            title: "learn hashmap".into(),
            done: false,
        },
        Todo {
            id: 3,
            title: "build api".into(),
            done: false,
        },
    ];

    let open = filter_todos(&todos, Some("learn"), Some(false));
    println!("filtered = {open:?}");
    // is_some_and: 可选查询参数非空才启用
    let q = Some("api");
    println!("q active = {}", q.is_some_and(|s| !s.is_empty()));

    let logs = [
        "GET /health 200",
        "POST /todos 201",
        "GET /todos/9 404",
        "GET /health 200",
    ];
    println!("status counts = {:?}", count_status_codes(&logs));

    let page1 = paginate(&todos, 1, 2);
    println!("page1 = {page1:?}");

    let titles: Vec<_> = todos
        .iter()
        .map(|t| t.title.to_uppercase())
        .filter(|t| t.len() > 8)
        .collect();
    println!("titles = {titles:?}");
}
