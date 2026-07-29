//! 同步引擎（本 crate 的心脏）：把凭证、provider、限流、锁、归一化入库串成一条流水线。
//!
//! ## 一次 `sync_tenant` 的数据流
//!
//! ```text
//! 抢分布式锁 lock:sync:{tenant}     ← 抢不到就整个跳过（集群去重，第 21 章）
//!   └─ load_credentials(解密该租户所有凭证)   ← WHERE tenant_id（多租户隔离，第 17 章）
//!        └─ 对每份凭证：
//!             check_rate_limit(provider)        ← 超配额就跳过这朵云（第 18 章）
//!               └─ provider_registry(name).list_assets(cred)   ← 多云抽象（第 6 章 trait）
//!                    └─ upsert_asset(ON CONFLICT DO UPDATE)     ← 归一化入库
//!   └─ 释放锁 → 返回 SyncReport（新增/更新/跳过/总数）
//! ```
//!
//! 三条红线在这条流水线上各司其职：
//! - **锁**保证「一次触发只有一个实例真正跑」；
//! - **限流**保证「不把客户的云账号 API 配额打爆」；
//! - **tenant_id** 保证「租户之间的凭证与资产彻底隔离」。

use anyhow::{Context, Result};
use redis::aio::ConnectionManager;
use sqlx::{PgPool, Row};
use std::time::Duration;

use crate::credentials::load_credentials;
use crate::crypto::Keyring;
use crate::lock::{new_token, sync_lock_key, try_lock, unlock};
use crate::provider::{CloudAsset, provider_registry};
use crate::ratelimit::check_rate_limit;

/// 同步锁 TTL。要略大于「一次租户同步预期最长耗时」——太短会导致同步没干完锁就过期、
/// 第二个实例趁虚而入；太长则持有者崩溃后锁迟迟不释放。教学 mock 很快，30 秒绰绰有余。
const SYNC_LOCK_TTL: Duration = Duration::from_secs(30);

/// 一次同步的结果报告。给演示打印、给 HTTP 响应、给测试断言三处共用。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct SyncReport {
    pub tenant_id: i64,
    /// 是否真正执行了同步（false = 没抢到锁，被别的实例抢先，整体跳过）。
    pub ran: bool,
    /// 处理了几份凭证。
    pub credentials: usize,
    /// 新插入的资产数。
    pub inserted: usize,
    /// 更新（已存在被 upsert 覆盖）的资产数。
    pub updated: usize,
    /// 因限流被跳过的 provider 次数。
    pub rate_limited: usize,
    /// 因 provider 未实现（registry 取不到）被跳过的凭证数。
    pub unsupported: usize,
}

impl SyncReport {
    /// 本次同步落库的资产总数（新增 + 更新）。
    pub fn total(&self) -> usize {
        self.inserted + self.updated
    }
}

/// 同步一个租户的全部云资产。集群安全、限流友好、多租户隔离，三合一。
///
/// 返回 [`SyncReport`]。注意：**没抢到锁不是错误**（正常的集群去重），返回 `ran=false`
/// 的报告即可，让调用方知道「这次由别的实例跑了」。
pub async fn sync_tenant(
    pool: &PgPool,
    cm: &mut ConnectionManager,
    keyring: &Keyring,
    tenant_id: i64,
) -> Result<SyncReport> {
    let mut report = SyncReport {
        tenant_id,
        ..Default::default()
    };

    // 1. 抢分布式锁：抢到的实例才同步，其余直接返回「没跑」。
    let lock_key = sync_lock_key(tenant_id);
    let token = new_token();
    if !try_lock(cm, &lock_key, &token, SYNC_LOCK_TTL).await? {
        tracing::info!(tenant_id, "同步锁被占，跳过（已有实例在同步该租户）");
        return Ok(report); // ran = false
    }
    report.ran = true;

    // 用一个内层闭包跑真正的同步，无论成功失败都能在末尾释放锁（避免任一 `?` 提前 return
    // 导致锁泄漏、要等 TTL 才释放）。Rust 没有 finally，这里用「跑完再解锁」的显式结构。
    let result = sync_locked(pool, cm, keyring, tenant_id, &mut report).await;

    // 2. 释放锁（验身解锁：只删自己那把）。解锁失败只记日志不改变同步结果。
    if let Err(e) = unlock(cm, &lock_key, &token).await {
        tracing::warn!(tenant_id, error = %e, "释放同步锁失败（将由 TTL 兜底过期）");
    }

    result.map(|()| report)
}

