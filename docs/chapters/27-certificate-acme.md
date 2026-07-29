# 第 27 章：证书管理 — ACME 自动签发与续期

> 示例工程：`examples-middleware/cert_demo`（离线核心可跑可测;真实 ACME 签发需公网域名）

## 学习目标

1. 理解 HTTPS 证书的生命周期,以及为什么要自动化
2. 掌握证书的生成/解析/续期判定(离线可做的部分)
3. 理解 ACME 协议:账户、下单、域名验证挑战、下载证书
4. 会用「到期前自动续期」把证书管理接进定时任务

## 27.1 为什么要自动化证书

面板/SaaS 要给用户站点配 HTTPS。手动买证书、传证书、记到期日、手动续——在几十上百个域名时就是灾难。现代方案是 **ACME 协议**(Let's Encrypt 免费签发):程序自动申请、自动验证域名所有权、到期前自动续期,全程无人值守。

证书生命周期:

```text
生成密钥 + CSR → 向 CA 申请 → 证明你控制这个域名（挑战）→ CA 签发 → 部署 → 到期前续期 → 循环
```

## 27.2 离线能做的:生成、解析、续期判定

`cert_demo` 把不需要联网的部分做成可运行可测:

- `generate_self_signed(domains)`:rcgen 生成自签证书(本地开发/内网用;公网面向用户**必须**用受信 CA)。注意 rcgen 默认有效期长到不真实,示例手动设成 90 天贴合 Let's Encrypt
- `generate_keypair_and_csr(domains)`:生成私钥 + CSR(证书签名请求)。ACME 下单要提交 CSR
- `parse_not_after(cert_pem)`:用 x509-parser 读证书到期时间
- `needs_renewal(not_after, now, threshold_days)`:`now >= not_after - threshold天`。**Let's Encrypt 证书 90 天有效,惯例提前 30 天续**——留足失败重试的窗口,别等最后一天

这些是纯函数,能穷尽单测(还早/临界/已过期各种边界)。

## 27.3 域名验证:HTTP-01 挑战

CA 凭什么给你签 `example.com` 的证书?你得**证明你控制这个域名**。最常用的 HTTP-01 挑战:

```text
CA:  访问 http://example.com/.well-known/acme-challenge/{token}
你:  在这个路径返回 {token}.{key_authorization}
CA:  拿到正确响应 → 确认你控制该域名 → 签发
```

`cert_demo` 实现了内存 `ChallengeStore`(token → key_authorization)+ axum 路由 `GET /.well-known/acme-challenge/{token}`,这部分离线可测。另外两种挑战一句话:**DNS-01**(在 DNS 加 TXT 记录,是唯一能签**通配符**证书的方式)、**TLS-ALPN-01**(走 443,适合不方便开 80 的场景)。

## 27.4 ACME 下单流程

`cert_demo` 用 `instant-acme` 写了完整的下单流程(代码真实能编译,真跑需要公网域名 + 80 端口 + Let's Encrypt staging):

```text
1. 创建 ACME 账户
2. 下单 new_order（提交要签的域名）
3. 遍历 authorizations，对每个域名取 HTTP-01 挑战：
     把 key_authorization 写进 ChallengeStore → set_ready()
4. poll_ready 等 CA 来验证
5. 生成私钥 + CSR(DER)，finalize 提交
6. poll_certificate 下载证书链
   → 私钥全程不出本机
```

> **一个工程取舍**:`instant-acme` 默认用 `aws-lc-rs` 后端,需要 C/cmake 工具链。示例改用 `ring` 后端(和 rcgen 一致),避免本地构建依赖——这类"选后端避开系统依赖"的决定在 Rust 加密库里很常见。

## 27.5 自动续期:接进定时任务

把证书管理跑起来的最后一环,是**无人值守续期**——正是第 21 章定时任务的用武之地:

```text
每天定时任务触发一次：
  对每个域名 → parse_not_after → needs_renewal？
    是 → 走 ACME 下单续签 → 成功后热重载证书（不重启服务）
```

告警也能串上:续期失败(域名解析变了、CA 限流)要通过第 14 章消息队列发通知,别等证书过期站点挂了才发现。

```bash
cd examples-middleware
cargo run -p cert_demo    # 离线：生成自签证书 → 解析到期 → 演示"还早"和"该续"两种判定
```

## 本章小结

- ACME(Let's Encrypt)让证书申请/验证/续期全自动,是现代 HTTPS 的标准做法
- 离线可做:生成密钥/CSR、解析到期、续期判定(90 天有效、提前 30 天续)
- HTTP-01 挑战:在 `/.well-known/acme-challenge/{token}` 返回指定值证明域名控制权
- DNS-01 能签通配符;TLS-ALPN-01 走 443
- ACME 下单:建账户→下单→完成挑战→提交 CSR→下载证书,私钥不出本机
- 自动续期 = 定时任务每天检查 needs_renewal,该续就续,失败要告警

**自测清单**

- [ ] 我能画出证书从申请到续期的完整生命周期
- [ ] 我能解释为什么证书管理必须自动化
- [ ] 我能说清 HTTP-01 挑战怎么证明域名控制权
- [ ] 我能说出三种挑战方式各自的适用场景(尤其通配符用哪个)
- [ ] 我能描述 ACME 下单的主要步骤
- [ ] 我能把自动续期接进定时任务并说清失败要告警

## 练习

1. 用 Pebble(Let's Encrypt 的测试 ACME 服务器,有 Docker 镜像)在本地真跑一次 `order_certificate`,不需要公网域名。
2. 把续期做成第 21 章的定时任务:每天检查所有域名,`needs_renewal` 为真的自动续,续完热重载。
3. 实现证书热重载:不重启服务的前提下换用新证书(提示:`rustls` 的 `ServerConfig` 用 `ArcSwap` 或每次握手取最新)。
4. 加通配符证书支持:改用 DNS-01 挑战(需要你的 DNS 服务商 API),思考它和 HTTP-01 在自动化上的差异。

**动手验证**：

```bash
cd examples-middleware
cargo test -p cert_demo    # 证书生成/解析/续期判定/HTTP-01 路由的断言（ACME live 测试默认跳过）
```

---

导航：[上一章](26-web-terminal-bastion.md) | [返回目录](../README.md) | [下一章](28-realtime-monitoring.md)
