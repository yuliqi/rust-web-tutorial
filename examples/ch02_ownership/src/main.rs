//! 第 2 章：所有权示例
//!
//! 练习入口：`src/exercises.rs`（做题）→ `src/solutions.rs`（对照答案）
//! 只跑练习测试：`cargo test -p ch02_ownership -- --ignored`

mod exercises;
mod solutions;

fn take_ownership(s: String) {
    println!("took: {s}");
}

// 演示借用时故意用 &String 与正文对应；正式代码应写 &str（见本章「工程建议」）。
#[allow(clippy::ptr_arg)]
fn calculate_len(s: &String) -> usize {
    s.len()
}

fn append_bang(s: &mut String) {
    s.push('!');
}

fn first_word(s: &str) -> &str {
    match s.find(' ') {
        Some(i) => &s[..i],
        None => s,
    }
}

fn count_words(text: &str) -> usize {
    text.split_whitespace().count()
}

fn normalize(s: &mut String) {
    let trimmed = s.trim().to_lowercase();
    *s = trimmed;
}

fn greet(name: &str) {
    println!("hi, {name}");
}

fn main() {
    let s1 = String::from("hello");
    let s2 = s1; // move
    // println!("{s1}"); // 不能再用 s1
    println!("moved value: {s2}");

    let mut name = String::from("Neo");
    println!("len={}", calculate_len(&name));
    append_bang(&mut name);
    println!("mut borrow => {name}");

    take_ownership(name);
    // name 已 move

    let owned = String::from("Trinity");
    greet(&owned);
    greet("Morpheus");

    let sentence = String::from("ownership is power");
    println!("first word = {}", first_word(&sentence));
    println!("word count = {}", count_words(&sentence));

    let mut messy = String::from("  HeLLo  ");
    normalize(&mut messy);
    println!("normalized = '{messy}'");
}
