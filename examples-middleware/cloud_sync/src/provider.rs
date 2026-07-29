//! 多云抽象（第 6 章 trait 的实战）：把「各家云千差万别的资产接口」收敛到一个
//! 统一 trait 后面，上层同步引擎只跟 trait 打交道，感知不到底下是阿里云还是 AWS。
//!
//! ## 为什么需要归一化
//!
//! 阿里云 ECS 的实例叫 `InstanceId` + `RegionId`，AWS EC2 叫 `InstanceId` + 放在
//! ARN 里的区域，私有云可能干脆叫 `vm_uuid` + 自定义机房编号……字段名、层级、
//! 甚至「一个实例」的定义都不一样。资产管理系统的**核心价值**就是把这些差异抹平成
//! 一个统一模型 [`CloudAsset`]，让「跨云盘点有多少台机器」这种问题有唯一答案。
//! 原始报文塞进 `raw` 字段留档（排查/审计时能回看云厂商到底返回了什么）。
//!
//! ## async fn in trait 的 dyn 兼容取舍
//!
//! edition 2024 里 trait 内 `async fn`（AFIT）已稳定，写起来最顺。但 [`provider_registry`]
//! 要按名字返回 `Box<dyn CloudProvider>` 做动态分发，而**含 `async fn` 的 trait 目前不是
//! dyn 兼容的**（返回的匿名 Future 类型无法擦除）。两条常见出路：
//! 1. 引 `async-trait` 宏——但那会给每次调用加一次堆分配，且多一个依赖；
//! 2. **手写**：让 trait 方法直接返回 `Pin<Box<dyn Future>>`（把「装箱」这件事从宏挪到
//!    手上）。这样 trait 天然 dyn 兼容，也不引新依赖。
//!
//! 本示例选 2：`list_assets` 的返回类型显式写成 `Pin<Box<dyn Future<...> + Send + '_>>`，
//! 每个实现体用 `Box::pin(async move { ... })` 收尾。代价是签名啰嗦一点，换来的是
//! 「一个 trait 对象存所有云」的动态注册表。

use std::future::Future;
use std::pin::Pin;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::credentials::Credential;

/// 归一化后的统一资产模型——各云返回结构千差万别，这里是抹平差异后的样子。
///
/// - `provider`：来自哪朵云（aliyun / aws / private），资产表里靠它 + external_id 区分同名资源；
/// - `asset_type`：资产种类（ecs / oss_bucket / ec2 / vm ……），跨云对齐成小写下划线风格；
/// - `external_id`：该资产在**云厂商侧**的唯一 id（实例 id / 桶名），upsert 的冲突键之一；
/// - `raw`：云厂商原始报文，原样留档，归一化丢掉的细节都还在这里。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CloudAsset {
    pub provider: String,
    pub asset_type: String,
    pub external_id: String,
    pub name: String,
    pub region: String,
    pub raw: serde_json::Value,
}

/// 多云统一接口：一朵云 = 一个实现。
///
/// 新增一朵云 = 加一个 `impl CloudProvider` + 在 [`provider_registry`] 里注册一行，
/// 上层同步引擎一个字都不用改——这就是开闭原则（对扩展开放、对修改关闭）。
///
/// `list_assets` 手写成返回 `Pin<Box<dyn Future>>`（理由见模块头）：等价于
/// `async fn list_assets(&self, cred: &Credential) -> Result<Vec<CloudAsset>>`，
/// 只是为了让 `dyn CloudProvider` 能装进注册表。
pub trait CloudProvider: Send + Sync {
    /// provider 的稳定名字（注册表的 key、也写进 CloudAsset.provider）。
    fn name(&self) -> &str;

    /// 用给定凭证拉取该账号下的资产。真实实现这里会用 reqwest 调各家 OpenAPI + 请求签名
    /// （阿里云 ROA/RPC 签名、AWS SigV4……），mock 实现直接返回一批假数据，
    /// 好处是**离线可跑可测**，不依赖真实云账号与网络。
    fn list_assets<'a>(
        &'a self,
        cred: &'a Credential,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<CloudAsset>>> + Send + 'a>>;
}

// ---------------------------------------------------------------------------
// 三个 mock 实现
// ---------------------------------------------------------------------------

