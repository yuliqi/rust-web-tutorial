# 第 28 章：实时监控与告警

> 示例工程：`examples-middleware/monitor_demo`（离线核心可跑;时序落库需 Postgres）
>
> `cargo run -p monitor_demo` 后打开 <http://127.0.0.1:3008> 看实时刷新的 CPU/内存仪表盘。

## 学习目标

1. 会用 sysinfo 采集系统指标,理解采集的注意事项
2. 掌握 Prometheus 文本格式,知道为什么它是事实标准
3. 用 SSE(第 24 章)把指标实时推到浏览器仪表盘
4. 实现阈值告警规则,了解生产告警的进阶问题

## 28.1 监控的两条出口:抓取与推送

服务器和业务的实时状态(CPU/内存/磁盘/请求量)要能看、异常要能告警。`monitor_demo` 演示同一份指标的两条出口:

- **`/metrics`(Prometheus 格式,给机器抓)**:Prometheus 服务器定期来抓,存进时序库,配 Grafana 看图、配告警规则
- **`/stream`(SSE,给人看)**:浏览器 `EventSource` 实时接收,仪表盘秒级刷新

两条出口对应两种消费者:监控系统(抓取)和运维的眼睛(推送)。

## 28.2 采集:sysinfo 的一个坑

```rust
fn collect_system() -> Vec<Metric>   // CPU/内存/磁盘/负载
```

用 `sysinfo` 读系统指标,有个必须知道的坑:**CPU 使用率需要两次采样求差**。首次读 CPU 恒为 0,要隔一个最小间隔再读第二次才有意义。`monitor_demo` 的后台任务持有一个长期的 `Collector` 句柄复用,避免每次都首刷为 0。采集要**快、不阻塞**——它每 2 秒被调一次,不能在里面做慢操作。

## 28.3 Prometheus 文本格式

```text
# HELP cpu_usage_percent CPU usage percentage
# TYPE cpu_usage_percent gauge
cpu_usage_percent 37.5
disk_used_percent{mount="/"} 62.1
```

为什么用这个格式:它是**事实标准**——Prometheus、Grafana、各类告警系统直接就能接,不用自己发明协议(同第 14/19 章「用标准协议」的思路)。`render_prometheus` 是纯函数:每个指标名输出一次 `# HELP`/`# TYPE`,label 值要转义(`\`、`"`、换行),label 顺序保持稳定让输出可复现、可测试。指标类型这里都是 `gauge`(瞬时值,可增可减),另一种常见的是 `counter`(只增计数,如累计请求数)。

## 28.4 SSE 实时仪表盘

后台任务的节奏:

```text
每 2 秒：collect_system
  → 更新共享的"最新快照"
  → broadcast 推给所有 SSE 订阅者（第 24 章）
  → evaluate 告警规则
```

浏览器 `EventSource` 连上 `/stream` 就持续收到快照 JSON,实时更新页面(示例用纯 CSS 进度条,不引图表库;生产用 Grafana 或 ECharts)。新连接一上来先补发当前帧,不用等下一个周期才有数据。

## 28.5 告警规则

```rust
struct Rule { metric: String, op: Op /* Gt/Lt/Ge/Le */, threshold: f64, severity: String }
fn evaluate(rules, metrics) -> Vec<Alert>
```

`evaluate` 是纯函数:逐条规则扫同名指标,超阈值就产生带 severity 的 Alert(多块磁盘各出一条,带各自 label)。默认规则:CPU>90 / 内存>90 是 critical,磁盘>=85 是 warning。

生产告警比这复杂得多,示例注释点到三个进阶问题:**去抖动**(短暂尖刺不该立即告警)、**静默**(维护窗口内不告警)、**分组**(100 台机器同时挂,发 1 条汇总而非 100 条)——否则就是「告警风暴」,人被淹没反而漏掉真问题。

## 28.6 串联:采集驱动与告警通道

- **采集驱动**:示例用自己的循环,生产可用第 21 章定时任务统一调度
- **告警送达**:`evaluate` 产生 Alert 后,通过第 14 章消息队列异步发通知(邮件/钉钉/webhook)——发通知是慢操作,不能卡住采集循环

```bash
cd examples-middleware
cargo run -p monitor_demo    # 浏览器看实时 CPU/内存，触发阈值看 /alerts
```

## 本章小结

- 监控两条出口:`/metrics` 给 Prometheus 抓取,`/stream`(SSE)给浏览器实时看
- sysinfo 采 CPU 要两次采样求差,首次恒为 0;采集要快不阻塞
- Prometheus 文本格式是事实标准,gauge(瞬时)vs counter(累计),label 要转义
- SSE 仪表盘:后台每 2 秒采集 → broadcast → 浏览器实时刷新,新连接先补发当前帧
- 告警规则纯函数判定 + severity 分级;生产要去抖/静默/分组防告警风暴
- 采集可接第 21 章定时任务,告警送达走第 14 章消息队列异步化

**自测清单**

- [ ] 我能说出监控两条出口分别服务什么消费者
- [ ] 我能解释 sysinfo 读 CPU 为什么要两次采样
- [ ] 我能说出 Prometheus 用文本标准格式的好处,gauge 与 counter 的区别
- [ ] 我能描述 SSE 仪表盘的数据流和"新连接补发当前帧"的意义
- [ ] 我能写一条告警规则并说清 severity 的作用
- [ ] 我能说出告警风暴的三个对策

## 练习

1. 加业务指标:给第 12 章的 todo_api 埋点「每分钟请求数」和「p99 延迟」,一并暴露到 `/metrics`。
2. 把告警送达接上第 14 章 `mq_rabbit`:Alert 产生后投消息队列,由 worker 发通知,验证采集循环不被阻塞。
3. 实现去抖动:某指标连续 N 次超阈值才告警,恢复后连续 M 次正常才消警。
4. 用真正的 Prometheus + Grafana(Docker)抓取本示例的 `/metrics`,画一张 CPU 曲线——体会标准格式的生态红利。

**动手验证**：

```bash
cd examples-middleware
cargo test -p monitor_demo    # Prometheus 渲染 + 告警规则判定的断言，默认全跑
```

---

导航：[上一章](27-certificate-acme.md) | [返回目录](../README.md) | [下一章](29-appstore-plugins.md)
