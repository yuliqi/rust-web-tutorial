# 第 17 章：SaaS 之一 — 多租户与鉴权

> 示例工程：`examples-middleware/todo_api_saas`（需要 Postgres + Redis，见第 14.2 节）
>
> 这是 `todo_api_pg` 的 SaaS 化升级：同一套分层，叠加租户隔离、JWT 登录、RBAC。
> 本章讲多租户与鉴权，第 18 章讲配额、计费与 API 治理，共用这一个示例工程。

## 学习目标

1. 说清多租户三种隔离方案的取舍，能实现「共享表 + tenant_id」并守住隔离红线
2. 会实现完整的登录链路：argon2 密码哈希 → JWT 签发 → axum 提取器验证
3. 理解 RBAC 的最小落地：角色进 claims，敏感操作按角色拦截
4. 知道跨租户访问为什么返回 404 而不是 403

## 17.1 多租户：一套代码服务一千个客户

SaaS 和普通后端的分水岭就在这：**同一套部署要同时服务大量互相不可见的客户（租户）**。三种主流隔离方案：

| 方案 | 隔离强度 | 成本/弹性 | 适用 |
|------|---------|----------|------|
| 共享表 + `tenant_id` 列 | 逻辑隔离 | 最低，扩租户零成本 | 绝大多数 SaaS 的起点与主流 |
| 每租户一个 schema | 中 | 迁移要乘以租户数 | 中大客户、合规要求 |
| 每租户一个库 | 物理隔离 | 最高 | 金融/医疗等强合规、超大客户 |

教程实现第一种——它是行业默认起点（Slack、Notion 早期都是），也最考验代码纪律：**隔离不是数据库给你的，是每一行查询自己挣来的**。

## 17.2 隔离红线：tenant_id 只能来自 token

`todo_api_saas` 的两条铁律，源码里用「安全红线」注释标出：

1. **所有业务查询一律 `WHERE tenant_id = $n`**。services 层每个函数第一个业务参数就是 `tenant_id`，漏写一处 = 数据串租户 = SaaS 死刑级事故。
2. **`tenant_id` 只从 JWT claims 里取，绝不从请求体、路径、查询参数取**。用户可控的输入永远不能决定「你是谁的数据」。

配套的细节：租户 A 访问租户 B 的 todo，返回 **404 而不是 403**——403 等于告诉攻击者「这个 id 存在，只是不属于你」，404 连资源存在性都不泄露。

> 进阶方向：Postgres 还支持 RLS（行级安全策略），把 `tenant_id` 过滤下沉到数据库强制执行，代码漏写也兜得住，代价是会话管理更复杂。关键词：`CREATE POLICY`。

## 17.3 登录链路：argon2 + JWT

```text
POST /auth/login {email, password}
  → 查用户 → argon2 验证密码哈希（数据库里绝不存明文）
  → 签 JWT：Claims { sub: user_id, tenant_id, role, exp }
  → 返回 {token}，客户端此后每个请求带 Authorization: Bearer <token>
```

几个「为什么」：

- **argon2**：密码哈希的现行推荐（抗 GPU 暴破），`password_hash` 字段存的是带盐哈希，泄库也无法反推密码
- **JWT 是无状态凭证**：服务端不存 session，token 自带身份和租户——天然适配多实例集群（第 15 章的无状态原则）；代价是**签发后无法立刻撤销**，只能等 exp 过期，所以 exp 要短（示例 1 小时），真撤销需求上黑名单或改用有状态 session
- **HS256 密钥来自环境变量**：`JWT_SECRET` 默认值仅供本地，生产必须走密钥管理（第 16.3 节）

验证侧是 axum 的惯用形态——一个实现 `FromRequestParts` 的提取器 `AuthUser`：handler 参数里写上它，路由就自动受保护、拿到的就是已验证的 claims：

```rust
async fn list_todos(user: AuthUser, State(state): State<AppState>) -> ... {
    // user.tenant_id 已经过签名验证，直接用于查询过滤
}
```

## 17.4 RBAC：角色的最小可用形态

示例只分两个角色：`admin` 与 `member`，规则一条：**删除 todo 仅限 admin**（member 得 403）。麻雀虽小，把 RBAC 的核心取舍讲清了：

- **角色放进 JWT claims**：省掉每个请求查库，代价是改角色后旧 token 里还是旧角色（等 exp 过期才生效）——和上面的撤销问题同源
- 真实系统的演进路径：角色 → 细粒度权限点（permission）→ 资源级授权（如「只能删自己创建的」）。关键词：Casbin、ReBAC

试玩（种子账号密码都是 `password123`）：

```bash
cd examples-middleware
docker compose up -d postgres redis
cargo run -p todo_api_saas    # 127.0.0.1:3003

# 登录拿 token
curl -s -X POST 127.0.0.1:3003/auth/login \
  -H 'content-type: application/json' \
  -d '{"email":"admin@acme.test","password":"password123"}'

# 带 token 访问（把 <TOKEN> 换成上一步返回值）
curl -s 127.0.0.1:3003/todos -H 'Authorization: Bearer <TOKEN>'
```

## 本章小结

- 多租户三方案：共享表 + tenant_id 是主流起点，schema/库级隔离按合规需求升级
- 隔离两条铁律：查询必带 tenant_id 过滤；tenant_id 只信 token、不信任何用户输入
- 跨租户访问返回 404，不泄露资源存在性
- 登录链路：argon2 哈希验证 → JWT 签发 → 提取器验证，无状态适配集群
- JWT 的代价是撤销延迟：exp 要短，角色变更同理
- RBAC 从「角色进 claims + 敏感操作拦截」起步，向权限点/资源级授权演进

**自测清单**

- [ ] 我能说出三种多租户隔离方案各自的适用场景
- [ ] 我能解释为什么 tenant_id 绝不能来自请求体或路径
- [ ] 我能说出跨租户 404 与 403 的信息泄露差异
- [ ] 我能画出从 login 到受保护接口的完整凭证流转
- [ ] 我能解释 JWT 无状态的好处和撤销难题
- [ ] 我能说出角色放 claims 与放数据库各自的代价

## 练习

1. 给 `todo_api_saas` 加「只能删自己创建的 todo」：todos 表加 `created_by`，DELETE 时 admin 可删任意、member 仅可删自己的——体会从角色到资源级授权的跨越。
2. 实现 token 刷新：`POST /auth/refresh` 用还没过期的旧 token 换新 token，思考它和「短 exp」怎么配合。
3. 读 Postgres RLS 文档，给 todos 表写一条 `CREATE POLICY`，验证代码里去掉 WHERE 过滤后隔离依然成立。

**动手验证**：

```bash
cd examples-middleware
docker compose up -d postgres redis
cargo test -p todo_api_saas -- --ignored   # 含跨租户隔离/RBAC 的完整断言
```

---

导航：[上一章](16-deployment.md) | [返回目录](../README.md) | [下一章](18-quota-billing-api.md)
