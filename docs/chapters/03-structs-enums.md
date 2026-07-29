# 第 3 章：结构体、枚举与模式匹配

> 示例：`examples/ch03_structs_enums`

## 学习目标

- 用 struct/enum 建模业务数据
- 熟练 `impl` 方法
- 用 `match` / `if let` / `let-else` 处理分支

## 3.1 结构体

```rust
#[derive(Debug, Clone)]
struct User {
    id: u64,
    email: String,
    active: bool,
}

impl User {
    fn new(id: u64, email: impl Into<String>) -> Self {
        Self {
            id,
            email: email.into(),
            active: true,
        }
    }

    fn deactivate(&mut self) {
        self.active = false;
    }
}
```

Web 里，struct 常同时承担：

- 领域模型（Domain）
- 请求/响应 DTO（配合 `serde`）

## 3.2 枚举：把非法状态排除在类型外

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TodoStatus {
    Pending,
    Doing,
    Done,
}

#[derive(Debug)]
enum ApiError {
    NotFound,
    BadRequest(String),
    Internal,
}
```

比一堆布尔/魔法数字更安全。

## 3.3 `Option` 与 `Result`

标准库两个最重要枚举：

```rust
enum Option<T> { None, Some(T) }
enum Result<T, E> { Ok(T), Err(E) }
```

```rust
fn find_user(id: u64) -> Option<User> {
    if id == 1 {
        Some(User::new(1, "neo@matrix.io"))
    } else {
        None
    }
}

fn parse_id(raw: &str) -> Result<u64, String> {
    raw.parse::<u64>().map_err(|_| format!("invalid id: {raw}"))
}
```

## 3.4 模式匹配

```rust
fn handle(err: ApiError) -> (u16, String) {
    match err {
        ApiError::NotFound => (404, "not found".into()),
        ApiError::BadRequest(msg) => (400, msg),
        ApiError::Internal => (500, "internal error".into()),
    }
}

// if let
if let Some(user) = find_user(1) {
    println!("{}", user.email);
}

// let-else（早期返回非常适合 handler）
let Ok(id) = parse_id("42") else {
    return;
};

// let chains：多层条件拍平
if let Some(user) = find_user(1) && user.active {
    println!("active user: {}", user.email);
}

// 1.95+：match 臂上的 if let guard
fn parse_status_line(line: &str) -> Option<u16> {
    match line.split_whitespace().last() {
        Some(raw) if let Ok(code) = raw.parse::<u16>() && (200..600).contains(&code) => Some(code),
        _ => None,
    }
}
```

> 提示：Edition 2024 对 `if let` 临时值作用域更严格（更安全）。若从 2021 升级后出现「临时值提前 drop」相关错误，优先缩短借用链或先绑定到变量。

## 3.5 方法接收者

```rust
impl User {
    fn email(&self) -> &str { &self.email }          // 借用
    fn into_email(self) -> String { self.email }     // 拿走所有权
    fn set_email(&mut self, email: String) { self.email = email; }
}
```

## 3.6 建模待办（为第 12 章铺路）

```rust
#[derive(Debug, Clone)]
struct Todo {
    id: u64,
    title: String,
    status: TodoStatus,
}

impl Todo {
    fn mark_done(&mut self) {
        self.status = TodoStatus::Done;
    }
}
```

后续会把它映射到：

- HTTP JSON DTO
- 数据库行
- 服务层命令

## 运行示例

```bash
cargo run -p ch03_structs_enums
```

## 本章小结

- `struct` + `impl` 把数据与行为放在一起；`new` 惯例返回 `Self`，`impl Into<String>` 让入参更灵活
- `enum` 把非法状态排除在类型外，比一堆布尔/魔法数字安全
- `Option`/`Result` 就是标准库枚举：用类型表达「可能没有」与「可能失败」
- 分支工具各有场景：`match` 穷尽处理、`if let` 只关心一种、`let-else` 早期返回、let chains 拍平多层条件
- 方法接收者决定语义：`&self` 借用、`&mut self` 可变借用、`self` 拿走所有权
- Edition 2024 对 `if let` 临时值作用域更严格；1.95+ 支持 match 臂上的 if let guard

**自测清单**

- [ ] 我能用 `enum` 建模状态，并让编译器逼我处理所有分支
- [ ] 我能解释 `Option` 与 `Result` 的区别及各自适用场景
- [ ] 我能在 `match` / `if let` / `let-else` 之间选对工具
- [ ] 我能说出 `&self`、`&mut self`、`self` 三种接收者的差异
- [ ] 我能用 `Result` 实现禁止非法状态转移的业务规则

## 练习

1. 扩展 `TodoStatus`，增加 `Cancelled`，并更新所有 `match`。
2. 实现 `Todo::transition(self, next: TodoStatus) -> Result<Todo, String>`，禁止 `Done -> Pending`。
3. 用 `Option` 实现内存版 `TodoRepo::get/create`。

**动手做题**：打开 `examples/ch03_structs_enums/src/exercises.rs`，把 `todo!()` 换成你的实现（`parse_status` 枚举解析、`transition` 状态机、`find_task_title` Option 组合子）：

```bash
cd examples
cargo test -p ch03_structs_enums -- --ignored   # 自动判题（没做完时失败是预期的）
```

全部通过后对照参考答案 `src/solutions.rs`（默认随 `cargo test` 验证，答案保证可靠）。

---

导航：[上一章](02-ownership.md) | [返回目录](../README.md) | [下一章](04-error-handling.md)
