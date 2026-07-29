//! 第 9 章练习：把每个 `todo!()` 换成你的实现。
//!
//! 做题流程：
//! 1. 只跑练习测试（未完成时会失败，这是预期的）：
//!    `cargo test -p ch09_smart_pointers -- --ignored`
//! 2. 全部通过后，对照参考答案 `src/solutions.rs`（先自己做，别偷看）。
#![allow(dead_code)]
#![allow(unused_variables)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

/// 练习 1：用 `Arc<Mutex<u64>>` 实现多线程计数。
///
/// 启动 `threads` 个线程，每个线程把共享计数器加 `per_thread` 次（每次 +1），
/// 等所有线程结束后返回最终计数值（应为 `threads * per_thread`）。
///
/// 提示：为什么是 `Arc` 而不是 `Rc`？`Rc` 的引用计数不是原子操作，
/// 没有实现 `Send`，编译器会直接拒绝它跨线程。
/// `Mutex` 提供互斥的内部可变性：`counter.lock().unwrap()` 拿到守卫后
/// 才能修改，守卫离开作用域自动解锁。
/// 每个线程 `let c = Arc::clone(&counter);` 再配合 `move` 闭包传入
/// `std::thread::spawn`，最后逐个 `handle.join().unwrap()` 等待。
pub fn parallel_count(threads: usize, per_thread: u64) -> u64 {
    todo!("Arc::new(Mutex::new(0)) + thread::spawn + join")
}

/// 练习 2 用的递归链表：`Box` 让递归类型有确定大小。
///
/// 没有 `Box` 时 `Cons(i32, List)` 的大小是「无限」的，编译器无法布局；
/// `Box` 把尾部放到堆上，栈上只存一个指针，大小就确定了。
pub enum List {
    Cons(i32, Box<List>),
    Nil,
}

/// 练习 2：对链表所有元素求和。
///
/// 提示：对 `&List` 做 `match`，`List::Cons(head, tail)` 分支里
/// `head + list_sum(tail)` 递归下去，`List::Nil` 返回 0。
/// 注意 `tail` 是 `&Box<List>`，传给 `list_sum(&List)` 时
/// deref 强制转换（`Box` 实现了 `Deref`）会自动帮你解一层，直接传即可。
pub fn list_sum(list: &List) -> i32 {
    todo!()
}

/// 练习 3 用的只读配置。
#[derive(Debug)]
pub struct Config {
    pub env: String,
}

/// 练习 3：设计 `AppState` —— 只读配置 + 可写缓存。
///
/// 字段已经定好：
/// - `config`：进程内共享的只读配置，用 `Arc` 只共享不复制；
/// - `cache`：单线程场景下的可写缓存，`Rc<RefCell<...>>` 提供
///   「共享所有权 + 内部可变性」（多线程版会换成 `Arc<RwLock<...>>`，见 main.rs）。
///
/// 关键点：`AppState` 派生了 `Clone`，克隆的只是两个智能指针（引用计数 +1），
/// 所有克隆体看到的是同一份配置和同一个缓存。
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub cache: Rc<RefCell<HashMap<String, String>>>,
}

impl AppState {
    /// 构造：把 `env` 装进 `Arc<Config>`，缓存初始化为空 HashMap。
    ///
    /// 提示：`Rc::new(RefCell::new(HashMap::new()))` 三层由外到内分别提供
    /// 「共享所有权 → 可变借用检查 → 键值存储」。
    pub fn new(env: &str) -> Self {
        todo!()
    }

    /// 写缓存：注意 `&self` 而不是 `&mut self` —— 这就是内部可变性的意义：
    /// 借用检查从编译期移到运行期（`RefCell::borrow_mut`），
    /// 共享引用也能改内部数据。
    pub fn cache_put(&self, key: &str, value: &str) {
        todo!()
    }

    /// 读缓存：`RefCell::borrow` 拿到不可变借用；
    /// 返回 `Option<String>` 需要 `.get(key).cloned()` 把 `&String` 变成拥有的值，
    /// 因为借用守卫在函数返回时就释放了，不能把内部引用带出去。
    pub fn cache_get(&self, key: &str) -> Option<String> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "练习：完成后运行 cargo test -p ch09_smart_pointers -- --ignored"]
    fn ex1_parallel_count() {
        assert_eq!(parallel_count(4, 100), 400);
        assert_eq!(parallel_count(1, 0), 0);
        assert_eq!(parallel_count(8, 250), 2000);
    }

    #[test]
    #[ignore = "练习：完成后运行 cargo test -p ch09_smart_pointers -- --ignored"]
    fn ex2_list_sum() {
        let list = List::Cons(
            1,
            Box::new(List::Cons(2, Box::new(List::Cons(3, Box::new(List::Nil))))),
        );
        assert_eq!(list_sum(&list), 6);
        assert_eq!(list_sum(&List::Nil), 0);
    }

    #[test]
    #[ignore = "练习：完成后运行 cargo test -p ch09_smart_pointers -- --ignored"]
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
