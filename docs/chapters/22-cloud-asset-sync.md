# 第 22 章：多云资产同步引擎 — SaaS 综合实战

> 示例工程：`examples-middleware/cloud_sync`（需要 Postgres + Redis）
>
> 这是一个「虚拟资产管理 SaaS」的心脏：租户填入各云厂商的 API 凭证，
> 系统定时用凭证拉取该账号下的云资产、归一化入库。本章把前面几章的机制总装成一个真实产品能力。

## 学习目标

1. 用 trait 抽象多个云厂商，做到「加一朵云 = 加一个实现」
2. 理解「归一化统一模型」为什么是资产管理系统的核心价值
3. 把加密存凭证、定时同步、集群去重、限流串成一条同步流水线
4. 掌握 upsert（`ON CONFLICT`）实现「同步不重复插入」

## 22.1 需求：把散落各云的资产收拢到一处

企业往往同时用阿里云、腾讯云、AWS、Cloudflare、私有云。资产（服务器、存储桶、域名）散在各家控制台,没有统一视图。资产管理 SaaS 要做的:**拿着租户提供的只读凭证,定时拉取各云资产,归一化后存进自己的库,给出统一视图**。

这个需求几乎用上了本书后半的每一块:

| 能力 | 复用章节 |
|------|---------|
| 多云抽象 | 第 6 章 trait / trait 对象 |
| 加密存 AK/SK | 第 20 章 字段加密 + 盲索引 |
| 定时拉取 | 第 21 章 定时任务 + 集群去重 |
| 保护云 API 配额 | 第 18 章 限流 |
| 多租户隔离 | 第 17 章 tenant_id |

## 22.2 多云抽象：一个 trait，多个实现

各云的 API 千差万别,但对上层业务,它们都只需回答一个问题:「这个账号下有哪些资产?」——这正是 trait 的用武之地(第 6 章):

```rust
trait CloudProvider {
    fn name(&self) -> &str;
    fn list_assets(&self, cred: &Credential)
        -> Pin<Box<dyn Future<Output = Result<Vec<CloudAsset>>> + Send + '_>>;
}
```

`cloud_sync` 里有三个 mock 实现:`AliyunProvider`(ECS + OSS)、`AwsProvider`(EC2)、`PrivateCloudProvider`(自定义 endpoint 的 VM)。真实实现里 `list_assets` 会用 `reqwest`(第 11 章)调各家 OpenAPI 并签名;mock 返回结构合理的假数据,让示例**离线可跑可测**。

> **一个 edition 2024 的坑**:原生的 async fn in trait 不满足 dyn 兼容,而注册表要用 `Box<dyn CloudProvider>` 做动态分发。解法是把返回类型手写成 `Pin<Box<dyn Future>>`,每个实现用 `Box::pin(async move { ... })` 收尾——既保住动态分发,又不必引入 `async-trait` 宏。源码注释详述了这个取舍。

新增一朵云 = 加一个 `impl CloudProvider` + 在 `provider_registry` 注册,别处代码不用动——这就是开闭原则的落地。

## 22.3 归一化:统一模型才是核心价值

阿里云叫 `DescribeInstances`、AWS 叫 `describe-instances`、字段名和结构全不一样。资产管理系统的价值,恰恰在于把它们**归一化成一个统一模型**:

```rust
struct CloudAsset {
    provider: String,       // "aliyun" / "aws" / "private"
    asset_type: String,     // "ecs" / "ec2" / "vm"
    external_id: String,    // 该云里的资源 id
    name: String,           // 统一的展示名（AWS 要从 Tags 里抽 Name）
    region: String,
    raw: serde_json::Value, // 保留原始响应，不丢信息
}
```

`name` 字段的处理最能说明问题:AWS EC2 没有直接的 name,要从 `Tags` 里找 `Key=Name` 的那条抽出来——**归一化就是把各家的差异吸收在 provider 实现里,让上层看到整齐划一的数据**。`raw` 保留原始 JSON,避免归一化丢信息。

## 22.4 同步流水线:一条链串起五章

`sync_tenant` 是整个引擎的主干:

```text
1. 抢分布式锁 lock:sync:{tenant}     ← 第21章：多实例只有一个真正同步
2. 解密加载该租户的所有凭证           ← 第20章：AK/SK 加密存储
   （查询带 WHERE tenant_id）         ← 第17章：多租户隔离
3. 逐个 provider：
     先过 Redis 限流闸                ← 第18章：别把云 OpenAPI 打到限额
     再 list_assets 拉取
4. 归一化后 upsert 进 assets 表：
     ON CONFLICT (tenant_id, provider, external_id) DO UPDATE
5. 解锁，返回 SyncReport（新增/更新/总数）
```

第 4 步的 upsert 是「同步」的关键:同一个资产第二次同步时,应该**更新**而不是**重复插入**——靠 `(tenant_id, provider, external_id)` 唯一约束 + `ON CONFLICT DO UPDATE` 实现,还能用 `RETURNING (xmax = 0)` 区分这次是新增还是更新。

安全红线(源码注释标出):云 AK/SK 泄露 = 整个云账号失守,比第 20 章的手机号严重得多——必须加密存储、用只读最小权限凭证、定期轮换。

```bash
cd examples-middleware
docker compose up -d postgres redis
cargo run -p cloud_sync    # 存三朵云凭证 → 同步 → 看归一化后的资产混在一张表里 → 再同步演示 upsert
```

## 本章小结

- 多云抽象用 trait:`CloudProvider` 一个接口,每朵云一个实现,新增云不改上层
- edition 2024 下 dyn async trait 用 `Pin<Box<dyn Future>>` 手写,避开 async-trait 依赖
- 归一化成统一 `CloudAsset` 模型是资产管理系统的核心价值,差异吸收在 provider 实现里
- 同步流水线串起五章:分布式锁去重 + 加密凭证 + 多租户隔离 + 限流 + upsert
- upsert（`ON CONFLICT DO UPDATE`）让重复同步变成更新而非重复插入
- 云凭证是最高敏感级数据:加密存 + 最小权限 + 轮换

**自测清单**

- [ ] 我能解释为什么用 trait 抽象云厂商,以及"加一朵云"要改什么
- [ ] 我能说出归一化统一模型解决了什么问题
- [ ] 我能画出 sync_tenant 流水线并指出每步复用了哪一章
- [ ] 我能解释 upsert 如何实现"同步不重复"
- [ ] 我能说出为什么同步前要抢分布式锁、调用前要限流
- [ ] 我能说出云 AK/SK 为什么是最高敏感级、怎么存

## 练习

1. 加一朵"腾讯云"provider:实现 `CloudProvider` 并注册,mock 返回 CVM 资产,验证上层同步代码一行不用改。
2. 把同步做成定时任务:用第 21 章的 `scheduler_demo` 每 5 分钟触发一次 `sync_tenant`,重任务(真拉取)投进 `mq_rabbit`(第 14 章)由 worker 执行。
3. 给资产加"变更检测":同步时对比新旧,记录哪些资产是新增/删除/配置变化,存一张 `asset_changes` 表。
4. 接入第 17 章的 JWT 鉴权,让 `POST /sync/{tenant_id}` 只能由该租户的账号触发。

**动手验证**：

```bash
cd examples-middleware
docker compose up -d postgres redis
cargo test -p cloud_sync -- --ignored   # 同步 + upsert 不翻倍 + 跨租户隔离
```

---

导航：[上一章](21-scheduled-jobs.md) | [返回目录](../README.md) | [下一章](23-2fa-iam.md)
