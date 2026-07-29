# 第 29 章：应用商店与插件机制

> 示例工程：`examples-middleware/plugin_demo`（**不需要外部服务**,纯内存 + 子进程）

## 学习目标

1. 理解面板类软件的两个扩展维度:应用商店 与 插件机制
2. 实现「应用 = docker-compose 模板 + 参数渲染」的一键安装(1Panel 式)
3. 掌握模板渲染的注入防护
4. 用子进程 + stdio JSON-RPC 实现插件,并能对比三种插件方案

## 29.1 两个扩展维度

面板类软件(如 1Panel)靠两种方式扩展:

- **应用商店**:一键安装「应用」(数据库、博客、网盘)。1Panel 的应用本质是 **docker-compose 模板**,安装 = 填参数 → 渲染模板 → 起容器。
- **插件机制**:让第三方扩展**面板本身**的功能。这需要一套宿主与插件的通信约定。

两者都是「让别人的东西跑进你的系统」,所以**安全边界**是核心。

## 29.2 应用商店:模板 + 参数渲染

```rust
struct AppManifest { id, name, version, description, compose_template: String, params: Vec<AppParam> }
struct AppParam { key, label, default: Option<String>, required: bool }
```

安装流程 `render_compose(manifest, values)`:模板里用 <code v-pre>{{key}}</code> 占位,把用户填的参数(端口、密码、数据目录)替换进去,渲染出最终的 compose,交给 `docker compose up` 就装好了。`plugin_demo` 内置了 postgres、静态站点两个 mock 应用。

**模板注入是这里的头号风险**。用户填的参数值如果含换行或 <code v-pre>{{</code>/<code v-pre>}}</code>,可能破坏 compose 的 YAML 结构、甚至注入额外的服务定义。示例的 `validate_value` 拒绝控制字符(含换行)和模板定界符——注释里点明:**换行是 YAML 的结构分隔符,生产上更稳妥的做法是用 YAML 序列化库输出,而不是字符串替换**(字符串替换永远要防注入,同第 18 章 SQL 参数化的思路)。

渲染的校验是四道:占位符必须都已声明、用户 key 必须已声明、必填项不能缺、每个值过注入校验——纯逻辑,能穷尽单测。

## 29.3 插件机制:子进程 + stdio JSON-RPC

`plugin_demo` 选的方案是**子进程 + 标准输入输出上的 JSON-RPC**:

```text
宿主 → 插件 stdin :  {"id":1, "method":"uppercase", "params":{"text":"abc"}}
插件 → 宿主 stdout:  {"id":1, "result":"ABC"}
```

一行一条(NDJSON)。`PluginHost` 负责 `spawn`(启动插件子进程)、`call`(发一次请求收一次响应,校验 id 配对)、`shutdown`(关 stdin 让插件读到 EOF 自退,超时强杀)。示例里样例插件是本 crate 的第二个 bin,测试能真 spawn 它、真通信(用 cargo 提供的 `CARGO_BIN_EXE_sample_plugin` 拿路径)。

**最小权限**:`PluginManifest.capabilities` 声明插件要什么权限,`authorize` 校验——申请未知能力或未授予的能力都拒绝。宿主永远不该无条件信任插件。

## 29.4 三种插件方案对比

| 方案 | 优点 | 缺点 | 适用 |
|------|------|------|------|
| **子进程 stdio**(本例) | 语言无关、崩溃隔离、易沙箱 | IPC 序列化开销、要管进程生命周期 | 插件可能是任意语言、要强隔离 |
| **WASM(wasmtime)** | 默认沙箱、in-process 快、跨平台字节码 | 插件须编译成 wasm、宿主内嵌运行时增大体积 | 追求性能 + 安全沙箱 |
| **动态库(.so/.dll)** | 最快(普通函数调用无 IPC) | Rust 无稳定 ABI 要手写 `extern "C"`、同地址空间不安全、插件崩溃拖垮宿主 | 信任的、性能极敏感的扩展 |

选型的核心权衡是**隔离性 vs 性能**:子进程隔离最好但最慢,动态库最快但最危险,WASM 是近年的折衷热点。`plugin_demo` 选子进程,因为它对教学最直观、也最安全。

```bash
cd examples-middleware
cargo run -p plugin_demo    # 列商店 → 安装应用看渲染出的 compose → spawn 样例插件 call ping/uppercase
```

## 本章小结

- 面板扩展两维度:应用商店(装别人的应用)、插件(扩展面板本身)
- 应用 = compose 模板 + 参数渲染,安装即渲染后交给 docker compose
- 模板渲染要防注入:拒绝换行/定界符,生产用 YAML 序列化库而非字符串替换
- 插件用子进程 + stdio JSON-RPC:spawn/call/shutdown,id 配对,EOF 优雅退出
- capabilities + authorize 实现插件最小权限,宿主不无条件信任插件
- 三种插件方案权衡隔离性 vs 性能:子进程 / WASM / 动态库

**自测清单**

- [ ] 我能说出应用商店与插件机制各扩展什么
- [ ] 我能描述"应用=compose 模板"的一键安装流程
- [ ] 我能说出模板渲染的注入风险和防护
- [ ] 我能画出宿主与插件的 JSON-RPC 通信流程
- [ ] 我能解释 capabilities 最小权限的作用
- [ ] 我能对比三种插件方案的隔离性与性能权衡

## 练习

1. 给应用商店加「卸载」:记录每个已安装实例的 compose,卸载时 `docker compose down`(真跑或打印命令)。
2. 用 YAML 序列化库(serde_yaml)重写 `render_compose`,把参数作为结构化数据注入而非字符串替换,彻底消除注入风险。
3. 给插件加一个真实能力:实现一个 `disk_usage` 插件,宿主授予 `read:fs` 能力后它才能返回磁盘占用——把 capabilities 授权走通。
4. 调研 WASM 方案:用 wasmtime 跑一个最简 wasm 插件,对比它和子进程方案的调用延迟与隔离性。

**动手验证**：

```bash
cd examples-middleware
cargo test -p plugin_demo    # 模板渲染/注入防护 + 真子进程 JSON-RPC 通信的断言，默认全跑
```

---

导航：[上一章](28-realtime-monitoring.md) | [返回目录](../README.md)
