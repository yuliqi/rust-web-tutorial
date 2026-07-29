//! 服务注册与发现（etcd 版）。
//!
//! 先把模型讲透——服务发现的本质就是一张「带 TTL 的注册表」：
//!
//! - **注册 = 写 key**：实例启动后把自己的地址写进 etcd，
//!   key 形如 `/services/{service}/{addr}`，value 是地址本身。
//! - **心跳 = 续租**：写 key 时绑定一个租约（lease，带 TTL），
//!   实例活着就定期续租（keepalive），key 就一直存在。
//! - **实例挂了 = 租约过期，key 自动消失**：进程崩溃、断网、机器宕机——
//!   无论哪种死法，续租都会停止，TTL 一到 etcd 自动删 key。
//!   注册表不需要任何人「负责清理」，这是整个设计最精妙的地方。
//!
//! 消费方两种玩法：`discover` 一次性按前缀读全量（轮询），
//! `watch_service` 订阅前缀变化（推送），差别见各自的注释。

use anyhow::{Context, Result};
use etcd_client::{Client, EventType, GetOptions, PutOptions, WatchOptions, WatchRequestSender};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// docker compose 起的本地 etcd（免鉴权，仅供学习）。
pub const DEFAULT_ETCD_ENDPOINT: &str = "localhost:2379";

/// 单个实例的注册 key：`/services/{service}/{addr}`。
/// 把 addr 编进 key（而不是只放在 value 里）有两个原因：
/// 1. 同一服务的多个实例互不覆盖——key 不同，天然共存；
/// 2. key 被删除（实例下线）时事件里只有 key 没有 value，
///    从 key 就能还原出是哪个地址下线了（见 watch_service）。
///
/// key 规范单独成纯函数：注册方和发现方必须用同一套规则拼 key，
/// 集中在一处才不会写着写着两边对不上，也方便离线单测。
pub fn service_key(service: &str, addr: &str) -> String {
    format!("/services/{service}/{addr}")
}

/// 某个服务全部实例的公共前缀：`/services/{service}/`。
/// 末尾的 `/` 不能少：没有它，前缀 `/services/hello` 会把
/// `/services/hello-admin/...` 也扫进来。
pub fn service_prefix(service: &str) -> String {
    format!("/services/{service}/")
}

/// 一次注册的句柄。拿着它，注册才持续有效：
/// - 显式调用 [`Registration::deregister`]：撤销租约，key **立刻**消失（优雅下线）；
/// - 直接 drop：只是停掉心跳任务，key 会在 TTL 到期后自动消失
///   （效果等同于实例崩溃——这正是租约模型兜底的场景，只是慢一个 TTL）。
pub struct Registration {
    client: Client,
    key: String,
    lease_id: i64,
    stop_tx: Option<oneshot::Sender<()>>,
    keepalive: Option<JoinHandle<()>>,
}

impl Registration {
    /// 本次注册写入的完整 key（调试/测试用）。
    pub fn key(&self) -> &str {
        &self.key
    }

    /// 优雅下线：先停心跳，再撤销租约。
    /// 撤销租约会让 etcd 立刻删掉绑定的 key——比等 TTL 过期快得多，
    /// 所以正常关机时应该走这条路，把「下线」第一时间告诉所有观察者。
    pub async fn deregister(mut self) -> Result<()> {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(()); // 心跳任务可能已自行退出，发送失败无所谓
        }
        if let Some(task) = self.keepalive.take() {
            let _ = task.await;
        }
        self.client
            .lease_revoke(self.lease_id)
            .await
            .context("撤销租约失败")?;
        Ok(())
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        // Drop 里没法 await（无法调用异步的 lease_revoke），
        // 只能通知心跳任务停下；key 交给 TTL 过期机制自动清理。
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
    }
}

