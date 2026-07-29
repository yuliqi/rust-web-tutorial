# 第 26 章：Web 终端与堡垒机 — PTY over WebSocket

> 示例工程：`examples-middleware/webterm_demo`（需要 Postgres 存审计）
>
> ⚠️ Web shell 是**极其危险**的能力——把服务器命令行暴露到网页。本章反复强调:
> 必须鉴权 + 授权 + 全程录制 + 命令限制,缺一不可。

## 学习目标

1. 理解堡垒机(跳板机)在运维安全中的定位:统一入口、认证、授权、审计
2. 用 PTY over WebSocket 实现浏览器里的真终端
3. 实现会话录制与访问审计——堡垒机的灵魂
4. 分清本章骨架与生产完整堡垒机的差距

## 26.1 堡垒机:所有访问的唯一入口

生产服务器不该让运维直接 SSH 进去——散乱、无审计、权限难收。**堡垒机**(跳板机)是统一入口:所有对服务器的访问都经过它,由它做认证、授权、**全程录制、事后可审计**。出了安全事故,能查到「谁、何时、从哪、在哪台机器、敲了什么命令」。

本章做一个最小但真实的核心:浏览器里操作 shell,服务端把 shell 的 PTY 通过 WebSocket(第 24 章)桥接到浏览器,并录制整个会话 + 记录访问审计。

## 26.2 PTY:为什么不是简单的管道

要让浏览器里的终端像真终端,不能只是把命令的 stdout 用管道接出来。需要 **PTY(伪终端)**:

```text
浏览器 ⇄ WebSocket ⇄ 服务端 ⇄ PTY master ⇄ shell 进程
```

PTY 相比裸管道多了:行编辑、信号(Ctrl-C)、终端尺寸(窗口大小)、颜色转义——这些是「真终端体验」的必需。`webterm_demo` 用 `portable-pty`(跨平台)打开伪终端并 spawn shell,把 master 的读写端桥接到 WebSocket。终端协议要区分**数据**与**控制**消息:

```rust
enum ClientMsg { Input(String), Resize { rows, cols } }   // 输入 vs 改窗口大小
enum ServerMsg { Output(String) }
```

窗口尺寸要 `clamp`(限制在合理范围),防止恶意的巨大尺寸值——这类边界校验是纯逻辑,能穷尽单测。

## 26.3 会话录制与审计:堡垒机的灵魂

堡垒机区别于「普通 Web 终端」的地方,就是**每一段 I/O 都录下来、可回放、可审计**:

```sql
-- schema bastion
terminal_sessions(id, account_id, target, started_at, ended_at, client_ip)  -- 谁/何时/从哪/连哪台
session_events(id, session_id, direction, data, at_ms)                       -- input/output 逐段带时间戳
```

`direction` 分 input/output——**输入也要录**,因为审计要知道「谁敲了什么危险命令」,不只是看输出。`at_ms` 时间戳让会话能按原速回放(格式思路接近 asciinema)。`webterm_demo` 提供 `GET /sessions`(列会话)和 `/sessions/{id}/replay`(取事件序列回放)。

三个安全要点(源码 ⚠️ 注释强调):

- **为什么录制**:合规审计、事后追责、安全取证——出事了能还原现场
- **敏感信息脱敏**:密码、密钥不能明文进录像
- **录像防篡改**:生产要加密存储 + 哈希链,防内部人事后删改自己的记录

WebSocket 入口必须鉴权(`AuthUser` 提取器,第 17 章),断线时 `end_session` 干净收尾(第 24 章连接级优雅停机)。

## 26.4 骨架 vs 生产完整堡垒机

`webterm_demo` 演示的是核心骨架「PTY over WebSocket + 录制 + 审计」。生产完整堡垒机还需要:

- **SSH 协议代理**:本例在跳板机**本地** spawn shell 仅为演示;生产用 `russh` 等 spawn「SSH 到被授权的目标机」
- **授权**:接第 23 章 RBAC/资源级授权,决定谁能连哪台、用什么账号(本例静态 token → 固定账号)
- **实时监控与阻断**:管理员实时旁观会话、必要时掐断(可在第 24 章广播骨架上加)
- **命令黑白名单**:写入 PTY 前拦截 `rm -rf /`、`shutdown` 等
- **录像加密与 WORM**(一次写入多次读取,防删改)

这些在附录 H 有进一步的选型与架构说明。

```bash
cd examples-middleware
docker compose up -d postgres
cargo run -p webterm_demo    # 浏览器开 http://127.0.0.1:3006 敲命令，会话被录制进库
```

## 本章小结

- 堡垒机是服务器访问的统一入口:认证、授权、全程录制、可审计
- PTY 相比裸管道多了行编辑/信号/尺寸/颜色,是真终端体验的必需
- 终端协议区分数据(input/output)与控制(resize)消息,尺寸要 clamp
- 会话录制:input 和 output 都逐段带时间戳记录,可按原速回放
- 输入必须录(审计危险命令)、敏感信息要脱敏、录像要防篡改
- 本章是骨架;生产还需 SSH 代理、授权、实时阻断、命令黑白名单

**自测清单**

- [ ] 我能说出堡垒机相比"直接 SSH"解决了什么
- [ ] 我能解释为什么需要 PTY 而不是裸管道
- [ ] 我能说出终端协议为什么要区分数据与控制消息
- [ ] 我能解释会话录制为什么连输入也要录
- [ ] 我能列出录像在安全上要做的三件事(脱敏/加密/防篡改)
- [ ] 我能说出本章骨架距离生产堡垒机还差哪些

## 练习

1. 加命令黑名单:写入 PTY 前检测 `rm -rf /`、`shutdown`、`mkfs` 等,命中则拒绝并记审计告警。
2. 实现会话回放页:读 `/sessions/{id}/replay`,按 `at_ms` 时间差在网页上「按原速重演」这次会话。
3. 加实时监控:用第 24 章的广播,让管理员打开一个页面**旁观**某个正在进行的会话(只读镜像流)。
4. 把授权接上第 23 章:`can(grants, "server", target, "ssh")` 判定该账号能否连这台目标机,替换掉现在的固定账号。

**动手验证**：

```bash
cd examples-middleware
docker compose up -d postgres
cargo test -p webterm_demo -- --ignored   # 会话录制→回放顺序→审计可查的断言
```

---

导航：[上一章](25-software-license.md) | [返回目录](../README.md) | [下一章](27-certificate-acme.md)
