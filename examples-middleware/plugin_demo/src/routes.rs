//! 最小路由：把「应用商店」与「插件」两套能力各暴露成几个端点。
//!
//! - `GET  /apps`                商店目录
//! - `POST /apps/{id}/install`   填参数 → 渲染 compose → 记一条安装记录（不真起容器）
//! - `GET  /installed`           已安装记录
//! - `GET  /plugins`             已装插件清单
//! - `POST /plugins/{name}/call` 调用某插件的一个方法（转成 JSON-RPC 发给子进程）

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::appstore::{self, AppManifest, AppStore, InstalledApp};
use crate::error::{AppError, AppResult};
use crate::plugin::{authorize, PluginError, PluginHost, PluginManifest};

/// 已装插件的注册表：清单（给 `/plugins` 列）+ 活着的宿主进程（给 `/plugins/{name}/call` 用）。
/// 每个 [`PluginHost`] 的 `call` 需要 `&mut`，所以整张表放在一把 `tokio::Mutex` 后面，
/// 调用期间串行化——教学够用；高并发下可改成「每插件一把锁」或进程池。
#[derive(Clone, Default)]
pub struct PluginRegistry {
    inner: Arc<Mutex<Registry>>,
}

#[derive(Default)]
struct Registry {
    manifests: Vec<PluginManifest>,
    hosts: HashMap<String, PluginHost>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 安装一个插件：先按 `granted` 做能力授权，通过后 spawn 进程并登记。
    pub async fn install(
        &self,
        manifest: PluginManifest,
        granted: &[&str],
    ) -> Result<(), PluginError> {
        authorize(&manifest, granted)?;
        let host = PluginHost::spawn(&manifest).await?;
        let mut reg = self.inner.lock().await;
        reg.hosts.insert(manifest.name.clone(), host);
        reg.manifests.push(manifest);
        Ok(())
    }

    pub async fn manifests(&self) -> Vec<PluginManifest> {
        self.inner.lock().await.manifests.clone()
    }

    /// 调用某插件的方法。找不到插件 → `Protocol`（路由层再翻成 404）。
    pub async fn call(
        &self,
        name: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, PluginError> {
        let mut reg = self.inner.lock().await;
        let host = reg
            .hosts
            .get_mut(name)
            .ok_or_else(|| PluginError::Protocol(format!("插件未安装: {name}")))?;
        host.call(method, params).await
    }
}

/// 全应用共享状态：应用商店的安装记录 + 插件注册表。
#[derive(Clone, Default)]
pub struct AppState {
    pub apps: AppStore,
    pub plugins: PluginRegistry,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/apps", get(list_apps))
        .route("/apps/{id}/install", post(install_app))
        .route("/installed", get(list_installed))
        .route("/plugins", get(list_plugins))
        .route("/plugins/{name}/call", post(call_plugin))
}

// ---------- 应用商店 ----------

async fn list_apps() -> Json<Vec<AppManifest>> {
    Json(appstore::catalog())
}

#[derive(Deserialize)]
struct InstallRequest {
    /// 这次安装的实例名（一份应用可装多个实例）。
    instance_name: String,
    /// 用户填的参数。
    #[serde(default)]
    values: HashMap<String, String>,
}

#[derive(Serialize)]
struct InstallResponse {
    /// 渲染出的 compose（真流程把它交给 `docker compose up -d` 就装好了）。
    compose: String,
    installed: InstalledApp,
}

/// POST /apps/{id}/install
async fn install_app(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<InstallRequest>,
) -> AppResult<Json<InstallResponse>> {
    let manifest = appstore::find(&id).ok_or_else(|| AppError::not_found(format!("应用不存在: {id}")))?;

    // 渲染 = 一键安装的第一步。校验不过（缺参/未知占位/注入）会在这里被 render 拦下。
    let compose = appstore::render_compose(&manifest, &req.values)?;

    // 记录安装（真流程此处才 docker compose up；本示例只记「已渲染」）。
    let values = req.values.into_iter().collect();
    let installed = state.apps.record(&id, &req.instance_name, values);

    Ok(Json(InstallResponse { compose, installed }))
}

async fn list_installed(State(state): State<AppState>) -> Json<Vec<InstalledApp>> {
    Json(state.apps.list())
}

// ---------- 插件 ----------

async fn list_plugins(State(state): State<AppState>) -> Json<Vec<PluginManifest>> {
    Json(state.plugins.manifests().await)
}

#[derive(Deserialize)]
struct CallRequest {
    method: String,
    #[serde(default)]
    params: Value,
}

/// POST /plugins/{name}/call {method, params} → 转成 JSON-RPC 发给插件子进程，回传 result。
async fn call_plugin(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<CallRequest>,
) -> AppResult<Json<Value>> {
    match state.plugins.call(&name, &req.method, req.params).await {
        Ok(result) => Ok(Json(result)),
        // 「插件未安装」翻成 404，其余插件错误由 From<PluginError> 归类（能力问题 400，其余 502）。
        Err(PluginError::Protocol(msg)) if msg.contains("插件未安装") => {
            Err(AppError::not_found(msg))
        }
        Err(e) => Err(e.into()),
    }
}