/// 注册一个服务实例：写 key + 绑租约 + 后台心跳续租。
///
/// `ttl_secs` 是「实例挂了之后，多久从注册表里消失」的上限：
/// 调小故障感知快，但心跳更频繁、对时钟抖动更敏感；生产常见 5~30 秒。
/// 心跳间隔取 TTL 的 1/3——续租必须明显快于过期，留出网络抖动的余量。
pub async fn register(
    client: &Client,
    service: &str,
    addr: &str,
    ttl_secs: i64,
) -> Result<Registration> {
    let mut client = client.clone(); // etcd Client 是廉价克隆的句柄，内部共享连接

    // 第一步：申请一个 TTL 秒后过期的租约。此刻起倒计时已经开始。
    let lease = client
        .lease_grant(ttl_secs, None)
        .await
        .context("申请租约失败")?;
    let lease_id = lease.id();

    // 第二步：写 key 并绑定租约。「绑定」的含义：租约过期或被撤销时，
    // etcd 自动删除这个 key——注册信息的生死从此和租约绑在一起。
    let key = service_key(service, addr);
    client
        .put(
            key.clone(),
            addr,
            Some(PutOptions::new().with_lease(lease_id)),
        )
        .await
        .with_context(|| format!("注册 {key} 失败"))?;

    // 第三步：后台心跳。keeper 负责发续租请求，stream 收 etcd 的确认。
    let (mut keeper, mut stream) = client
        .lease_keep_alive(lease_id)
        .await
        .context("建立续租通道失败")?;

    let (stop_tx, mut stop_rx) = oneshot::channel::<()>();
    let heartbeat = Duration::from_secs(u64::try_from(ttl_secs / 3).unwrap_or(1).max(1));
    let hb_key = key.clone();
    let keepalive = tokio::spawn(async move {
        let mut tick = tokio::time::interval(heartbeat);
        loop {
            tokio::select! {
                // 句柄那头喊停（deregister 或 drop），心跳到此为止
                _ = &mut stop_rx => break,
                _ = tick.tick() => {
                    // 发一次续租；失败通常意味着和 etcd 断连。
                    // 教学示例选择直接退出（key 会随 TTL 消失，符合「我可能真的不健康」的语义）；
                    // 生产客户端会在这里做重连重试。
                    if keeper.keep_alive().await.is_err() {
                        tracing::warn!(key = %hb_key, "续租请求失败，停止心跳");
                        break;
                    }
                    match stream.message().await {
                        Ok(Some(_)) => tracing::debug!(key = %hb_key, "续租成功"),
                        _ => {
                            tracing::warn!(key = %hb_key, "续租确认流中断，停止心跳");
                            break;
                        }
                    }
                }
            }
        }
    });

    Ok(Registration {
        client,
        key,
        lease_id,
        stop_tx: Some(stop_tx),
        keepalive: Some(keepalive),
    })
}

/// 服务发现：按前缀读出某个服务当前活着的全部实例地址。
///
/// 这是「轮询」式用法：每次调用都是一张此刻的快照。简单直接，
/// 但拿到的名单会随时间失效——想及时感知变化要么定期重查（有延迟、
/// 有无效请求开销），要么改用 [`watch_service`] 订阅推送。
pub async fn discover(client: &Client, service: &str) -> Result<Vec<String>> {
    let mut client = client.clone();
    let resp = client
        .get(
            service_prefix(service),
            Some(GetOptions::new().with_prefix()), // 前缀查询：一次拿回整个目录
        )
        .await
        .with_context(|| format!("查询服务 {service} 失败"))?;

    let mut addrs: Vec<String> = resp
        .kvs()
        .iter()
        .filter_map(|kv| kv.value_str().ok().map(str::to_string))
        .collect();
    addrs.sort(); // 排序只为输出稳定，方便肉眼比对和测试断言
    Ok(addrs)
}

/// 实例上下线事件：Up = 新实例注册（key 被 PUT），Down = 实例消失（key 被 DELETE，
/// 无论是优雅下线主动撤销，还是崩溃后租约过期，观察者看到的都是同一种事件）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceEvent {
    Up { addr: String },
    Down { addr: String },
}

