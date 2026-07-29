# 第 16 章：部署与上线 — 从能跑到能上生产

> 配套产物（都有逐行注释）：
> - `examples-middleware/todo_api_pg/Dockerfile` —— 多阶段构建
> - `examples-middleware/deploy/k8s/todo-api-pg.yaml` —— k8s 最小清单
> - `examples-middleware/deploy/ci.yml` —— CI 质量门禁
> - `examples-middleware/todo_api_pg` 源码 —— 优雅停机（main.rs）与双探针（routes/health.rs）已实装

## 学习目标

1. 能产出一个小体积、非 root 运行的生产镜像，说清多阶段构建的缓存原理
2. 理解 liveness / readiness 两种探针的分工，知道优雅停机的完整链路
3. 能读懂并改写最小 k8s 清单：副本、滚动更新、探针、资源、Secret
4. 会搭 CI 质量门禁，知道集成测试怎么在 CI 里跑真实中间件
5. 有一张上线前检查表：配置、密钥、日志、监控、安全

## 16.1 编译产物：release 不只是 `--release`

```toml
# Cargo.toml（workspace 级）
[profile.release]
lto = "thin"        # 跨 crate 内联优化，Web 服务常见 5-15% 提升
strip = true        # 去掉调试符号，二进制体积减半以上
panic = "abort"     # 可选：panic 直接终止，体积更小；依赖 catch_unwind 的库慎用
```

Rust 的部署优势在这里兑现：**产物是单个静态二进制**，没有解释器、没有 node_modules、没有 JVM。这决定了它的镜像可以做到几十 MB。

## 16.2 Docker 多阶段构建

`todo_api_pg/Dockerfile` 的两个关键设计，都是生产必备：

**1. 依赖层缓存**。Rust 编译慢，慢在依赖。先拷 `Cargo.toml`/`Cargo.lock` + 空 main 编译一遍依赖，再拷真实源码——只要依赖清单没变，改业务代码就能命中缓存，构建从十分钟降到几十秒。生产工程常用 `cargo-chef` 把这套做得更彻底。

**2. builder 与 runtime 分离**。编译要完整工具链（~2GB），运行只要二进制 + ca-certificates（本教程实测 158MB）。小镜像不只是省磁盘：拉取快（扩容速度）、攻击面小（镜像里没有编译器和 shell 工具可供利用）。

```bash
cd examples-middleware
docker build -f todo_api_pg/Dockerfile -t todo-api-pg .
# 挂进 compose 网络，用服务名 postgres 访问数据库——
# 这与 k8s 里通过 Service 名互访是同一种语义（DNS 服务发现）
docker run --rm --network examples-middleware_default -p 3001:3001 \
  -e DATABASE_URL=postgres://tutorial:tutorial@postgres:5432/todos \
  todo-api-pg
```

两个容易踩的细节，Dockerfile 注释里都点了：容器内要监听 `0.0.0.0`（`127.0.0.1` 出不了容器）；用**非 root 用户**跑服务。

## 16.3 配置与密钥

- **配置外置**（12-factor）：一切随环境变化的值走环境变量——`todo_api_pg` 的 `Config::from_env` 从第一天就是这么设计的。代码里的默认值只是本地开发兜底。
- **密钥不进镜像、不进仓库**：`DATABASE_URL` 里有密码，它只能来自运行时注入（k8s Secret、云厂商密钥管理）。检查方式很简单：`docker history` 和 `git log -p` 里搜不到密码才算合格。
- **区分配置与密钥**：端口、日志级别可以进 ConfigMap/编排文件；连接串、token 必须走 Secret 通道。

## 16.4 探针与优雅停机：滚动更新不丢请求的完整链路

这是部署里最容易被忽略、又最影响线上质量的一环。`todo_api_pg` 已实装：

**两种探针分工**（`routes/health.rs`）：

| 探针 | 端点 | 失败的后果 | 检查什么 |
|------|------|-----------|---------|
| liveness | `/health` | **重启**容器 | 进程活着即可，**绝不碰依赖** |
| readiness | `/health/ready` | 只**摘除流量** | 真探数据库（SELECT 1） |

为什么不能混用：数据库抖动时，如果 liveness 也探库，k8s 会不停重启一个本来健康的进程，把小故障放大成雪崩；readiness 只是暂时不给它流量，数据库恢复后流量自动回来。

**优雅停机链路**（`main.rs` 的 `shutdown_signal`）：

```text
k8s 决定停止实例
  → Pod 从 Service 摘除（readiness 失效，新流量不再进来）
  → 容器收到 SIGTERM
  → axum with_graceful_shutdown：停止接收新连接，送完在途请求
  → 正常退出；若超过 terminationGracePeriodSeconds（清单里设 30s）才强杀
```

没有这条链，每次发版都会切断一批用户的在途请求。本地验证：`cargo run -p todo_api_pg`，压一个慢请求的同时 Ctrl-C，观察它先送完响应再退出。

