//! 消息队列入门：用 lapin 驱动 RabbitMQ，跑通「生产者 → 队列 → 消费者」最小闭环。
//!
//! 为什么要消息队列？三个经典理由：
//! - 削峰：请求洪峰先落进队列排队，消费者按自己的节奏处理，下游不被冲垮；
//! - 解耦：生产者只管往队列扔消息，不需要知道谁消费、怎么消费，双方可以独立部署升级；
//! - 异步化：耗时操作（发邮件、生成报表）挪到后台，接口立刻返回，用户不用干等。
//!
//! 它和第 10 章的 tokio::mpsc 有什么区别？mpsc 是**进程内**的内存通道，进程一退消息就没了；
//! RabbitMQ 是独立运行的 broker，**跨进程/跨服务**传递消息，且可以持久化到磁盘——
//! 生产者和消费者可以是两台机器上用不同语言写的程序。
//!
//! 本文件职责：封装连接、声明队列、发布、消费四个动作，供 main.rs 与集成测试复用。

use anyhow::{Context, Result, bail};
use futures_lite::StreamExt;
use lapin::{
    BasicProperties, Channel, Connection, ConnectionProperties,
    options::{BasicAckOptions, BasicConsumeOptions, BasicPublishOptions, QueueDeclareOptions},
    types::FieldTable,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// 本地 docker compose 起的 RabbitMQ 默认地址。
/// 结尾的 `%2f` 是 `/` 的 URL 编码——RabbitMQ 的默认 vhost 名字就叫 "/"，
/// 但 URL 路径里不能直接再写一个裸 `/`，所以要转义。初学者常在这里连不上。
pub const DEFAULT_AMQP_URL: &str = "amqp://guest:guest@localhost:5672/%2f";

/// 一条「发邮件」任务。
///
/// 为什么定义显式的消息结构体，而不是直接发裸字符串？
/// 消息就是生产者与消费者之间的 **API 契约**：双方可能是不同团队、不同语言写的程序，
/// 中间隔着一个 broker，编译器帮不上忙。有了明确的结构 + JSON 序列化，
/// 字段增减、类型变化都有迹可循；裸字符串则要靠口头约定，改一个格式全线崩。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailJob {
    pub to: String,
    pub subject: String,
}

/// 连接 RabbitMQ，返回 (Connection, Channel)。
///
/// 两层概念：Connection 是一条真实的 TCP 连接（建立成本高，全程序共享一条即可）；
/// Channel 是复用在这条连接上的轻量「会话」，几乎所有 AMQP 操作都通过 Channel 发起。
/// 注意要把 Connection 也一并返回并持有——它一旦被 drop，TCP 断开，Channel 随之失效。
pub async fn connect(url: &str) -> Result<(Connection, Channel)> {
    let conn = Connection::connect(url, ConnectionProperties::default())
        .await
        .with_context(|| format!("连接 RabbitMQ 失败: {url}"))?;
    let channel = conn.create_channel().await.context("创建 channel 失败")?;
    tracing::info!(url, "已连接 RabbitMQ");
    Ok((conn, channel))
}

