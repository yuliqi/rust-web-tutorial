//! 第 6 章：泛型与 Trait（含 use<> precise capturing）
//!
//! 练习入口：`src/exercises.rs`（做题）→ `src/solutions.rs`（对照答案）
//! 只跑练习测试：`cargo test -p ch06_traits -- --ignored`

mod exercises;
mod solutions;

use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone)]
struct Todo {
    id: u64,
    title: String,
    done: bool,
}

impl fmt::Display for Todo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mark = if self.done { "x" } else { " " };
        write!(f, "[{mark}] #{} {}", self.id, self.title)
    }
}

trait TodoRepository {
    fn get(&self, id: u64) -> Option<Todo>;
    fn upsert(&mut self, todo: Todo);
}

struct MemoryTodoRepo {
    data: HashMap<u64, Todo>,
}

impl TodoRepository for MemoryTodoRepo {
    fn get(&self, id: u64) -> Option<Todo> {
        self.data.get(&id).cloned()
    }

    fn upsert(&mut self, todo: Todo) {
        self.data.insert(todo.id, todo);
    }
}

fn first<T>(items: &[T]) -> Option<&T> {
    items.first()
}

fn paginate<T: Clone>(items: &[T], page: usize, size: usize) -> Vec<T> {
    if page == 0 || size == 0 {
        return vec![];
    }
    items
        .iter()
        .skip((page - 1) * size)
        .take(size)
        .cloned()
        .collect()
}

/// Edition 2024 / precise capturing：明确只捕获 'a
fn open_titles<'a>(items: &'a [String]) -> impl Iterator<Item = &'a str> + use<'a> {
    items.iter().map(|s| s.as_str())
}

fn use_repo_static<R: TodoRepository>(repo: &R, id: u64) {
    match repo.get(id) {
        Some(t) => println!("static dispatch: {t}"),
        None => println!("static dispatch: missing"),
    }
}

fn use_repo_dynamic(repo: &dyn TodoRepository, id: u64) {
    match repo.get(id) {
        Some(t) => println!("dynamic dispatch: {t}"),
        None => println!("dynamic dispatch: missing"),
    }
}

fn main() {
    let mut repo = MemoryTodoRepo {
        data: HashMap::new(),
    };
    repo.upsert(Todo {
        id: 1,
        title: "trait object demo".into(),
        done: false,
    });

    use_repo_static(&repo, 1);
    use_repo_dynamic(&repo, 1);

    let nums = [10, 20, 30, 40];
    println!("first={:?}", first(&nums));
    println!("page={:?}", paginate(&nums, 2, 2));

    let titles = vec!["alpha".into(), "beta".into()];
    let joined: Vec<_> = open_titles(&titles).collect();
    println!("open_titles={joined:?}");
}
