# 第 6 章：泛型与 Trait

> 示例：`examples/ch06_traits`

## 学习目标

- 用泛型消除重复
- 定义/实现 Trait，理解 bound
- 知道静态分发与动态分发取舍

## 6.1 泛型函数

```rust
fn first<T>(items: &[T]) -> Option<&T> {
    items.first()
}
```

编译器会单态化（monomorphize）：每种具体类型生成专用代码，运行时零开销。

## 6.2 Trait：行为抽象

```rust
trait Summarize {
    fn summary(&self) -> String;

    fn short(&self) -> String {
        format!("{}...", &self.summary()) // 默认实现
    }
}

struct Article {
    title: String,
    body: String,
}

impl Summarize for Article {
    fn summary(&self) -> String {
        format!("{}: {}", self.title, self.body)
    }
}
```

## 6.3 Trait Bound

```rust
fn notify(item: &impl Summarize) {
    println!("{}", item.summary());
}

fn notify_verbose<T: Summarize + std::fmt::Debug>(item: &T) {
    println!("{:?} => {}", item, item.summary());
}

fn parse_pair<T, U>(a: &str, b: &str) -> Result<(T, U), String>
where
    T: std::str::FromStr,
    U: std::str::FromStr,
{
    let a = a.parse().map_err(|_| "bad a".to_string())?;
    let b = b.parse().map_err(|_| "bad b".to_string())?;
    Ok((a, b))
}
```

## 6.4 常用 Trait（后端高频）

| Trait | 作用 |
|-------|------|
| `Debug` | 调试打印 |
| `Clone` | 显式克隆 |
| `Default` | 默认值 |
| `From`/`Into` | 类型转换 |
| `Display` | 用户可读文本 |
| `Serialize`/`Deserialize` | serde |
| `Send`/`Sync` | 线程安全标记 |

```rust
struct UserId(u64);

impl From<u64> for UserId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}
```

## 6.5 仓库抽象（为 Web 分层铺路）

```rust
trait TodoRepository {
    fn get(&self, id: u64) -> Option<String>;
    fn upsert(&mut self, id: u64, title: String);
}

struct MemoryTodoRepo {
    data: std::collections::HashMap<u64, String>,
}

impl TodoRepository for MemoryTodoRepo {
    fn get(&self, id: u64) -> Option<String> {
        self.data.get(&id).cloned()
    }

    fn upsert(&mut self, id: u64, title: String) {
        self.data.insert(id, title);
    }
}
```

后续可换成 `SqlxTodoRepo`，业务服务不改。

## 6.6 静态分发 vs 动态分发

```rust
// 静态：泛型/impl Trait，编译期确定，通常更快
fn use_repo_static<R: TodoRepository>(repo: &R) { let _ = repo.get(1); }

// 动态：对象安全 Trait + dyn，运行时虚表
fn use_repo_dynamic(repo: &dyn TodoRepository) { let _ = repo.get(1); }
```

原则：热路径优先泛型；插件化/异构集合再 `dyn`。

## 6.7 生命周期标注（够用版）

```rust
fn longer<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.len() >= b.len() { a } else { b }
}
```

结构体持有引用时必须标注：

```rust
struct Page<'a> {
    title: &'a str,
}
```

能拥有就拥有（`String`），可少打很多生命周期战争。

## 6.8 现代 RPIT：`use<..>` 精确捕获（Edition 2024）

返回 `impl Trait` 时，可用 `use<..>` 明确捕获哪些泛型/生命周期，避免「多借了不该借的」：

```rust
fn open_titles<'a>(items: &'a [String]) -> impl Iterator<Item = &'a str> + use<'a> {
    items.iter().map(|s| s.as_str())
}
```

多数日常代码编译器能推断；当生命周期报错难读时，`use<..>` 是 1.97 时代的标准排障手段之一。

## 6.9 Trait 对象向上转型（1.86+）

`dyn SubTrait` 可以向上转成 `dyn SuperTrait`（在对象安全前提下）。插件化接口设计时更自然，不必手动加转换方法。

## 运行示例

```bash
cargo run -p ch06_traits
```

## 本章小结

- 泛型经单态化为每种具体类型生成专用代码，运行时零开销
- Trait 定义行为抽象，可带默认实现；类型 `impl Trait for T` 接入
- Trait bound 三种写法：`&impl Trait`、`<T: A + B>`、`where` 子句（约束多时更清晰）
- 高频 Trait：`Debug`/`Clone`/`Default`/`From`/`Display`/serde 的 `Serialize`/`Deserialize`/`Send`/`Sync`
- 仓库抽象（`trait TodoRepository`）让内存实现与数据库实现可互换，业务层不改
- 静态分发（泛型）编译期确定、通常更快；动态分发（`dyn`）走虚表，适合插件化/异构集合
- 生命周期标注：返回引用与结构体持有引用时需要；能拥有（`String`）就拥有，少打生命周期战争
- Edition 2024 的 `use<..>` 可精确控制 RPIT 捕获，是生命周期排障手段之一

**自测清单**

- [ ] 我能解释单态化及「零开销」的含义
- [ ] 我能定义带默认实现的 Trait 并为自己的类型实现它
- [ ] 我能写出三种 Trait bound 形式并知道何时用 `where`
- [ ] 我能说出静态分发与动态分发的取舍
- [ ] 我能给返回引用的函数和持有引用的结构体标注生命周期

## 练习

1. 为 `Todo` 实现 `Display`。
2. 定义 `FromRow`-风格 Trait：从 `(u64, String, bool)` 构造 Todo。
3. 写泛型 `paginate<T: Clone>(items: &[T], page: usize, size: usize) -> Vec<T>`。

**动手做题**：打开 `examples/ch06_traits/src/exercises.rs`，把 `todo!()` 换成你的实现（`Display for Todo`、`FromRow` trait、`paginate` 泛型 + trait bound）：

```bash
cd examples
cargo test -p ch06_traits -- --ignored   # 自动判题（没做完时失败是预期的）
```

全部通过后对照参考答案 `src/solutions.rs`（默认随 `cargo test` 验证，答案保证可靠）。

---

导航：[上一章](05-collections-iterators.md) | [返回目录](../README.md) | [下一章](07-modules-workspace.md)