/// 把人类可读的名字规范成队列名：小写，字母/数字/`-`/`.`/`_` 之外的字符替换为 `_`。
///
/// 为什么值得写个函数？队列名会出现在管理界面、监控和运维脚本里，
/// 如果 "Email Jobs" 和 "email_jobs" 混用，broker 会老老实实建出两个队列，
/// 消息各进各的，排查起来非常痛苦。统一规范从源头堵住这种事故。
/// （另注：以 `amq.` 开头的名字是 RabbitMQ 保留前缀，自己起名要避开。）
pub fn normalize_queue_name(raw: &str) -> String {
    raw.trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_') {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// 声明一个 durable（持久）队列。声明是幂等的：队列已存在且参数一致时相当于 no-op。
///
/// 易混淆点：durable 和消息持久化是**两回事**——
/// - `durable: true` 只保证「队列本身」在 broker 重启后还在；
/// - 队列里的「消息」要不要落盘，由每条消息的 delivery_mode 决定（见 [`publish`]）。
///
/// 只开其一都不保险：durable 队列 + 非持久消息，重启后队列还在但消息没了；
/// 非 durable 队列 + 持久消息，重启后队列连同消息一起消失。两者要一起用。
pub async fn declare_queue(channel: &Channel, name: &str) -> Result<()> {
    channel
        .queue_declare(
            name,
            QueueDeclareOptions {
                durable: true,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await
        .with_context(|| format!("声明队列失败: {name}"))?;
    Ok(())
}

/// 把一条任务以 JSON 发布到指定队列，并标记为持久化消息。
pub async fn publish(channel: &Channel, queue: &str, job: &EmailJob) -> Result<()> {
    let payload = serde_json::to_vec(job).context("序列化 EmailJob 失败")?;
    channel
        .basic_publish(
            // 交换机（exchange）留空 = 使用「默认交换机」：它把消息直接路由到
            // 与 routing key 同名的队列。这是最简单的点对点模式，
            // 发布/订阅、广播等玩法要换别的交换机类型，教程正文再展开。
            "",
            queue,
            BasicPublishOptions::default(),
            &payload,
            // delivery_mode = 2 表示「持久化消息」：broker 会把它写到磁盘。
            // 配合 durable 队列（见 declare_queue），broker 重启才不丢消息。
            BasicProperties::default().with_delivery_mode(2),
        )
        .await
        .with_context(|| format!("发布消息到 {queue} 失败"))?
        // basic_publish 返回的是一个「确认」future，再 await 一次等 broker 回执。
        .await
        .context("等待发布确认失败")?;
    tracing::debug!(queue, to = %job.to, "已发布消息");
    Ok(())
}

/// 从队列消费 n 条消息后返回；超过 timeout 还没凑齐就报错。
///
/// 「消费 n 条就退出」是教学场景的特殊设计——demo 和测试总得能结束。
/// 真实服务里的消费者是一个**常驻循环**：`while let Some(delivery) = consumer.next().await`
/// 一直跑到进程收到退出信号为止。
pub async fn consume_n(
    channel: &Channel,
    queue: &str,
    n: usize,
    timeout: Duration,
) -> Result<Vec<EmailJob>> {
    let mut consumer = channel
        .basic_consume(
            queue,
            // consumer tag 留空，让 broker 生成唯一标识，避免多个消费者撞名。
            "",
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await
        .with_context(|| format!("订阅队列失败: {queue}"))?;

    let mut jobs = Vec::with_capacity(n);
    let recv_loop = async {
        while jobs.len() < n {
            let Some(delivery) = consumer.next().await else {
                bail!("consumer 流意外结束（channel 可能已关闭）");
            };
            let delivery = delivery.context("接收消息失败")?;
            let job: EmailJob =
                serde_json::from_slice(&delivery.data).context("反序列化 EmailJob 失败")?;

            // 手动 ack：告诉 broker「这条我处理完了，可以删了」。
            // 这就是 at-least-once（至少一次）语义的来源——如果消费者处理到一半
            // 崩溃、没来得及 ack，broker 会把这条消息**重新投递**给别的消费者。
            // 推论：同一条消息可能被处理两次，所以消费逻辑必须**幂等**
            // （例如按业务 ID 去重，重复的发邮件请求直接跳过）。
            delivery
                .ack(BasicAckOptions::default())
                .await
                .context("ack 失败")?;
            jobs.push(job);
        }
        Ok(())
    };

    // 教学/测试场景必须防挂死：队列里消息不够 n 条时，不带超时会永远等下去。
    match tokio::time::timeout(timeout, recv_loop).await {
        Ok(result) => {
            result?;
            Ok(jobs)
        }
        Err(_) => bail!(
            "消费超时：期望 {n} 条，{timeout:?} 内只收到 {} 条",
            jobs.len()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 消息是跨服务契约，序列化格式就是契约的载体——round-trip 测试保证
    /// 「发出去的」和「收回来的」是同一个东西，不依赖任何外部服务。
    #[test]
    fn email_job_serde_roundtrip() {
        let job = EmailJob {
            to: "user@example.com".to_string(),
            subject: "欢迎注册".to_string(),
        };
        let json = serde_json::to_string(&job).unwrap();
        // 顺带固定字段名：改了字段名会破坏和旧消费者的兼容，让测试先叫起来。
        assert!(json.contains(r#""to":"user@example.com""#));
        let back: EmailJob = serde_json::from_str(&json).unwrap();
        assert_eq!(back, job);
    }

    #[test]
    fn normalize_queue_name_rules() {
        assert_eq!(normalize_queue_name("Email Jobs"), "email_jobs");
        assert_eq!(normalize_queue_name("  order.Created!  "), "order.created_");
        assert_eq!(normalize_queue_name("already_ok-1"), "already_ok-1");
    }
}
