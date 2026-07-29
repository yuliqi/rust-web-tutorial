//! 第 1 章练习：把每个 `todo!()` 换成你的实现。
//!
//! 做题流程：
//! 1. 只跑练习测试（未完成时会失败，这是预期的）：
//!    `cargo test -p ch01_basics -- --ignored`
//! 2. 全部通过后，对照参考答案 `src/solutions.rs`（先自己做，别偷看）。
#![allow(dead_code)]
#![allow(unused_variables)]

/// 练习 1：实现摄氏温度的反向换算（华氏 → 摄氏）。
///
/// 提示：`main.rs` 里已有正向的 `celsius_to_fahrenheit`，公式反推即可。
/// 注意 Rust 不会帮你做整数/浮点自动转换，运算全程用 `f64`。
pub fn fahrenheit_to_celsius(f: f64) -> f64 {
    todo!("(f - 32.0) 之后该怎么算？")
}

/// 练习 2：猜数字的「提示」函数（第 1 章练习 2 的可测试版本）。
///
/// 返回值要求：
/// - guess < secret 时返回 "too small"
/// - guess > secret 时返回 "too big"
/// - 相等时返回 "correct"
///
/// 提示：可以用 if/else 链，也可以试试 `guess.cmp(&secret)` + `match`，
/// 体会 match 对 `std::cmp::Ordering` 枚举的穷尽匹配。
pub fn guess_hint(secret: u32, guess: u32) -> &'static str {
    todo!()
}

/// 练习 3：经典 FizzBuzz，练习 match + 守卫（guard）。
///
/// - 能被 3 和 5 整除 → "FizzBuzz"
/// - 只被 3 整除 → "Fizz"
/// - 只被 5 整除 → "Buzz"
/// - 其他 → 数字本身的字符串（用 `n.to_string()`）
///
/// 提示：试试 `match (n % 3, n % 5)` 的元组匹配写法。
pub fn fizzbuzz(n: u32) -> String {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "练习：完成后运行 cargo test -p ch01_basics -- --ignored"]
    fn ex1_fahrenheit_to_celsius() {
        assert!((fahrenheit_to_celsius(212.0) - 100.0).abs() < 1e-9);
        assert!((fahrenheit_to_celsius(32.0) - 0.0).abs() < 1e-9);
        assert!((fahrenheit_to_celsius(98.6) - 37.0).abs() < 1e-9);
    }

    #[test]
    #[ignore = "练习：完成后运行 cargo test -p ch01_basics -- --ignored"]
    fn ex2_guess_hint() {
        assert_eq!(guess_hint(50, 30), "too small");
        assert_eq!(guess_hint(50, 80), "too big");
        assert_eq!(guess_hint(50, 50), "correct");
    }

    #[test]
    #[ignore = "练习：完成后运行 cargo test -p ch01_basics -- --ignored"]
    fn ex3_fizzbuzz() {
        assert_eq!(fizzbuzz(15), "FizzBuzz");
        assert_eq!(fizzbuzz(9), "Fizz");
        assert_eq!(fizzbuzz(10), "Buzz");
        assert_eq!(fizzbuzz(7), "7");
    }
}