/// 阿里云：返回一批假的 ECS 实例。真实实现调 `DescribeInstances`（RPC 风格 + HMAC 签名）。
pub struct AliyunProvider;

impl CloudProvider for AliyunProvider {
    fn name(&self) -> &str {
        "aliyun"
    }

    fn list_assets<'a>(
        &'a self,
        cred: &'a Credential,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<CloudAsset>>> + Send + 'a>> {
        Box::pin(async move {
            // 假数据也做得「结构合理」：阿里云 ECS 的字段名（InstanceId/RegionId/Status）
            // 尽量贴近真实报文，好让读者看清「原始报文 → 归一化」到底抹平了什么。
            let assets = vec![
                CloudAsset {
                    provider: self.name().to_string(),
                    asset_type: "ecs".to_string(),
                    external_id: "i-bp1aliyun001".to_string(),
                    name: "web-server-1".to_string(),
                    region: "cn-hangzhou".to_string(),
                    raw: json!({
                        "InstanceId": "i-bp1aliyun001",
                        "InstanceName": "web-server-1",
                        "RegionId": "cn-hangzhou",
                        "Status": "Running",
                        "InstanceType": "ecs.g7.large",
                        "_note": "阿里云 DescribeInstances 的原始结构（此处为 mock）"
                    }),
                },
                CloudAsset {
                    provider: self.name().to_string(),
                    asset_type: "oss_bucket".to_string(),
                    external_id: "oss-cn-hangzhou-assets".to_string(),
                    name: "assets-bucket".to_string(),
                    region: "cn-hangzhou".to_string(),
                    raw: json!({
                        "Name": "assets-bucket",
                        "Location": "oss-cn-hangzhou",
                        "StorageClass": "Standard"
                    }),
                },
            ];
            // 真实实现里 cred.access_key/secret_key 会参与请求签名；mock 里只做个存在性说明。
            tracing::debug!(provider = self.name(), ak = %cred.access_key, "mock list_assets");
            Ok(assets)
        })
    }
}

/// AWS：返回一批假的 EC2 实例。真实实现调 `DescribeInstances`（SigV4 签名）。
pub struct AwsProvider;

impl CloudProvider for AwsProvider {
    fn name(&self) -> &str {
        "aws"
    }

    fn list_assets<'a>(
        &'a self,
        cred: &'a Credential,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<CloudAsset>>> + Send + 'a>> {
        Box::pin(async move {
            let assets = vec![
                CloudAsset {
                    provider: self.name().to_string(),
                    asset_type: "ec2".to_string(),
                    external_id: "i-0aws1234567890".to_string(),
                    name: "api-gateway".to_string(),
                    region: "us-east-1".to_string(),
                    // AWS 的实例名藏在 Tags 里（Key=Name），归一化时要把它抽到 name 字段——
                    // 这正是「统一模型」要处理的典型差异。
                    raw: json!({
                        "InstanceId": "i-0aws1234567890",
                        "Placement": { "AvailabilityZone": "us-east-1a" },
                        "State": { "Name": "running" },
                        "InstanceType": "t3.medium",
                        "Tags": [ { "Key": "Name", "Value": "api-gateway" } ]
                    }),
                },
                CloudAsset {
                    provider: self.name().to_string(),
                    asset_type: "s3_bucket".to_string(),
                    external_id: "my-org-logs".to_string(),
                    name: "my-org-logs".to_string(),
                    region: "us-east-1".to_string(),
                    raw: json!({ "Name": "my-org-logs", "CreationDate": "2024-01-01T00:00:00Z" }),
                },
            ];
            tracing::debug!(provider = self.name(), ak = %cred.access_key, "mock list_assets");
            Ok(assets)
        })
    }
}

/// 私有云：返回一批假的 VM，并演示「自定义 endpoint」——公有云 endpoint 是固定的，
/// 私有云/专有云部署在客户自己机房，接入地址因客户而异，必须由凭证带进来。
pub struct PrivateCloudProvider;

impl CloudProvider for PrivateCloudProvider {
    fn name(&self) -> &str {
        "private"
    }

