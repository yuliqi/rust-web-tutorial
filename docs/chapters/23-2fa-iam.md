# 第 23 章：两步验证与层级账号 — 企业级 IAM

> 示例工程：`examples-middleware/iam_demo`（需要 Postgres）
>
> 把第 17 章的扁平「租户 + admin/member」升级为真实 SaaS 的身份体系:
> 两步验证(2FA)、组织下的层级子账号、按资源的细粒度授权。

## 学习目标

1. 理解并实现 TOTP 两步验证,包括「半程 token」的两步登录流程
2. 用自引用表建层级账号树(组织 → 主账号 → 子账号)
3. 实现资源级授权(RBAC 之上的细粒度),掌握「默认拒绝」原则
4. 理解层级授权的核心不变量:子账号权限不能超过父账号

## 23.1 两步验证:密码之外的第二把锁

密码会被钓鱼、撞库、泄露。两步验证(2FA)要求登录时再提供一个「手机上动态生成的 6 位码」,即使密码泄露,攻击者没有你的手机也进不来。主流方案是 **TOTP**(基于时间的一次性密码,Google/Microsoft Authenticator 都用它):

```text
绑定:服务端生成随机 secret → 编码成 otpauth:// URI → 前端渲染二维码 → 用户 App 扫码保存
登录:App 用 secret + 当前时间每 30 秒算出一个 6 位码 → 服务端用同样的 secret 和时间验证
```

关键点(源码注释都讲了):

- **secret 是共享密钥**,双方各自用它 + 时间算码,过程不联网。时间步长 30 秒,验证时**允许前后一个窗口**容忍手机与服务器的时钟漂移
- **secret 本身是敏感数据**,落库应加密(呼应第 20 章)——拿到 secret 等于能生成有效码
- **恢复码**:手机丢了要有救。生成一批一次性恢复码,哈希存库,用一次即失效

## 23.2 两步登录:半程 token

开了 2FA 后,登录不再是一步。`iam_demo` 的做法是「半程 token」:

```text
POST /auth/login  {email, password}
  → 密码对 + 未开 2FA:直接发正式 JWT
  → 密码对 + 开了 2FA:只发一个 mfa_pending 的半程 token（短寿命、进不了任何受保护路由）

POST /auth/login/totp  {code}  （带半程 token）
  → 码对:换发正式 JWT
  → 码错:401
```

半程 token 用 JWT 的 claims 标记 `mfa_pending`,受保护路由的提取器会拒绝它——**密码只是拿到"待验证"状态,过了第二步才是真身份**。

## 23.3 层级账号:一张自引用表

真实 SaaS 里,一个组织有主账号(owner),主账号可以建子账号,子账号还能再建下级。这种树形结构用**自引用外键**表达:

```sql
accounts(
  id, org_id,
  parent_id BIGINT NULL REFERENCES accounts,  -- owner 的 parent_id 为 NULL，子账号指向创建者
  email UNIQUE, password_hash, role,
  totp_secret_enc NULL, totp_enabled
)
```

`parent_id` 自己指向自己这张表,就长出了账号树。查询隔离照旧按 `org_id`(第 17 章的多租户红线)。

## 23.4 资源级授权:RBAC 之上的细粒度

第 17 章的 role(admin/member)是**粗粒度**——同一个 role 权限一刀切。但资产管理系统需要「这个子账号只能看云账号 A 的资产,那个只能看 B」这种**按具体资源**的授权。`iam_demo` 用一张 grants 表:

```sql
grants(account_id, resource_type, resource_id, action)
-- 例：(子账号7, "cloud-account", "A", "read")
-- resource_id='*' 或 action='*' 表示通配
```

判定函数 `can()` 的规则是**默认拒绝**:

```text
can(grants, resource_type, resource_id, action):
  resource_type 必须精确匹配
  resource_id、action 支持 '*' 通配
  任一 grant 命中 → 允许；一条都不中 → 拒绝
```

「默认拒绝」是授权系统的安全底线——没有明确授予的,一律不给。

## 23.5 核心不变量:子不越父

层级授权最容易出错、也最关键的一条:**子账号的权限不能超过父账号**。否则主账号建个子账号就能"提权",整个隔离形同虚设。`iam_demo` 用两道防线:

```text
1. 判定侧 effective_grants(child, parent) = 子 grants ∩ 父 grants
   只保留「父账号覆盖得住」的子 grant。
   注意通配方向：子要 '*' 时，父也必须是 '*' 才算覆盖——
   具体的父盖不住通配的子（父只有 A，子却要全部，不行）。

2. 写入侧 Authority 二次设防：建子账号/授权时，
   owner 权限无限，其余账号只能授出自己 grants 覆盖的范围，
   索要超父权限 → 403 Forbidden。
```

判定侧 + 写入侧双重保证,「子不越父」这个不变量任何路径都破不了。

```bash
cd examples-middleware
docker compose up -d postgres
cargo run -p iam_demo    # 建 org 主账号 → 开 TOTP 自测 → 建两子账号分授 A/B → 演示越权被拒
```

## 本章小结

- 2FA 用 TOTP:secret 共享、按时间算码、验证容忍时钟漂移;secret 要加密存,配恢复码
- 两步登录用半程 token:密码过关只给 mfa_pending 状态,第二步验码才发正式 JWT
- 层级账号用自引用表 `parent_id`,组织内按 org_id 隔离
- 资源级授权(grants 表)是 RBAC 粗粒度之上的细粒度,判定遵循「默认拒绝」
- 核心不变量「子不越父」:判定侧取子∩父 + 写入侧 Authority 双重设防
- 通配方向要小心:具体的父盖不住通配的子

**自测清单**

- [ ] 我能画出 TOTP 绑定与验证的流程,并说出为什么容忍时钟漂移
- [ ] 我能解释半程 token 在两步登录里的作用
- [ ] 我能用自引用表建账号层级,并说明查询怎么隔离
- [ ] 我能解释资源级授权与 role 的区别,以及「默认拒绝」
- [ ] 我能说清「子不越父」为什么关键、怎么两道防线保证
- [ ] 我能解释通配方向「具体的父盖不住通配的子」

## 练习

1. 给 `iam_demo` 的 TOTP secret 落库加上第 20 章的字段加密(现在是明文占位),真正做到"泄库也拿不到 secret"。
2. 实现"记住这台设备 30 天":2FA 通过后发一个设备信任 token,期内该设备免二步——思考它和安全性的权衡。
3. 把授权模型扩展成支持"角色模板":定义几个预设的 grant 集合(如"只读运维""资产管理员"),建子账号时套用模板而非逐条授权。
4. 给 `cloud_sync`(第 22 章)接上本章授权:同步某云账号的资产前,先 `can(grants, "cloud-account", id, "read")` 判定,把两个综合项目打通。

**动手验证**：

```bash
cd examples-middleware
docker compose up -d postgres
cargo test -p iam_demo -- --ignored   # 层级隔离/越权被拒/两步登录的完整断言
```

---

导航：[上一章](22-cloud-asset-sync.md) | [返回目录](../README.md)
