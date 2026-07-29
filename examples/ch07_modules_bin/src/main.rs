//! 第 7 章：二进制依赖库 crate

use ch07_modules_lib::{MemoryTodoRepo, Todo};

fn main() {
    let mut repo = MemoryTodoRepo::new();
    repo.insert(Todo::new(1, "split lib/bin"));
    repo.insert(Todo::new(2, "workspace rocks"));

    for todo in repo.list() {
        println!("#{id} {title}", id = todo.id, title = todo.title);
    }
}
