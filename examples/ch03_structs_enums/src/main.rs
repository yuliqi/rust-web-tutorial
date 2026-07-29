//! 第 3 章：结构体 / 枚举 / 模式匹配（含 1.95 if-let guard、let chains）
//!
//! 练习入口：`src/exercises.rs`（做题）→ `src/solutions.rs`（对照答案）
//! 只跑练习测试：`cargo test -p ch03_structs_enums -- --ignored`

mod exercises;
mod solutions;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum TodoStatus {
    Pending,
    Doing,
    Done,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct Todo {
    id: u64,
    title: String,
    status: TodoStatus,
}

impl Todo {
    fn new(id: u64, title: impl Into<String>) -> Self {
        Self {
            id,
            title: title.into(),
            status: TodoStatus::Pending,
        }
    }

    fn mark_done(&mut self) {
        self.status = TodoStatus::Done;
    }

    fn transition(mut self, next: TodoStatus) -> Result<Self, String> {
        use TodoStatus::*;
        let ok = match (self.status, next) {
            (Pending, Doing) | (Pending, Done) | (Doing, Done) | (Doing, Pending) => true,
            (s, t) if s == t => true,
            _ => false,
        };
        if !ok {
            return Err(format!(
                "invalid transition: {:?} -> {:?}",
                self.status, next
            ));
        }
        self.status = next;
        Ok(self)
    }
}

#[derive(Debug)]
#[allow(dead_code)]
enum ApiError {
    NotFound,
    BadRequest(String),
    Internal,
}

#[derive(Debug, Clone)]
struct User {
    email: String,
    active: bool,
}

fn find_user(id: u64) -> Option<User> {
    if id == 1 {
        Some(User {
            email: "neo@matrix.io".into(),
            active: true,
        })
    } else {
        None
    }
}

fn handle(err: ApiError) -> (u16, String) {
    match err {
        ApiError::NotFound => (404, "not found".into()),
        ApiError::BadRequest(msg) => (400, msg),
        ApiError::Internal => (500, "internal error".into()),
    }
}

fn parse_id(raw: &str) -> Result<u64, String> {
    raw.parse::<u64>()
        .map_err(|_| format!("invalid id: {raw}"))
}

/// 1.95+：match 臂 if let guard
fn parse_status_line(line: &str) -> Option<u16> {
    match line.split_whitespace().last() {
        Some(raw) if let Ok(code) = raw.parse::<u16>() && (200..600).contains(&code) => Some(code),
        _ => None,
    }
}

fn main() {
    let mut todo = Todo::new(1, "learn match");
    println!("created: {todo:?}");
    todo.mark_done();
    println!("done: {todo:?}");

    match Todo::new(2, "ok jump").transition(TodoStatus::Done) {
        Ok(t) => println!("transition ok: {t:?}"),
        Err(e) => println!("transition err: {e}"),
    }

    let t = Todo {
        id: 3,
        title: "closed".into(),
        status: TodoStatus::Done,
    };
    println!("{:?}", t.transition(TodoStatus::Pending));

    let (code, msg) = handle(ApiError::BadRequest("title required".into()));
    println!("api error => {code} {msg}");

    let Ok(id) = parse_id("42") else {
        panic!("parse failed");
    };
    println!("parsed id={id}");

    // let chains
    if let Some(user) = find_user(1) && user.active {
        println!("active user: {}", user.email);
    }

    println!(
        "status line => {:?}",
        parse_status_line("GET /health 200")
    );
}