/// 持锁期间的实际同步逻辑，拆出来是为了让 [`sync_tenant`] 的「解锁」在任何路径上都执行到。
async fn sync_locked(
    pool: &PgPool,
    cm: &mut ConnectionManager,
    keyring: &Keyring,
    tenant_id: i64,
    report: &mut SyncReport,
) -> Result<()> {
    // 加载并解密该租户的凭证（load_credentials 内部带 WHERE tenant_id，隔离红线在那里）。
    let creds = load_credentials(pool, keyring, tenant_id).await?;
    report.credentials = creds.len();

    for cred in &creds {
        // provider 未实现（比如库里存了 gcp 但还没写 GcpProvider）：跳过，不报错。
        let Some(provider) = provider_registry(&cred.provider) else {
            tracing::warn!(tenant_id, provider = %cred.provider, "未实现的 provider，跳过");
            report.unsupported += 1;
            continue;
        };

        // 限流闸门：调 list_assets 前先问 Redis「这分钟还能不能再打这朵云」。
        let decision = check_rate_limit(cm, tenant_id, &cred.provider).await?;
        if !decision.allowed() {
            tracing::warn!(tenant_id, provider = %cred.provider, "触发限流，跳过本次拉取");
            report.rate_limited += 1;
            continue;
        }

        // 拉取 + 归一化（provider 返回的已是统一的 CloudAsset）。
        let assets = provider
            .list_assets(cred)
            .await
            .with_context(|| format!("list_assets 失败：tenant={tenant_id} provider={}", cred.provider))?;

        // 逐个 upsert 进统一资产表。
        for asset in &assets {
            let inserted = upsert_asset(pool, tenant_id, asset).await?;
            if inserted {
                report.inserted += 1;
            } else {
                report.updated += 1;
            }
        }
    }
    Ok(())
}

/// upsert 一个归一化资产，返回 true = 新插入、false = 更新了已有行。
///
/// `ON CONFLICT (tenant_id, provider, external_id) DO UPDATE`：同一份资产（同租户、同云、
/// 同云侧 id）第二次同步时不会重复插入，而是刷新它的 name/region/raw/synced_at——
/// 这就是「再 sync 一次总数不翻倍」的保证。
///
/// 怎么知道这行是插入还是更新：Postgres 的系统列 `xmax`，对**刚插入**的行为 0，
/// 对**被更新**的行非 0。`RETURNING (xmax = 0)` 就地告诉我们走了哪条路，无需再查一次。
async fn upsert_asset(pool: &PgPool, tenant_id: i64, asset: &CloudAsset) -> Result<bool> {
    let row = sqlx::query(
        r#"
        INSERT INTO cloudsync.assets
            (tenant_id, provider, external_id, asset_type, name, region, raw, synced_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, now())
        ON CONFLICT (tenant_id, provider, external_id) DO UPDATE SET
            asset_type = EXCLUDED.asset_type,
            name       = EXCLUDED.name,
            region     = EXCLUDED.region,
            raw        = EXCLUDED.raw,
            synced_at  = now()
        RETURNING (xmax = 0) AS inserted
        "#,
    )
    .bind(tenant_id)
    .bind(&asset.provider)
    .bind(&asset.external_id)
    .bind(&asset.asset_type)
    .bind(&asset.name)
    .bind(&asset.region)
    .bind(&asset.raw)
    .fetch_one(pool)
    .await
    .with_context(|| format!("upsert asset {}/{}", asset.provider, asset.external_id))?;
    Ok(row.get::<bool, _>("inserted"))
}

/// 列出某租户已归一化的全部资产（按 provider、external_id 排序，稳定输出）。
///
/// **多租户隔离红线**：`WHERE tenant_id = $1`——这是「租户 2 看不到租户 1 资产」的唯一保证，
/// 少了它就是跨租户数据泄露。
pub async fn list_assets(pool: &PgPool, tenant_id: i64) -> Result<Vec<CloudAsset>> {
    let rows = sqlx::query(
        r#"
        SELECT provider, asset_type, external_id, name, region, raw
        FROM cloudsync.assets
        WHERE tenant_id = $1
        ORDER BY provider, external_id
        "#,
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await
    .context("list assets")?;

    Ok(rows
        .into_iter()
        .map(|row| CloudAsset {
            provider: row.get("provider"),
            asset_type: row.get("asset_type"),
            external_id: row.get("external_id"),
            name: row.get("name"),
            region: row.get("region"),
            raw: row.get("raw"),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_total_sums_insert_and_update() {
        let r = SyncReport {
            tenant_id: 1,
            ran: true,
            credentials: 3,
            inserted: 4,
            updated: 1,
            rate_limited: 0,
            unsupported: 0,
        };
        assert_eq!(r.total(), 5);
    }

    #[test]
    fn report_serializes_to_json() {
        // HTTP 路由要把报告序列化成 JSON 返回，这里断言字段名如预期。
        let r = SyncReport {
            tenant_id: 7,
            ran: true,
            ..Default::default()
        };
        let v: serde_json::Value = serde_json::to_value(&r).unwrap();
        assert_eq!(v["tenant_id"], 7);
        assert_eq!(v["ran"], true);
        assert_eq!(v["inserted"], 0);
    }
}
