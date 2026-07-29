# 中间件示例（第 14–15 章配套）

与 `../examples` 分开的独立 Cargo workspace：这里的示例依赖真实中间件服务
（PostgreSQL / MySQL / Redis / RabbitMQ / etcd），单独成 workspace 避免拖慢主教程的编译与测试。

## crate ↔ 章节索引

| crate | 章节 | 学什么 |
|-------|------|--------|
| `todo_api_pg` | [第 14.3 节](../chapters/14-middleware-production.md) | SQLite → PostgreSQL 迁移、`RETURNING`、连接池 |
| `mysql_demo` | [第 14.4 节](../chapters/14-middleware-production.md) | sqlx + MySQL 方言差异 |
| `redis_cache` | [第 14.5 节](../chapters/14-middleware-production.md) | cache-aside 模式、TTL、缓存三坑 |
| `mq_rabbit` | [第 14.6 节](../chapters/14-middleware-production.md) | RabbitMQ 生产/消费、ack、幂等 |
| `cluster_demo` | [第 15 章](../chapters/15-cluster-distributed.md) | etcd 服务发现、客户端负载均衡、Redis 分布式锁 |
| `todo_api_saas` | [第 17](../chapters/17-multitenancy-auth.md)–[18 章](../chapters/18-quota-billing-api.md) | 多租户隔离、JWT/RBAC、配额限流、幂等键、计费 webhook |
| `api_crypto` | [第 19 章](../chapters/19-api-encryption.md) | 接口报文加密：RSA+AES 混合信封、Web Crypto 前端（无需 Docker） |
| `db_crypto` | [第 20 章](../chapters/20-data-at-rest.md) | 数据库字段级 AES-GCM 加密、HMAC 盲索引、密钥版本轮换 |
| `scheduler_demo` | [第 21 章](../chapters/21-scheduled-jobs.md) | 定时任务：interval/cron 调度、分布式锁+唯一约束集群去重、优雅停机 |
| `cloud_sync` | [第 22 章](../chapters/22-cloud-asset-sync.md) | 多云资产同步：CloudProvider trait、加密凭证、归一化 upsert、限流+锁 |
| `iam_demo` | [第 23 章](../chapters/23-2fa-iam.md) | 两步验证(TOTP)、层级子账号、资源级授权(子不越父) |
| `realtime_demo` | [第 24 章](../chapters/24-realtime-websocket.md) | WebSocket 双向 + SSE 单向推送、broadcast 广播、心跳（无需 Docker） |
| `license_demo` | [第 25 章](../chapters/25-software-license.md) | 软件许可：离线 ed25519 签名授权 + 在线激活/心跳/吊销（无需 Docker） |
| `webterm_demo` | [第 26 章](../chapters/26-web-terminal-bastion.md) | Web 终端/堡垒机：PTY over WebSocket、会话录制、访问审计 |
| `cert_demo` | [第 27 章](../chapters/27-certificate-acme.md) | 证书管理：ACME 下单、HTTP-01 挑战、续期判定（离线核心可跑） |
| `monitor_demo` | [第 28 章](../chapters/28-realtime-monitoring.md) | 实时监控：sysinfo 采集、Prometheus 格式、SSE 仪表盘、阈值告警 |
| `plugin_demo` | [第 29 章](../chapters/29-appstore-plugins.md) | 应用商店(compose 模板渲染)+ 插件机制(子进程 stdio JSON-RPC)（无需 Docker） |

## 使用方式

```bash
# 1. 启动服务（需要 Docker）
docker compose up -d          # 全部；或 up -d redis 只起一个
docker compose ps             # 等待全部 healthy

# 2. 离线部分：编译 + 单元测试（不需要服务也能跑）
cargo test --workspace

# 3. 在线部分：真连服务的集成测试（标了 #[ignore]）
cargo test -p todo_api_pg -- --ignored
cargo test -p redis_cache -- --ignored
cargo test -p mysql_demo -- --ignored
cargo test -p mq_rabbit -- --ignored
cargo test -p cluster_demo -- --ignored
cargo test -p todo_api_saas -- --ignored
cargo test -p db_crypto -- --ignored
cargo test -p scheduler_demo -- --ignored
cargo test -p cloud_sync -- --ignored
cargo test -p iam_demo -- --ignored
cargo test -p webterm_demo -- --ignored
cargo test -p monitor_demo -- --ignored

# 4. 可运行演示
cargo run -p todo_api_pg      # PostgreSQL 版 Todo API（127.0.0.1:3001）
cargo run -p redis_cache      # 缓存命中前后耗时对比
cargo run -p mq_rabbit        # 发 3 条消息并消费
cargo run -p cluster_demo     # 注册/发现/轮询/抢锁演示
cargo run -p todo_api_saas    # SaaS 版 Todo API（127.0.0.1:3003，种子账号见启动日志）
cargo run -p api_crypto       # 报文加密演示（127.0.0.1:3004，浏览器打开，无需 Docker）
cargo run -p db_crypto        # 数据库字段加密演示（打印库里的密文行 + 盲索引查询 + 轮换）
cargo run -p scheduler_demo   # 定时任务演示（两个实例，看谁抢到锁执行、job_runs 只落一条）
cargo run -p cloud_sync       # 多云同步演示（三朵云凭证 → 同步 → 归一化资产 → upsert）
cargo run -p iam_demo         # IAM 演示（开 TOTP + 建层级子账号 + 越权被拒）
cargo run -p realtime_demo    # 实时通信演示（127.0.0.1:3005，开两个浏览器标签看广播，无需 Docker）
cargo run -p license_demo     # 软件许可演示（签发/验证/篡改被拒/在线激活/吊销，无需 Docker）
cargo run -p webterm_demo     # Web 终端/堡垒机演示（127.0.0.1:3006，浏览器敲命令，会话入库审计）
cargo run -p cert_demo        # 证书管理演示（生成自签证书 → 解析到期 → 续期判定，无需 Docker）
cargo run -p monitor_demo     # 实时监控演示（127.0.0.1:3008，浏览器看 CPU/内存实时刷新）
cargo run -p plugin_demo      # 应用商店/插件演示（渲染 compose + spawn 样例插件调用，无需 Docker）

# 5. 学完清场
docker compose down -v
```

## 部署产物（第 16 章配套）

| 文件 | 用途 |
|------|------|
| `todo_api_pg/Dockerfile` | 多阶段构建的生产镜像（依赖层缓存 + 非 root 运行） |
| `.dockerignore` | 构建上下文瘦身（target/ 绝不能进上下文） |
| `deploy/k8s/todo-api-pg.yaml` | k8s 最小清单：双副本滚动更新 + 双探针 + Secret |
| `../.github/workflows/ci.yml` | CI 门禁：fmt/clippy/test + service 容器跑集成测试 |

```bash
# 构建并本地试跑生产镜像（挂进 compose 网络，用服务名访问 Postgres）
docker build -f todo_api_pg/Dockerfile -t todo-api-pg .
docker run --rm --network examples-middleware_default -p 3001:3001 \
  -e DATABASE_URL=postgres://tutorial:tutorial@postgres:5432/todos \
  todo-api-pg
```

`todo_api_pg` 已实装优雅停机（SIGTERM → 排空在途请求）与双探针
（`/health` 存活、`/health/ready` 就绪探库），细节见第 16 章。

约定与主 workspace 相同：Rust 1.97.1（根目录 rust-toolchain.toml 自动生效）、edition 2024。
compose 里全部是弱口令，仅供本地学习。
