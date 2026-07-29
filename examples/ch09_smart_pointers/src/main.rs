//! 第 9 章：智能指针与共享状态（含 LazyLock）
//!
//! 练习入口：`src/exercises.rs`（做题）→ `src/solutions.rs`（对照答案）
//! 只跑练习测试：`cargo test -p ch09_smart_pointers -- --ignored`

mod exercises;
mod solutions;

use std::sync::{Arc, LazyLock, Mutex};
use std::thread;

#[derive(Debug, Clone)]
struct Config {
    env: String,
}

#[derive(Clone)]
struct AppState {
    config: Arc<Config>,
    hits: Arc<Mutex<u64>>,
}

// 1.97 时代优先标准库 LazyLock，替代 lazy_static/once_cell 的常见场景
static APP_ENV: LazyLock<String> =
    LazyLock::new(|| std::env::var("APP_ENV").unwrap_or_else(|_| "dev".into()));

fn main() {
    println!("LazyLock APP_ENV={}", *APP_ENV);

    let state = AppState {
        config: Arc::new(Config {
            env: APP_ENV.clone(),
        }),
        hits: Arc::new(Mutex::new(0)),
    };

    let mut handles = Vec::new();
    for _ in 0..4 {
        let state = state.clone();
        handles.push(thread::spawn(move || {
            for _ in 0..100 {
                let mut guard = state.hits.lock().unwrap();
                *guard += 1;
            }
            assert_eq!(state.config.env, *APP_ENV);
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    println!(
        "env={}, hits={}",
        state.config.env,
        *state.hits.lock().unwrap()
    );

    #[derive(Debug)]
    #[allow(dead_code)]
enum List {
        Cons(i32, Box<List>),
        Nil,
    }
    let list = List::Cons(1, Box::new(List::Cons(2, Box::new(List::Nil))));
    println!("{list:?}");
}
