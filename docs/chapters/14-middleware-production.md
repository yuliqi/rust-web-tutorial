# 第 14 章：生产中间件实战 — PostgreSQL / MySQL / Redis / 消息队列

> 示例工程：`examples-middleware/`（独立 workspace，与主示例分开编译）
>
> 本章需要 Docker：示例依赖真实的数据库/缓存/消息队列，用 `docker compose` 一键启动。

## 学习目标

1. 能把 SQLite 版 todo_api 迁移到 PostgreSQL / MySQL，说清三者的方言差异
2. 会用 Redis 实现 cache-aside 缓存模式，知道穿透/击穿/雪崩各自的对策
3. 理解消息队列解决什么问题，能用 RabbitMQ 写出生产者/消费者闭环
4. 建立一张「生产常用中间件」的选型地图，知道每类该用哪个 Rust crate

## 14.1 选型速览：生产上你会遇到什么

| 类别 | 常见选择 | Rust crate | 本教程覆盖 |
|------|---------|-----------|-----------|
| 关系数据库 | PostgreSQL / MySQL | `sqlx`（同一套 API） | ✅ 14.3 / 14.4 |
| 缓存 | Redis | `redis` | ✅ 14.5 |
| 消息队列 | RabbitMQ / Kafka / RocketMQ | `lapin` / `rdkafka` | ✅ 14.6（RabbitMQ） |
| 文档数据库 | MongoDB | `mongodb` | 提及 |
| 搜索 | Elasticsearch / Meilisearch | `elasticsearch` / `meilisearch-sdk` | 提及 |
| 对象存储 | S3 / OSS / MinIO | `aws-sdk-s3` / `opendal` | 提及 |
| 服务发现/配置 | etcd / consul / Nacos | `etcd-client` 等 | ✅ 第 15 章 |

选型的第一性原则：**没有明确痛点前，先用最少的组件**。SQLite → Postgres 是因为需要并发写和网络访问；加 Redis 是因为出现了热点读；上消息队列是因为出现了「慢操作拖垮请求」或「服务间解耦」的需求。倒着上中间件只会增加运维负担。

## 14.2 环境准备：docker compose

```bash
cd examples-middleware
docker compose up -d          # 全部启动（Postgres/MySQL/Redis/RabbitMQ）
docker compose up -d redis    # 也可以只启动本节需要的
docker compose ps             # 看健康状态（等 healthy 再跑测试）
docker compose down -v        # 学完清场：停止并删除数据
```

本章示例的连接参数全部与 compose 文件对齐（弱口令仅供本地学习）：

| 服务 | 地址 | 账号 |
|------|------|------|
| PostgreSQL | `localhost:5432` | tutorial / tutorial，库 `todos` |
| MySQL | `localhost:3306` | root / tutorial，库 `todos` |
| Redis | `localhost:6379` | 无密码 |
| RabbitMQ | `localhost:5672` | guest / guest（管理界面 <http://localhost:15672>） |

示例的设计约定（与练习支架同一套模式）：**编译和单元测试不需要任何服务**；真连服务的集成测试标了 `#[ignore]`，起好 Docker 后用 `-- --ignored` 运行：

```bash
cargo test -p todo_api_pg                  # 离线：编译 + 单测
cargo test -p todo_api_pg -- --ignored     # 在线：真连 Postgres 的集成测试
```

## 14.3 PostgreSQL：把 todo_api 迁过去（`todo_api_pg`）

生产上单机文件库撑不住并发写和多实例部署，Postgres 是最常见的第一站。sqlx 的好处是**换库不换 API**：`SqlitePool` → `PgPool`，查询函数一个不用改名。真正要动的是 SQL 方言：

| | SQLite | PostgreSQL |
|---|--------|------------|
| 占位符 | `?` | `$1, $2, …` |
| 自增主键 | `INTEGER PRIMARY KEY AUTOINCREMENT` | `BIGSERIAL PRIMARY KEY` |
| 布尔 | `INTEGER` 0/1（sqlx 帮你转） | 原生 `BOOLEAN` |
| 插入后拿 id | `last_insert_rowid()` + 回查 | `INSERT … RETURNING` 一步到位 |
| 部分更新 | 先 SELECT 再 UPDATE（有竞态） | `UPDATE … RETURNING` 原子完成 |

最后一行值得展开：第 12 章的 SQLite 版 `update` 是「先读后写」两条语句，并发 PATCH 会丢失更新；Postgres 的 `RETURNING` 让读写合成一条原子语句 —— 这是「换更强的数据库解锁更正确的写法」的典型例子，`todo_api_pg/src/services/todos.rs` 里有对照注释。

连接池参数也从「随便设」变成「要算账」：`max_connections × 实例数` 不能超过数据库侧上限（Postgres 默认 100），生产还要配 `acquire_timeout` 防止排队请求无限堆积。

```bash
cargo run -p todo_api_pg     # 监听 127.0.0.1:3001（SQLite 版是 3000，可同时跑对比）
```

## 14.4 MySQL：同一套 sqlx，另一套方言（`mysql_demo`）

MySQL 在国内生产环境占有率极高，值得知道它和 Postgres 的差异：

- 占位符回到 `?`（sqlx 按数据库类型选择语法）
- **没有 `RETURNING`**：插入后拿 id 走 `last_insert_id()`，两步走，和 SQLite 版一个套路
- `BOOLEAN` 实际是 `TINYINT(1)` 的别名
- 字符串列要显式定长度（`VARCHAR(255)`），不像 SQLite/Postgres 的 `TEXT` 随意

`mysql_demo` 是一个小而全的单文件示例：连接池 + 建表 + CRUD，每个差异点上都有注释。

## 14.5 Redis：cache-aside 缓存（`redis_cache`）

