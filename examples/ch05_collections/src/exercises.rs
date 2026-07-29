//! 第 5 章练习：把每个 `todo!()` 换成你的实现。
//!
//! 做题流程：
//! 1. 只跑练习测试（未完成时会失败，这是预期的）：
//!    `cargo test -p ch05_collections -- --ignored`
//! 2. 全部通过后，对照参考答案 `src/solutions.rs`（先自己做，别偷看）。
#![allow(dead_code)]
#![allow(unused_variables)]

use std::collections::HashMap;

/// 练习 1：词频统计。
///
/// 按空白分词，统计每个词出现的次数（区分大小写，不做其他清洗）。
/// 例如 `"hello world hello"` → `{"hello": 2, "world": 1}`。
///
/// 提示：`text.split_whitespace()` 返回一个惰性迭代器；
/// 往 HashMap 里累加计数的惯用写法是 entry API：
/// `*map.entry(key).or_insert(0) += 1` —— 一次哈希查找就完成
/// 「有则加一，无则插入 0 再加一」，比先 `contains_key` 再 `insert` 更高效。
pub fn word_freq(text: &str) -> HashMap<String, usize> {
    todo!("split_whitespace + entry API")
}

/// 练习 2：top_k 词频。
///
/// 输入词频表，返回出现次数最多的前 k 个 `(词, 次数)`；
/// 次数相同时按词典序升序排（这样结果是确定的，方便测试）。
/// k 超过总词数时返回全部；k 为 0 时返回空 Vec。
///
/// 提示：HashMap 的迭代顺序是不确定的，所以必须先收集成
/// `Vec<(String, usize)>` 再排序。「次数降序、词升序」的两级排序
/// 可以用 `sort_by` + `Ordering::then` 一次表达（想想比较时 a、b
/// 各放在哪一侧才是降序）；最后 `truncate(k)` 截断即可。
pub fn top_k(freq: &HashMap<String, usize>, k: usize) -> Vec<(String, usize)> {
    todo!()
}

/// 练习 3：用迭代器组合器改写循环。
///
/// 求切片中所有偶数的平方和。等价的 for 循环版本：
/// ```text
/// let mut sum = 0;
/// for n in nums { if n % 2 == 0 { sum += n * n; } }
/// ```
///
/// 提示：用 `iter().filter(..).map(..).sum()` 一条链搞定。
/// `filter`/`map` 是惰性适配器，只有 `sum()`（消费适配器）才真正驱动计算；
/// 编译后与手写循环性能相同 —— 这就是迭代器的「零成本抽象」。
/// `iter()` 产出 `&i64`，可以先 `.copied()` 转成 `i64` 少写解引用。
pub fn sum_even_squares(nums: &[i64]) -> i64 {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "练习：完成后运行 cargo test -p ch05_collections -- --ignored"]
    fn ex1_word_freq() {
        let freq = word_freq("hello world hello");
        assert_eq!(freq.len(), 2);
        assert_eq!(freq.get("hello"), Some(&2));
        assert_eq!(freq.get("world"), Some(&1));
        assert!(word_freq("").is_empty());
    }

    #[test]
    #[ignore = "练习：完成后运行 cargo test -p ch05_collections -- --ignored"]
    fn ex2_top_k() {
        let freq = HashMap::from([
            ("rust".to_string(), 3),
            ("web".to_string(), 2),
            ("api".to_string(), 2),
            ("fun".to_string(), 1),
        ]);
        assert_eq!(
            top_k(&freq, 2),
            vec![("rust".to_string(), 3), ("api".to_string(), 2)]
        );
        assert_eq!(top_k(&freq, 0), vec![]);
        assert_eq!(top_k(&freq, 10).len(), 4);
    }

    #[test]
    #[ignore = "练习：完成后运行 cargo test -p ch05_collections -- --ignored"]
    fn ex3_sum_even_squares() {
        assert_eq!(sum_even_squares(&[1, 2, 3, 4]), 20);
        assert_eq!(sum_even_squares(&[]), 0);
        assert_eq!(sum_even_squares(&[-2, 3]), 4);
    }
}
