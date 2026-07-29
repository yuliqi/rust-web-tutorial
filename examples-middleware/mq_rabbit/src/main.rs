//! 可运行的最小演示：发 3 条 EmailJob 进队列 → 消费 3 条并打印 → 退出。
//! 「为什么要消息队列」以及各函数的教学注释见 lib.rs 文件头。
//!
//! 运行前先启动 RabbitMQ：
//!   cd examples-middleware && docker compose up -d rabbitmq
//! 然后 `cargo run -p mq_rabbit`。
//! 管理界面 http://localhost:15672（guest/guest）可以实时看到队列和消息数。

use anyhow::Result;
use mq_rabbit::{DEFAULT_AMQP_URL, EmailJob, connect, consume_n, declare_queue, publish};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    // 环境变量可覆盖，方便连非本机的 broker；默认连 docker compose 起的本地实例。
    let url = std::env::var("AMQP_URL").unwrap_or_else(|_| DEFAULT_AMQP_URL.to_string());

    // 连不上时最常见的原因是容器没起，把解决办法直接写进错误信息里。
    let (_conn, channel) = connect(&url).await.map_err(|e| {
        e.context(
            "连不上 RabbitMQ？请先执行 `docker compose up -d rabbitmq`（在 examples-middleware 目录下），\
             启动后可打开管理界面 http://localhost:15672（guest/guest）确认状态",
        )
    })?;

    let queue = "email_jobs";
    declare_queue(&channel, queue).await?;

    // 生产者视角：把「要做的事」描述成消息扔进队列，立刻返回，不等邮件真的发出去。
    for i in 1..=3 {
        let job = EmailJob {
            to: format!("user{i}@example.com"),
            subject: format!("欢迎注册（第 {i} 封）"),
        };
        publish(&channel, queue, &job).await?;
        tracing::info!(to = %job.to, "已发布");
    }

    // 消费者视角：从队列取出消息逐条处理。真实项目里生产者和消费者
    // 通常是两个独立进程；这里为了演示闭环，放在同一个程序里一前一后执行。
    let jobs = consume_n(&channel, queue, 3, Duration::from_secs(5)).await?;
    for job in &jobs {
        tracing::info!(to = %job.to, subject = %job.subject, "假装发送邮件");
    }

    tracing::info!("演示结束：发布 3 条，消费 {} 条", jobs.len());
    Ok(())
}