读多写少的热点数据（商品详情、用户信息）每次都打数据库太浪费，标准解法是**旁路缓存（cache-aside）**：

```text
读：先查 Redis —— 命中直接返回
              —— 未命中 → 查库 → 回填 Redis（带 TTL）→ 返回
写：先更新数据库，再删除缓存（下次读自动回填新值）
```

三个必须知道的坑（示例注释里各有对策）：

- **穿透**：查不存在的 key，每次都打到库 → 缓存空值（短 TTL）或布隆过滤器
- **击穿**：热点 key 恰好过期，瞬间大量请求打库 → 互斥回源或逻辑过期
- **雪崩**：大批 key 同时过期 → TTL 加随机抖动

两个设计决策的「为什么」：TTL 是兜底（就算失效逻辑写错，数据最多错一个 TTL 周期）；写路径选「删缓存」而不是「改缓存」（改缓存在并发写下会出现旧值覆盖新值）。

```bash
cargo run -p redis_cache     # 演示第一次读慢（模拟查库）、第二次命中缓存快
```

工程上别用裸连接，示例用 `ConnectionManager`（断线自动重连）；生产另需要设超时和连接池。

## 14.6 消息队列：RabbitMQ 生产者/消费者（`mq_rabbit`）

消息队列解决三类问题：**异步化**（发邮件别让 HTTP 请求等）、**削峰**（秒杀流量先进队列慢慢消费）、**解耦**（下游服务挂了，消息先存着）。

和第 10 章的 `tokio::mpsc` 对比着理解最快：mpsc 是**进程内**通道，进程退出消息就没了；RabbitMQ 是**跨进程/跨服务**的通道，消息可持久化、消费方可以是另一台机器上的另一个语言写的服务。

`mq_rabbit` 演示最小闭环，几个概念是重点：

- **消息即契约**：用显式的 `EmailJob` 结构 + JSON，而不是裸字符串——队列两端往往是不同团队
- **durable 队列 ≠ 持久化消息**：前者保住队列定义，后者（delivery_mode=2）保住消息本身，要一起用
- **手动 ack 与 at-least-once**：消费者处理完才 ack；没 ack 就断线，消息会重投给别人——所以**消费逻辑必须幂等**（同一条消息处理两次结果不变）

```bash
cargo run -p mq_rabbit       # 发 3 条 EmailJob，消费 3 条，退出
```

Kafka（日志流/大吞吐）和 RocketMQ（国内电商系）解决类似问题但模型不同（消费位点 vs 队列删除），入门先把 RabbitMQ 的 ack/幂等吃透，概念可以平移。

## 14.7 生产清单（任何中间件都适用）

- [ ] **超时**：连接、读写、请求三层都要设；没有超时的调用就是定时炸弹
- [ ] **连接池**：算清 `池大小 × 实例数 ≤ 服务端上限`；设 acquire 超时
- [ ] **重试**：只重试幂等操作；带退避（backoff）和上限，防止重试风暴
- [ ] **健康检查**：依赖的中间件挂了，服务要能暴露出来（/health 里带依赖状态）
- [ ] **可观测**：每次中间件调用都该有 tracing span（见 todo_api 的 TraceLayer）
- [ ] **优雅停机**：收到 SIGTERM 先停止收新请求，处理完在途消息/请求再退出
- [ ] **配置外置**：连接串一律走环境变量（12-factor），示例里的默认值只是本地兜底

## 本章小结

- sqlx 换库不换 API，要换的是 SQL 方言：占位符、自增主键、`RETURNING` 支持度
- Postgres 的 `UPDATE … RETURNING` 把「先读后写」合成原子操作，消除丢失更新竞态
- MySQL 无 `RETURNING`、`BOOLEAN` 是 `TINYINT(1)`、字符串列要定长度
- cache-aside：读走「缓存→库→回填」，写走「更新库→删缓存」，TTL 永远要设
- 穿透缓存空值、击穿加互斥、雪崩加抖动
- 消息队列 = 异步化 + 削峰 + 解耦；手动 ack 带来 at-least-once，消费必须幂等
- 中间件不是越多越好：没有痛点不引入，引入就按 14.7 清单武装到牙齿

**自测清单**

- [ ] 我能列出 SQLite → Postgres 迁移要改的四类 SQL 方言差异
- [ ] 我能解释 `RETURNING` 为什么能消除部分更新的竞态
- [ ] 我能画出 cache-aside 的读写两条路径，并说出为什么是「删缓存」
- [ ] 我能说出穿透/击穿/雪崩的区别和各自对策
- [ ] 我能解释为什么手动 ack 要求消费逻辑幂等
- [ ] 我能说出连接池大小和数据库连接上限之间的约束关系

## 练习

1. 给 `todo_api_pg` 加一层 Redis 缓存：`GET /todos/{id}` 走 cache-aside，PATCH/DELETE 时删缓存（把 `redis_cache` 的做法搬过去）。
2. 给 `mq_rabbit` 的消费者加幂等保护：用 Redis `SET NX` 记录已处理的消息 id，重复消息直接 ack 跳过。
3. 把 `mysql_demo` 的建表语句改成带 `created_at TIMESTAMP` 的版本，体会三种库的时间类型差异。

**动手验证**：本章示例的集成测试都标了 `#[ignore]`，起好 Docker 后逐个跑通：

```bash
cd examples-middleware
docker compose up -d && docker compose ps   # 等全部 healthy
cargo test -p todo_api_pg -- --ignored
cargo test -p mysql_demo -- --ignored
cargo test -p redis_cache -- --ignored
cargo test -p mq_rabbit -- --ignored
```

---

导航：[上一章](13-advanced-roadmap.md) | [返回目录](../README.md) | [下一章](15-cluster-distributed.md)
