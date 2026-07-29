# 示例工程（Cargo Workspace）

基准工具链：

```text
rustc 1.97.1 (8bab26f4f 2026-07-14)
cargo 1.97.1 (c980f4866 2026-06-30)
rustup 1.29.0 (28d1352db 2026-03-05)
```

上级目录的 `rust-toolchain.toml` 会自动选用 1.97.1。

```bash
# 在本目录执行
rustc --version
cargo test --workspace
cargo run -p ch01_basics
cargo run -p todo_api
```

## 章节 ↔ crate 索引

| crate | 对应章节 | 学什么 |
|-------|---------|--------|
| `ch01_basics` | [第 1 章](../docs/chapters/01-language-basics.md) | 变量、函数、控制流、let chains |
| `ch02_ownership` | [第 2 章](../docs/chapters/02-ownership.md) | 所有权、借用、切片 |
| `ch03_structs_enums` | [第 3 章](../docs/chapters/03-structs-enums.md) | 结构体、枚举、match、状态机 |
| `ch04_errors` | [第 4 章](../docs/chapters/04-error-handling.md) | Result、`?`、thiserror/anyhow |
| `ch05_collections` | [第 5 章](../docs/chapters/05-collections-iterators.md) | Vec/HashMap、迭代器链 |
| `ch06_traits` | [第 6 章](../docs/chapters/06-generics-traits.md) | 泛型、trait、静态/动态分发 |
| `ch07_modules_lib/bin` | [第 7 章](../docs/chapters/07-modules-workspace.md) | 模块、lib+bin 工程组织 |
| `ch08_testing` | [第 8 章](../docs/chapters/08-testing-quality.md) | 单测、集成测试、doctest |
| `ch09_smart_pointers` | [第 9 章](../docs/chapters/09-smart-pointers.md) | Box/Rc/RefCell、LazyLock |
| `ch10_async` | [第 10 章](../docs/chapters/10-concurrency-async.md) | 线程、tokio、async/await |
| `ch11_http_client` | [第 11 章](../docs/chapters/11-web-ecosystem.md) | reqwest、serde、Web 生态 |
| `todo_api` | [第 12 章](../docs/chapters/12-todo-api-project.md) | 综合项目：axum + sqlx REST API |

## 练习怎么做

语言章节的 crate（ch01–ch06、ch09–ch11）都带练习支架：

- `src/exercises.rs` —— 题目，把 `todo!()` 换成你的实现
- `src/solutions.rs` —— 参考答案（默认随 `cargo test` 验证，保证答案是对的）

```bash
# 只跑某章的练习测试（没做完时失败是预期的）
cargo test -p ch01_basics -- --ignored

# 该章全部测试（练习测试默认跳过，答案测试保持绿色）
cargo test -p ch01_basics
```

做题顺序建议：读完章节正文 → `cargo run -p chXX` 看演示 → 做 `exercises.rs` → 对照 `solutions.rs`。

`Cargo.toml` 约定：

- `edition = "2024"`
- `rust-version = "1.97"`

综合项目默认地址：`http://127.0.0.1:3000`

```bash
curl -s http://127.0.0.1:3000/health
curl -s -X POST http://127.0.0.1:3000/todos \
  -H 'content-type: application/json' \
  -d '{"title":"learn rust"}'
curl -s 'http://127.0.0.1:3000/todos?done=false'
```
