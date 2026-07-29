# 附录 A：常见编译错误速查

> 先确认工具链：`rustc 1.97.1`。版本过旧时，某些依赖会直接拒绝编译。

## E0382 value used after move

值被 move 后继续使用。

**处理：**

- 改用借用 `&T` / `&mut T`
- 需要双持有时 `clone`
- 对可 `Copy` 类型保持小而 Copy

## E0502 cannot borrow as mutable because it is also borrowed as immutable

同时存在不可变借用与可变借用。

**处理：**缩短借用生命周期，拆开作用域。

## E0515 cannot return reference to local variable

返回了指向局部值的引用。

**处理：**返回 `String`/`Vec` 等拥有型数据，或从输入参数借用。

## E0277 trait bound not satisfied

类型没实现需要的 Trait。

**处理：**

- 加 `#[derive(...)]`
- 显式 `impl Trait for Type`
- 检查泛型 bound

## E0308 mismatched types

类型不匹配（最常见）。

**处理：**看期望类型与实际类型，补转换：`.to_string()` / `into()` / `as`（谨慎）/ `From`。

## E0282 type annotations needed

类型推断失败。

**处理：**补显式类型，如 `let x: Vec<i32> = ...` 或涡轮鱼 `collect::<Vec<_>>()`。

## async: future cannot be sent between threads safely

任务要求 `Send`，捕获了 `Rc`/`RefCell` 等。

**处理：**换 `Arc`/`Mutex`，检查是否在 await 点持有非 Send 守卫。

## sqlx: pool timed out / connection refused

DB URL 或池配置问题。先检查 `DATABASE_URL` 与文件权限。

## serde: missing field

JSON 缺字段。可用 `#[serde(default)]` 或 `Option<T>`。