## 16.5 Kubernetes 最小清单

`deploy/k8s/todo-api-pg.yaml` 的每个字段都值得读一遍注释，骨架是：

- **replicas: 2** + **maxUnavailable: 0**：任何时刻都有满编容量，滚动更新零感知
- **image 用不可变 tag**（git sha），`latest` 是生产事故的经典源头（你永远不知道跑的是哪个版本，也没法回滚）
- **resources**：requests 决定调度，limits 兜住失控实例；Rust 服务内存占用低且稳定，这里是它相对 JVM 系的显著优势
- **Secret 注入 DATABASE_URL**：呼应 16.3
- **Service**：第 15 章讲的服务发现在 k8s 的开箱形态——集群内 `http://todo-api-pg:3001` 直接可用

发布策略从滚动更新起步就够；金丝雀（先放 5% 流量到新版本）和蓝绿属于下一阶段，概念先挂个号。

## 16.6 CI：质量门禁自动化

`examples-middleware/deploy/ci.yml` 把第 8 章的三件套（fmt / clippy `-D warnings` / test）变成每次 push 的强制关卡，两个值得学的点：

- **依赖缓存**（rust-cache）：CI 提速的第一杠杆，原理同 Docker 的依赖层缓存
- **service 容器**：CI 里起真实 Postgres/Redis，把本地标 `#[ignore]` 的集成测试也跑起来——「离线单测 + 在线集成测试」两层结构在 CI 里完整落地

CD（自动部署）的形态依平台差异大，通用套路是：CI 通过 → build & push 镜像（tag 用 git sha）→ 更新部署清单里的 tag → 滚动更新。

## 16.7 上线前检查表（补齐 14.7 的部署侧）

- [ ] 镜像：多阶段构建、非 root、不可变 tag、密钥不进镜像
- [ ] 探针：liveness 不碰依赖，readiness 探依赖，两者端点分离
- [ ] 停机：处理 SIGTERM + graceful shutdown，grace period 覆盖最长请求
- [ ] 日志：结构化输出到 stdout（容器约定），不写本地文件；错误详情进日志、不进响应
- [ ] 监控：至少有请求量/延迟/错误率三大指标的看板与告警（Rust 生态：`metrics` + Prometheus、`tracing-opentelemetry`；本教程未展开，关键词先记住）
- [ ] 鉴权：对外接口有认证（axum 生态：tower 中间件 + `jsonwebtoken` 做 JWT；见第 12 章扩展作业）
- [ ] 限流：入口层兜底（`tower::limit` / 网关限流），防止单客户端打挂服务
- [ ] 回滚：部署系统能一键回到上一个镜像 tag，且数据库迁移向后兼容

## 本章小结

- Rust 部署的底牌是单个静态二进制：镜像几十 MB、冷启动毫秒级、内存低且稳定
- 多阶段构建两件事：依赖层缓存解决编译慢，builder/runtime 分离解决镜像大
- liveness 管「要不要重启」，readiness 管「要不要给流量」，混用会放大故障
- 优雅停机 = 摘流量 → SIGTERM → 排空在途请求 → 退出，缺一环发版就丢请求
- k8s 清单四要素：多副本滚动更新、不可变 tag、资源声明、Secret 注入
- CI 门禁 = fmt + clippy -D warnings + 测试，service 容器让集成测试也自动化
- 上线检查表比记忆可靠：镜像、探针、停机、日志、监控、鉴权、限流、回滚

**自测清单**

- [ ] 我能解释 Dockerfile 里「先拷 Cargo.toml 编译空 main」这层的作用
- [ ] 我能说出 liveness 探针为什么绝不能检查数据库
- [ ] 我能画出滚动更新时一个请求不被切断所依赖的完整链路
- [ ] 我能说出 `latest` tag 在生产的两个具体危害
- [ ] 我能解释 requests 和 limits 各自影响什么
- [ ] 我能列出上线检查表里至少六项

## 练习

1. 本地跑通镜像：`docker build` 构建 `todo-api-pg`，用 `docker run` 连上 compose 里的 Postgres，验证 `/health/ready` 在数据库停掉时返回 503、恢复后转 200。
2. 给 `todo_api`（SQLite 版）也写一个多阶段 Dockerfile，对比：它不需要外部数据库，镜像里带一个 volume 挂载的 db 文件——体会「有状态」给部署带来的额外复杂度（这正是 15.1 无状态化的反面教材）。
3. 优雅停机实测：用 `curl` 发一个慢请求（可临时在 handler 里加 sleep），同时 `kill -TERM` 进程，验证响应完整返回；去掉 `with_graceful_shutdown` 再试一次对比。
4. 把 CI 里的 clippy 换成 `--deny clippy::unwrap_used`，看看示例代码有多少处要改——体会「生产代码禁 unwrap」这条团队规范怎么落地。

---

导航：[上一章](15-cluster-distributed.md) | [返回目录](../README.md) | [下一章](17-multitenancy-auth.md)
