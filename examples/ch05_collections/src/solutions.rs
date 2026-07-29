//! 第 5 章练习参考答案。先自己完成 `exercises.rs`，再来对照。
//!
//! 本文件的测试与练习测试断言完全相同，且默认随 `cargo test` 运行，
//! 保证参考答案本身始终是对的。
#![allow(dead_code)]

use std::collections::HashMap;

/// 练习 1 参考答案：entry API 把「查找 + 插入/更新」合成一次哈希查找，
/// 是 HashMap 计数的惯用写法；`split_whitespace` 自动处理连续空白，
/// 不需要手动 split 再过滤空串。
pub fn word_freq(text: &str) -> HashMap<String, usize> {
    let mut map = HashMap::new();
    for word in text.split_whitespace() {
        *map.entry(word.to_string()).or_insert(0) += 1;
    }
    map
}

/// 练习 2 参考答案：HashMap 迭代顺序不确定，必须先落到 Vec 再排序。
/// `Ordering::then` 把「次数降序、词升序」两级规则串成一个比较器，
/// 保证并列名次时输出稳定；`truncate(k)` 对 k 超长的情况天然安全。
pub fn top_k(freq: &HashMap<String, usize>, k: usize) -> Vec<(String, usize)> {
    let mut pairs: Vec<(String, usize)> =
        freq.iter().map(|(w, c)| (w.clone(), *c)).collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    pairs.truncate(k);
    pairs
}

/// 练习 3 参考答案：filter/map 是惰性适配器，`sum()` 才驱动整条链；
/// 编译后与手写 for 循环等价（零成本抽象），但「筛偶数 → 平方 → 求和」
/// 的意图一眼可读。`copied()` 把 `&i64` 转成 `i64`，省去闭包里的解引用。
pub fn sum_even_squares(nums: &[i64]) -> i64 {
    nums.iter()
        .copied()
        .filter(|n| n % 2 == 0)
        .map(|n| n * n)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ex1_word_freq() {
        let freq = word_freq("hello world hello");
        assert_eq!(freq.len(), 2);
        assert_eq!(freq.get("hello"), Some(&2));
        assert_eq!(freq.get("world"), Some(&1));
        assert!(word_freq("").is_empty());
    }

    #[test]
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
    fn ex3_sum_even_squares() {
        assert_eq!(sum_even_squares(&[1, 2, 3, 4]), 20);
        assert_eq!(sum_even_squares(&[]), 0);
        assert_eq!(sum_even_squares(&[-2, 3]), 4);
    }
}
