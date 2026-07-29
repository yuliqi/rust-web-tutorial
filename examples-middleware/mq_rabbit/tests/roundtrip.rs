//! 集成测试：真连 RabbitMQ，发 N 条收 N 条，验证整条链路（连接 → 声明 → 发布 → 消费 → ack）。
//! 依赖外部服务，所以标记 #[ignore]，平时 `cargo test` 会跳过；想跑请先启动容器：
//!   cd examples-middleware && docker compose up -d rabbitmq
//!   cargo test -p mq_rabbit -- --ignored

use anyhow::Result;
use lapin::options::QueueDeleteOptions;
use mq_rabbit::{DEFAULT_AMQP_URL, EmailJob, connect, consume_n, declare_queue, publish};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 每次运行生成一个独一无二的队列名（时间戳纳秒 + 进程 id）。
/// 为什么不用固定名字？上次测试残留的消息会混进这次的断言里，
/// 测试之间互相污染是集成测试最常见的「偶发失败」来源——用随机资源名隔离。
fn unique_queue_name() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时间早于 1970？")
        .as_nanos();
    format!("test.roundtrip.{}.{nanos}", std::process::id())
}

#[tokio::test]
#[ignore = "需要先 docker compose up -d rabbitmq"]
async fn publish_n_then_consume_n() -> Result<()> {
    let url = std::env::var("AMQP_URL").unwrap_or_else(|_| DEFAULT_AMQP_URL.to_string());
    let (_conn, channel) = connect(&url).await?;

    let queue = unique_queue_name();
    declare_queue(&channel, &queue).await?;

    let sent: Vec<EmailJob> = (1..=3)
        .map(|i| EmailJob {
            to: format!("user{i}@example.com"),
            subject: format!("集成测试第 {i} 封"),
        })
        .collect();
    for job in &sent {
        publish(&channel, &queue, job).await?;
    }

    // 带超时消费：万一 broker 出问题，测试会明确失败而不是永远挂住。
    let got = consume_n(&channel, &queue, sent.len(), Duration::from_secs(10)).await?;

    // 单队列单消费者场景下 RabbitMQ 保证先进先出，所以可以按顺序整体断言。
    assert_eq!(got, sent);

    // 清理临时队列，别在 broker 里留垃圾（管理界面看着也清爽）。
    channel
        .queue_delete(&queue, QueueDeleteOptions::default())
        .await?;
    Ok(())
}
