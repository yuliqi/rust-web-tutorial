//! 第 2 章练习参考答案。先自己完成 `exercises.rs`，再来对照。
//!
//! 本文件的测试与练习测试断言完全相同，且默认随 `cargo test` 运行，
//! 保证参考答案本身始终是对的。
#![allow(dead_code)]

/// 练习 1 参考答案：`rfind` 找到最后一个空格，`&s[i + 1..]` 切出之后的部分。
/// 返回的是借用自入参的切片，零分配；借用检查器保证它不会比 `s` 活得更久。
pub fn last_word(s: &str) -> &str {
    match s.rfind(' ') {
        Some(i) => &s[i + 1..],
        None => s,
    }
}

/// 练习 2 参考答案：先用 `trim` + `to_uppercase` 算出新值，再 `*s = ...` 写回。
/// 用 `&mut String` 就地修改，调用方无需重新绑定变量，也不会丢失所有权——
/// 这是「独占可变借用」的典型用法。
pub fn shout(s: &mut String) {
    let upper = s.trim().to_uppercase();
    *s = upper;
}

/// 练习 3 参考答案：只用 `>`（严格大于）比较，保证并列时保留最靠前的那个。
/// 全程只移动引用、不 clone 任何 String；省略的生命周期等价于
/// `fn longest_in<'a>(words: &'a [String]) -> &'a str`，
/// 编译器由此知道返回值借用自 `words`。
pub fn longest_in(words: &[String]) -> &str {
    let mut best = &words[0];
    for w in &words[1..] {
        if w.len() > best.len() {
            best = w;
        }
    }
    best.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ex1_last_word() {
        assert_eq!(last_word("ownership is power"), "power");
        assert_eq!(last_word("hello world"), "world");
        assert_eq!(last_word("hello"), "hello");
    }

    #[test]
    fn ex2_shout() {
        let mut s = String::from("  hello rust  ");
        shout(&mut s);
        assert_eq!(s, "HELLO RUST");

        let mut already = String::from("OK");
        shout(&mut already);
        assert_eq!(already, "OK");
    }

    #[test]
    fn ex3_longest_in() {
        let words = vec![
            "hi".to_string(),
            "borrow".to_string(),
            "rustacean".to_string(),
        ];
        assert_eq!(longest_in(&words), "rustacean");

        let tie = vec!["aa".to_string(), "bb".to_string()];
        assert_eq!(longest_in(&tie), "aa");

        // words 只是被借用，调用后依然可用（没有被 move）
        assert_eq!(words.len(), 3);
    }
}
