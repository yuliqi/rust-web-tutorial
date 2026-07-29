//! 第 9 章练习参考答案。先自己完成 `exercises.rs`，再来对照。
//!
//! 本文件的测试与练习测试断言完全相同，且默认随 `cargo test` 运行，
//! 保证参考答案本身始终是对的。
#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::thread;

/// 练习 1 参考答案：`Rc` 的计数不是原子的、没实现 `Send`，
/// 跨线程共享所有权必须用 `Arc`；`Mutex` 保证同一时刻只有一个线程改计数。
/// 「Arc 管共享、Mutex 管互斥」各司其职，是共享可变状态跨线程的标准组合。
/// 守卫（MutexGuard）离开作用域自动解锁，所以循环体里不需要手动 unlock。
pub fn parallel_count(threads: usize, per_thread: u64) -> u64 {
    let counter = Arc::new(Mutex::new(0u64));
    let mut handles = Vec::new();
    for _ in 0..threads {
        let counter = Arc::clone(&counter);
        handles.push(thread::spawn(move || {
            for _ in 0..per_thread {
                *counter.lock().unwrap() += 1;
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    *counter.lock().unwrap()
}

/// 练习 2 用的递归链表（与练习一致）：`Box` 提供一层堆上间接，
/// 递归类型的大小才能在编译期确定。
pub enum List {
    Cons(i32, Box<List>),
    Nil,
}

/// 练习 2 参考答案：对引用做 match，`head` 是 `&i32`、`tail` 是 `&Box<List>`。
/// `list_sum(tail)` 能直接编译，靠的是 deref 强制转换：
/// `&Box<List>` 经由 `Box: Deref<Target = List>` 自动变成 `&List`。
pub fn list_sum(list: &List) -> i32 {
    match list {
        List::Cons(head, tail) => head + list_sum(tail),
        List::Nil => 0,
    }
}

/// 练习 3 用的只读配置（与练习一致）。
#[derive(Debug)]
pub struct Config {
    pub env: String,
}

/// 练习 3 参考答案的 AppState（字段与练习完全相同）。
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub cache: Rc<RefCell<HashMap<String, String>>>,
}

impl AppState {
    /// 只读数据装 `Arc` 就够（不需要锁）；可写缓存加一层 `RefCell`
    /// 换来内部可变性。`Clone` 派生后克隆的只是指针，状态天然共享。
    pub fn new(env: &str) -> Self {
        AppState {
            config: Arc::new(Config {
                env: env.to_string(),
            }),
            cache: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    /// `&self` 就能写，正是 `RefCell` 的价值：把「同一时刻只有一个可变借用」
    /// 的检查从编译期挪到运行期，违反时 panic 而不是编译错误。
    pub fn cache_put(&self, key: &str, value: &str) {
        self.cache
            .borrow_mut()
            .insert(key.to_string(), value.to_string());
    }

    /// `borrow()` 的守卫在函数返回时释放，不能把 `&String` 带出去，
    /// 所以用 `.cloned()` 返回拥有的 `Option<String>`。
    pub fn cache_get(&self, key: &str) -> Option<String> {
        self.cache.borrow().get(key).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ex1_parallel_count() {
        assert_eq!(parallel_count(4, 100), 400);
        assert_eq!(parallel_count(1, 0), 0);
        assert_eq!(parallel_count(8, 250), 2000);
    }

    #[test]
    fn ex2_list_sum() {
        let list = List::Cons(
            1,
            Box::new(List::Cons(2, Box::new(List::Cons(3, Box::new(List::Nil))))),
        );
        assert_eq!(list_sum(&list), 6);
        assert_eq!(list_sum(&List::Nil), 0);
    }

    #[test]
    fn ex3_app_state() {
        let state = AppState::new("dev");
        let cloned = state.clone();

        // 克隆体写入，原始体能读到：说明缓存是共享的同一份
        cloned.cache_put("token", "abc123");
        assert_eq!(state.cache_get("token"), Some("abc123".to_string()));
        assert_eq!(state.cache_get("missing"), None);

        // 配置也是共享的同一份
        assert_eq!(state.config.env, "dev");
        assert_eq!(cloned.config.env, "dev");
        assert_eq!(Arc::strong_count(&state.config), 2);
    }
}
