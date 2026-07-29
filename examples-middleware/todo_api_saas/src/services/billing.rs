//! 计费业务：处理支付商 webhook 送来的订阅变更事件（验签已由 routes 层完成）。

use sqlx::PgPool;

use crate::error::{AppError, AppResult};
use crate::models::BillingEvent;

/// 应用一条计费事件。支付商回调的通用模式是三步：
/// **验签**（routes/webhooks.rs）→ **幂等处理**（本函数：UPDATE 天然幂等，
/// 同一事件重放两次结果一致；真实系统还会按 event_id 去重）→ **快速 200**
/// （支付商都有重试机制，超时/非 2xx 会反复重发；耗时的后续动作应扔进队列，
/// 先把 200 还回去）。
pub async fn handle_event(pool: &PgPool, event: BillingEvent) -> AppResult<()> {
    // 不认识的事件类型：记日志、返回 Ok（对外 200）。这是 Stripe 官方建议——
    // 支付商随时会新增事件类型，报错只会让它对着你无限重试。
    if event.event != "subscription.updated" {
        tracing::info!(event = %event.event, "ignoring unhandled billing event");
        return Ok(());
    }

    // plan 是外部输入，进库前收紧到已知枚举值——数据库里的 plan 列被
    // services/todos.rs 的配额逻辑消费，脏值会让配额判断悄悄失效。
    if event.plan != "free" && event.plan != "pro" {
        return Err(AppError::bad_request(format!(
            "unknown plan: {}",
            event.plan
        )));
    }

    let result = sqlx::query("UPDATE saas.tenants SET plan = $1 WHERE slug = $2")
        .bind(&event.plan)
        .bind(&event.tenant_slug)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found(format!(
            "tenant {} not found",
            event.tenant_slug
        )));
    }

    // 套餐即刻生效：配额检查每次现查 tenants.plan（见 services/todos.rs），
    // 这条 UPDATE 一提交，free 的闸门立即抬起，无需等任何缓存/token 过期。
    tracing::info!(tenant = %event.tenant_slug, plan = %event.plan, "tenant plan updated");
    Ok(())
}
