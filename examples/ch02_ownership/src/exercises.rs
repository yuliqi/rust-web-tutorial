//! 第 2 章练习：把每个 `todo!()` 换成你的实现。
//!
//! 做题流程：
//! 1. 只跑练习测试（未完成时会失败，这是预期的）：
//!    `cargo test -p ch02_ownership -- --ignored`
//! 2. 全部通过后，对照参考答案 `src/solutions.rs`（先自己做，别偷看）。
#![allow(dead_code)]
#![allow(unused_variables)]

/// 练习 1：返回句子中的最后一个单词（`first_word` 的镜像版本）。
///
/// - 有空格时返回最后一个空格之后的部分；
/// - 没有空格时返回整个字符串。
///
/// 提示：`main.rs` 里的 `first_word` 用了 `s.find(' ')`，这里换成
/// `s.rfind(' ')` 从右往左找。返回 `&str` 切片而不是新 `String`，
/// 因为返回值只是入参的一段借用——零拷贝、无需分配，这正是切片的意义。
pub fn last_word(s: &str) -> &str {
    todo!()
}

/// 练习 2：就地规范化——去首尾空白并全部转大写（`normalize` 的大写版本）。
///
/// 提示：接收 `&mut String` 就地修改，而不是拿走所有权再返回新值，
/// 这样调用方不用重新绑定变量、也不会丢失所有权。
/// `trim()` / `to_uppercase()` 会算出新数据，最后用 `*s = ...`
/// 通过可变借用把结果写回去。
// 本题特意用 &mut String（实现里要 `*s = ...` 整体替换，&mut str 做不到），
// clippy 对空实现会误报 ptr_arg，这里显式压掉。
#[allow(clippy::ptr_arg)]
pub fn shout(s: &mut String) {
    todo!()
}

/// 练习 3：返回切片中最长的字符串（并列时返回最靠前的），全程不许 clone。
///
/// 调用方保证 `words` 非空。
///
/// 提示：入参用 `&[String]` 借用而不是 `Vec<String>` 拿走所有权，
/// 调用之后原 Vec 还能继续用；返回 `&str` 借用其中一个元素即可。
/// 生命周期省略规则会自动把返回值和入参绑在一起
/// （等价于 `fn longest_in<'a>(words: &'a [String]) -> &'a str`，参考 2.7 节）。
pub fn longest_in(words: &[String]) -> &str {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "练习：完成后运行 cargo test -p ch02_ownership -- --ignored"]
    fn ex1_last_word() {
        assert_eq!(last_word("ownership is power"), "power");
        assert_eq!(last_word("hello world"), "world");
        assert_eq!(last_word("hello"), "hello");
    }

    #[test]
    #[ignore = "练习：完成后运行 cargo test -p ch02_ownership -- --ignored"]
    fn ex2_shout() {
        let mut s = String::from("  hello rust  ");
        shout(&mut s);
        assert_eq!(s, "HELLO RUST");

        let mut already = String::from("OK");
        shout(&mut already);
        assert_eq!(already, "OK");
    }

    #[test]
    #[ignore = "练习：完成后运行 cargo test -p ch02_ownership -- --ignored"]
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
