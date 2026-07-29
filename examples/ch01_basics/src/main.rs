//! 第 1 章：语言基础示例（Rust 1.97 / edition 2024）
//!
//! 练习入口：`src/exercises.rs`（做题）→ `src/solutions.rs`（对照答案）
//! 只跑练习测试：`cargo test -p ch01_basics -- --ignored`

mod exercises;
mod solutions;

fn celsius_to_fahrenheit(c: f64) -> f64 {
    c * 9.0 / 5.0 + 32.0
}

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Unknown",
    }
}

fn bucket(n: u8) -> &'static str {
    match n {
        0 => "zero",
        1..10 => "single digit (1-9)",
        10..=99 => "two digits",
        _ => "big",
    }
}

fn abs_diff(a: i32, b: i32) -> i32 {
    if a > b { a - b } else { b - a }
}

fn count_vowels(s: &str) -> usize {
    s.chars().filter(|c| "aeiouAEIOU".contains(*c)).count()
}

/// let chains：多层条件拍平（1.88+ / edition 2024）
fn can_publish(title: Option<&str>, user_active: bool) -> bool {
    if let Some(t) = title && !t.trim().is_empty() && user_active {
        true
    } else {
        false
    }
}

fn main() {
    let mut hits = 0;
    hits += 1;
    let hits = hits;

    let name = "Rustacean";
    println!("Hello, {name}! hits={hits}");
    println!("36.5C = {:.1}F", celsius_to_fahrenheit(36.5));
    println!("|3-10| = {}", abs_diff(3, 10));
    println!("status 404 => {}", status_text(404));
    println!("bucket(7) => {}", bucket(7));
    println!("vowels in 'education' = {}", count_vowels("education"));
    println!(
        "can_publish => {}",
        can_publish(Some(" ship it "), true)
    );

    let mut n = 0;
    let doubled = loop {
        n += 1;
        if n == 5 {
            break n * 2;
        }
    };
    println!("loop result = {doubled}");

    for i in 1..=3 {
        println!("tick {i}");
    }
}
