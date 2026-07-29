//! 第 1 章练习参考答案。先自己完成 `exercises.rs`，再来对照。
//!
//! 本文件的测试与练习测试断言完全相同，且默认随 `cargo test` 运行，
//! 保证参考答案本身始终是对的。
#![allow(dead_code)]

/// 练习 1 参考答案：华氏 → 摄氏。
pub fn fahrenheit_to_celsius(f: f64) -> f64 {
    (f - 32.0) * 5.0 / 9.0
}

/// 练习 2 参考答案：用 `Ordering` + match 穷尽三种情况，
/// 比 if/else 链更能体现「编译器帮你检查漏没漏分支」。
pub fn guess_hint(secret: u32, guess: u32) -> &'static str {
    use std::cmp::Ordering;
    match guess.cmp(&secret) {
        Ordering::Less => "too small",
        Ordering::Greater => "too big",
        Ordering::Equal => "correct",
    }
}

/// 练习 3 参考答案：元组匹配让「同时被 3 和 5 整除」的分支一目了然。
/// 注意分支顺序：`(0, 0)` 必须放在 `(0, _)` / `(_, 0)` 之前。
pub fn fizzbuzz(n: u32) -> String {
    match (n % 3, n % 5) {
        (0, 0) => "FizzBuzz".to_string(),
        (0, _) => "Fizz".to_string(),
        (_, 0) => "Buzz".to_string(),
        _ => n.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ex1_fahrenheit_to_celsius() {
        assert!((fahrenheit_to_celsius(212.0) - 100.0).abs() < 1e-9);
        assert!((fahrenheit_to_celsius(32.0) - 0.0).abs() < 1e-9);
        assert!((fahrenheit_to_celsius(98.6) - 37.0).abs() < 1e-9);
    }

    #[test]
    fn ex2_guess_hint() {
        assert_eq!(guess_hint(50, 30), "too small");
        assert_eq!(guess_hint(50, 80), "too big");
        assert_eq!(guess_hint(50, 50), "correct");
    }

    #[test]
    fn ex3_fizzbuzz() {
        assert_eq!(fizzbuzz(15), "FizzBuzz");
        assert_eq!(fizzbuzz(9), "Fizz");
        assert_eq!(fizzbuzz(10), "Buzz");
        assert_eq!(fizzbuzz(7), "7");
    }
}
