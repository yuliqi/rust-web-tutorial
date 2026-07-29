# 第 24 章：实时通信 — WebSocket 与 SSE

> 示例工程：`examples-middleware/realtime_demo`（**不需要 Docker**，纯内存）
>
> `cargo run -p realtime_demo` 后开两个浏览器标签访问 <http://127.0.0.1:3005>,
> 在一个标签发消息,两个标签同时收到——直观看到服务端主动推送与广播。
> 本章是后续「Web 终端/堡垒机、实时监控」的基础。

## 学习目标

1. 理解为什么请求-响应式 HTTP 不够,实时功能需要服务端主动推送
2. 掌握 WebSocket(双向)和 SSE(单向)的实现与取舍
3. 用 `tokio::sync::broadcast` 做一对多广播中心
4. 理解连接级的心跳、超时与优雅断线

## 24.1 为什么需要实时推送

全书到现在都是「客户端问一次、服务端答一次」。但很多功能是服务端**主动**要告诉客户端:Web 终端的命令输出、实时滚动的日志、监控图表的新数据点。HTTP 轮询(客户端每秒问一次)延迟高又浪费,正解是让服务端能主动推:

- **SSE(Server-Sent Events)**:基于普通 HTTP 的**单向**流(服务端→客户端),浏览器 `EventSource` 自带自动重连、穿代理友好、实现简单。适合「只推不收」——日志、监控、进度。
- **WebSocket**:一条 TCP 上的**双向**全双工,需要 Upgrade 握手,要自己处理心跳和重连。适合客户端也频繁上行的交互场景——Web 终端、协同编辑、聊天。

经验法则一句话:**只推不收用 SSE,需要双向交互才上 WebSocket。**

## 24.2 广播中心:一对多推送

实时功能常是「一条消息推给所有在线连接」(一个人发言,所有人看到)。`realtime_demo` 用 `tokio::sync::broadcast` 做广播中心:

```rust
struct Event { kind: String, payload: serde_json::Value, ts: i64 }
struct Hub { tx: broadcast::Sender<Event> }
// 每个连接 subscribe() 拿一个 receiver；publish() 广播给所有 receiver
```

两个要点(源码注释有):

- **broadcast vs mpsc**:mpsc 是多生产者单消费者(活给一个人干);broadcast 是一发多收(消息给所有人)。实时推送要的是后者。
- **慢消费者(Lagged)**:某个连接消费太慢、跟不上广播速度时,`broadcast` 会丢老消息并返回 `Lagged` 错误——实时系统的取舍是「宁可丢老数据也不阻塞其他人」,消息本身也要有稳定的 JSON schema(同第 14/19 章「消息即契约」)。

## 24.3 WebSocket:握手、双向、心跳

axum 的 WebSocket handler 升级连接后,把连接 `split` 成读、写两半:

```text
写半：spawn 一个任务，把 hub 订阅到的 Event 不断推给客户端
读半：接收客户端上行消息（回显或作为新的 publish 源）
心跳：定期发 Ping，收 Pong；超时没响应就断开
```

**为什么要应用层心跳**:TCP 层察觉不到对端「假死」(断电、拔网线),连接会僵在那里占资源。应用层定期 Ping/Pong 才能及时发现僵尸连接并清理。断线时任务要干净退出、不 panic、不泄漏——这是把第 16 章的优雅停机思想下沉到**单个连接**。

## 24.4 SSE:更简单的单向推送

如果客户端只需要「听」,SSE 比 WebSocket 省事得多——axum 把广播流包成 `Sse<Stream>` 就行,浏览器 `EventSource` 会自动重连:

```text
GET /sse → 把 hub 的 Event 流转成 SSE 持续推送
浏览器 new EventSource('/sse').onmessage = ...   // 自动重连，无需手写
```

日志跟随、监控数据、任务进度这类场景,SSE 是更对的选择。

```bash
cd examples-middleware
cargo run -p realtime_demo    # 开两个浏览器标签，一个发消息，两个都实时收到
```

## 本章小结

- 实时功能需要服务端主动推送,HTTP 轮询延迟高又浪费
- SSE 单向、走普通 HTTP、自动重连、简单——只推不收的场景首选
- WebSocket 双向全双工、需握手与心跳——客户端要交互才用
- 广播用 `broadcast`,一发多收;慢消费者会 Lagged 丢老消息,不阻塞他人
- 应用层心跳(Ping/Pong)才能发现 TCP 察觉不到的僵尸连接
- 断线时连接任务要干净退出,是连接级的优雅停机

**自测清单**

- [ ] 我能说出为什么实时功能不能用 HTTP 轮询
- [ ] 我能列出 SSE 与 WebSocket 的差异并按场景选型
- [ ] 我能解释 broadcast 与 mpsc 的区别
- [ ] 我能说出慢消费者 Lagged 的语义和取舍
- [ ] 我能解释为什么需要应用层心跳
- [ ] 我能描述连接断开时该怎么干净收尾

## 练习

1. 给广播加「频道」:`/ws?channel=room1` 只收该频道的消息(提示:每个频道一个 broadcast，或消息带 channel 字段过滤)。
2. 用 SSE 实现一个「任务进度条」:后台任务每完成一步就 publish 进度,前端 EventSource 实时更新百分比。
3. 加断线重连:WebSocket 前端在连接断开后指数退避重连,恢复后补拉断线期间的消息(提示:客户端记住最后收到的消息 id)。
4. 把第 21 章 `scheduler_demo` 的任务执行结果通过 SSE 实时推到一个监控页——为第 28 章实时监控热身。

**动手验证**：

```bash
cd examples-middleware
cargo test -p realtime_demo    # hub 广播 + WS/SSE 推送到达的断言，默认全跑
```

---

导航：[上一章](23-2fa-iam.md) | [返回目录](../README.md) | [下一章](25-software-license.md)