/// 订阅某个服务的实例变化，每个事件回调一次 `on_event`。
///
/// 轮询 vs watch 的差别：轮询的感知延迟 = 轮询间隔，且绝大多数请求
/// 查回来的是「没变化」，纯属浪费；watch 由 etcd 在变化发生时**推送**，
/// 延迟是毫秒级的，且没变化时一个字节都不用传。代价是要维护一条
/// 长连接——这条连接断了得重建（本示例从简，未做重连）。
///
/// 返回的 `WatchRequestSender` 是订阅的生命线：drop 它订阅就取消，调用方要拿住。
pub async fn watch_service(
    client: &Client,
    service: &str,
    mut on_event: impl FnMut(ServiceEvent) + Send + 'static,
) -> Result<(WatchRequestSender, JoinHandle<()>)> {
    let mut client = client.clone();
    let prefix = service_prefix(service);
    let stream = client
        .watch(prefix.clone(), Some(WatchOptions::new().with_prefix()))
        .await
        .with_context(|| format!("watch 服务 {service} 失败"))?;
    // 拆成两半：sender 留给调用方当「订阅句柄」，response 流交给后台任务消费
    let (sender, mut responses) = stream.split();

    let task = tokio::spawn(async move {
        // 每个 message 是一批事件（etcd 会把同一时刻的变更打包推送）
        while let Ok(Some(resp)) = responses.message().await {
            for event in resp.events() {
                let Some(kv) = event.kv() else { continue };
                let key = kv.key_str().unwrap_or_default();
                // DELETE 事件里 value 已经没了，只能从 key 反推地址——
                // 这就是 service_key 把 addr 编进 key 的第二个理由。
                let addr = key.strip_prefix(prefix.as_str()).unwrap_or(key).to_string();
                match event.event_type() {
                    EventType::Put => on_event(ServiceEvent::Up { addr }),
                    EventType::Delete => on_event(ServiceEvent::Down { addr }),
                }
            }
        }
    });

    Ok((sender, task))
}

/// 客户端负载均衡的最小形态：拿着实例名单，轮流选下一个（round-robin）。
///
/// 原子计数器自增后对实例数取模，多线程并发调用也能大致均匀地摊开请求。
/// 别小看这几行——gRPC 客户端的 round_robin 策略、服务网格 sidecar 的
/// 转发逻辑，做的都是同一件事的工程化版本：名单来自服务发现，
/// 再加上健康检查剔除坏实例、加权、会话保持等策略。
/// 名单为空返回 None：没有可用实例是调用方必须处理的正常情况。
pub fn pick_round_robin<'a>(instances: &'a [String], counter: &AtomicUsize) -> Option<&'a str> {
    if instances.is_empty() {
        return None;
    }
    // Relaxed 就够了：计数器只求「大家拿到的序号不同」，不同步任何其他数据
    let n = counter.fetch_add(1, Ordering::Relaxed);
    Some(instances[n % instances.len()].as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_layout() {
        assert_eq!(
            service_key("hello", "127.0.0.1:8080"),
            "/services/hello/127.0.0.1:8080"
        );
        assert_eq!(service_prefix("hello"), "/services/hello/");
        // 发现方靠前缀匹配找到注册方写的 key，两者必须咬合
        assert!(service_key("hello", "127.0.0.1:8080").starts_with(&service_prefix("hello")));
    }

    #[test]
    fn prefix_does_not_leak_into_similar_names() {
        // 末尾的 `/` 保证 hello 的前缀匹配不到 hello-admin 的实例
        assert!(!service_key("hello-admin", "1.2.3.4:80").starts_with(&service_prefix("hello")));
    }

    #[test]
    fn round_robin_rotates() {
        let instances = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let counter = AtomicUsize::new(0);
        let picks: Vec<&str> = (0..4)
            .map(|_| pick_round_robin(&instances, &counter).unwrap())
            .collect();
        assert_eq!(picks, ["a", "b", "c", "a"]); // 转满一圈后回到起点
    }

    #[test]
    fn round_robin_empty_is_none() {
        let counter = AtomicUsize::new(0);
        assert_eq!(pick_round_robin(&[], &counter), None);
    }
}