    fn list_assets<'a>(
        &'a self,
        cred: &'a Credential,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<CloudAsset>>> + Send + 'a>> {
        Box::pin(async move {
            // 私有云没有固定 endpoint：真实实现会 `GET {endpoint}/api/v1/vms`。
            // 这里把 endpoint 回填进 raw，演示它确实被用上了；缺省给个占位说明配置缺失。
            let endpoint = cred
                .endpoint
                .clone()
                .unwrap_or_else(|| "<未配置 endpoint>".to_string());
            let assets = vec![CloudAsset {
                provider: self.name().to_string(),
                asset_type: "vm".to_string(),
                external_id: "vm-priv-0001".to_string(),
                name: "db-primary".to_string(),
                region: "idc-shanghai-a".to_string(),
                raw: json!({
                    "vm_uuid": "vm-priv-0001",
                    "hostname": "db-primary",
                    "zone": "idc-shanghai-a",
                    "endpoint": endpoint,
                    "vcpu": 8,
                    "mem_gb": 32
                }),
            }];
            tracing::debug!(provider = self.name(), ak = %cred.access_key, "mock list_assets");
            Ok(assets)
        })
    }
}

/// 按名字取 provider（动态分发的入口）。
///
/// 返回 `Box<dyn CloudProvider>`：调用方拿到的是「某朵云」的抽象句柄，不关心具体类型。
/// 名字不认识就返回 `None`（比如库里存了一条 provider="gcp" 的凭证但还没实现 GCP），
/// 同步引擎据此跳过而不是 panic——多云系统必须容忍「有凭证但暂不支持的云」。
///
/// 新增一朵云只需在这里加一行 `"xxx" => Some(Box::new(XxxProvider))`——开闭原则的落点。
pub fn provider_registry(name: &str) -> Option<Box<dyn CloudProvider>> {
    match name {
        "aliyun" => Some(Box::new(AliyunProvider)),
        "aws" => Some(Box::new(AwsProvider)),
        "private" => Some(Box::new(PrivateCloudProvider)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_cred(provider: &str, endpoint: Option<&str>) -> Credential {
        Credential {
            tenant_id: 1,
            provider: provider.to_string(),
            access_key: "AK_test".to_string(),
            secret_key: "SK_test".to_string(),
            endpoint: endpoint.map(str::to_string),
        }
    }

    #[tokio::test]
    async fn aliyun_returns_reasonable_assets() {
        let p = AliyunProvider;
        let assets = p.list_assets(&dummy_cred("aliyun", None)).await.unwrap();
        assert!(!assets.is_empty(), "mock 应返回非空资产");
        // 字段合理性：provider 名对得上、external_id/name/region 都非空。
        for a in &assets {
            assert_eq!(a.provider, "aliyun");
            assert!(!a.external_id.is_empty());
            assert!(!a.name.is_empty());
            assert!(!a.region.is_empty());
        }
        // 至少有一台 ECS。
        assert!(assets.iter().any(|a| a.asset_type == "ecs"));
    }

    #[tokio::test]
    async fn aws_extracts_name_from_tags() {
        let p = AwsProvider;
        let assets = p.list_assets(&dummy_cred("aws", None)).await.unwrap();
        let ec2 = assets.iter().find(|a| a.asset_type == "ec2").unwrap();
        // 归一化的价值：name 从 Tags 里的 Name 抽到了顶层。
        assert_eq!(ec2.name, "api-gateway");
        assert_eq!(ec2.provider, "aws");
    }

    #[tokio::test]
    async fn private_cloud_uses_custom_endpoint() {
        let p = PrivateCloudProvider;
        let ep = "https://cloud.customer-idc.example";
        let assets = p.list_assets(&dummy_cred("private", Some(ep))).await.unwrap();
        // 自定义 endpoint 被用上了（回填进 raw）。
        assert_eq!(assets[0].raw["endpoint"], ep);
    }

    #[tokio::test]
    async fn registry_lookup_dispatches_by_name() {
        // 三朵已实现的云都能取到，且取到的 provider 名字对得上。
        for name in ["aliyun", "aws", "private"] {
            let p = provider_registry(name).unwrap_or_else(|| panic!("{name} 应已注册"));
            assert_eq!(p.name(), name);
            // 通过 dyn 句柄调 async 方法也应正常工作。
            let assets = p.list_assets(&dummy_cred(name, Some("https://x"))).await.unwrap();
            assert!(!assets.is_empty());
        }
        // 未实现的云返回 None，不 panic。
        assert!(provider_registry("gcp").is_none());
    }
}
