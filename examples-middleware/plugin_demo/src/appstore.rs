//! 应用商店（1Panel 式）。
//!
//! 核心认知：**面板里的「应用」= 一份 docker-compose 模板 + 一组待填参数**。
//! 用户在界面上填几个参数（密码、端口、卷路径……），面板把参数渲染进模板，再 `docker compose up`
//! 就把应用装好了。所以「一键安装」拆开看只有两步：**渲染模板** + **起容器**。
//!
//! 本模块只实现「渲染 + 记录」这段纯逻辑（可穷尽单测），真正的 `docker compose up` 交给文档演示——
//! 教学 crate 不该依赖本机装了 Docker。重点讲清一个容易被忽略的安全点：**模板注入**。
//!
//! 模板注入：compose 是 YAML，用户填的参数会被原样插进去。如果不校验，用户填一个带换行的值
//! （比如把密码填成 `x\n    privileged: true`），就能往容器定义里注入任意字段、拿到特权容器。
//! 所以 render 前必须校验每个用户值，并且模板里出现的占位符必须都是「声明过的参数」——
//! 不能让一份被篡改的模板引用未知变量。

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// 应用需要用户填写的一个参数。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppParam {
    /// 模板里的占位符名，对应 `{{key}}`。
    pub key: String,
    /// 界面上给人看的标签。
    pub label: String,
    /// 默认值。用户没填时用它；配合 required 决定「没填也没默认」时是否报错。
    pub default: Option<String>,
    /// 是否必填。required 且既无用户值也无默认 → 渲染报错。
    pub required: bool,
}

/// 一个应用的清单：元信息 + compose 模板 + 参数表。相当于 1Panel 应用目录里的一个条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// docker-compose 模板，用 `{{key}}` 作占位符。渲染时被参数替换。
    pub compose_template: String,
    pub params: Vec<AppParam>,
}

impl AppManifest {
    fn param(&self, key: &str) -> Option<&AppParam> {
        self.params.iter().find(|p| p.key == key)
    }
}

/// 内置应用目录：这里放几个 mock 应用，够演示渲染与校验的各种分支。
/// 真实的 1Panel 是从远端应用仓库拉 manifest，这里内联成常量，纯离线。
pub fn catalog() -> Vec<AppManifest> {
    vec![
        AppManifest {
            id: "postgres".into(),
            name: "PostgreSQL".into(),
            version: "16".into(),
            description: "关系型数据库，安装即得一个带持久卷的实例".into(),
            // 注意所有会插入用户值的地方都用引号包起来，配合 render 的值校验，双保险防注入。
            compose_template: "services:\n  \
                {{instance}}:\n    \
                image: postgres:16\n    \
                environment:\n      \
                POSTGRES_PASSWORD: \"{{password}}\"\n      \
                POSTGRES_DB: \"{{db}}\"\n    \
                ports:\n      \
                - \"{{port}}:5432\"\n    \
                volumes:\n      \
                - {{instance}}_data:/var/lib/postgresql/data\n\
                volumes:\n  \
                {{instance}}_data:\n"
                .into(),
            params: vec![
                AppParam {
                    key: "instance".into(),
                    label: "实例名".into(),
                    default: Some("pg".into()),
                    required: false,
                },
                AppParam {
                    key: "password".into(),
                    label: "数据库密码".into(),
                    default: None,
                    required: true,
                },
                AppParam {
                    key: "db".into(),
                    label: "初始数据库名".into(),
                    default: Some("app".into()),
                    required: false,
                },
                AppParam {
                    key: "port".into(),
                    label: "宿主机端口".into(),
                    default: Some("5432".into()),
                    required: false,
                },
            ],
        },
        AppManifest {
            id: "static-site".into(),
            name: "静态站点（Nginx）".into(),
            version: "1.27".into(),
            description: "把一个本地目录挂进 Nginx 当静态站点发布".into(),
            compose_template: "services:\n  \
                {{instance}}:\n    \
                image: nginx:1.27-alpine\n    \
                ports:\n      \
                - \"{{port}}:80\"\n    \
                volumes:\n      \
                - {{webroot}}:/usr/share/nginx/html:ro\n"
                .into(),
            params: vec![
                AppParam {
                    key: "instance".into(),
                    label: "实例名".into(),
                    default: Some("site".into()),
                    required: false,
                },
                AppParam {
                    key: "port".into(),
                    label: "宿主机端口".into(),
                    default: Some("8080".into()),
                    required: false,
                },
                AppParam {
                    key: "webroot".into(),
                    label: "站点根目录（宿主机路径）".into(),
                    default: None,
                    required: true,
                },
            ],
        },
    ]
}

/// 按 id 找一个应用清单。
pub fn find(id: &str) -> Option<AppManifest> {
    catalog().into_iter().find(|m| m.id == id)
}

/// 扫出模板里所有 `{{key}}` 占位符（去重前的原始序列）。
/// `{{` 未闭合视为坏模板——宁可报错也不要渲染出半截东西。
fn placeholders(template: &str) -> AppResult<Vec<String>> {
    let mut keys = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let end = after
            .find("}}")
            .ok_or_else(|| AppError::bad_request("模板占位符 {{ 未闭合"))?;
        keys.push(after[..end].trim().to_string());
        rest = &after[end + 2..];
    }
    Ok(keys)
}

/// 校验单个用户值，挡住模板注入。教学取「拒绝控制字符（含换行）与模板定界符」这条底线：
/// 换行是 YAML 的结构分隔符，禁掉换行就堵死了「注入新字段」这条最危险的路；
/// 禁掉 `{{`/`}}` 则防止值里再塞占位符导致二次替换。
///
/// 生产更稳妥的做法是「用 YAML 序列化库输出、让它负责转义/加引号」，而不是字符串替换 +
/// 手动校验。这里为了教学直观才用字符串替换，务必记得它的边界。
fn validate_value(key: &str, val: &str) -> AppResult<()> {
    if let Some(bad) = val.chars().find(|c| c.is_control()) {
        return Err(AppError::bad_request(format!(
            "参数 {key} 含非法控制字符（如换行）U+{:04X}，可能破坏 compose 结构",
            bad as u32
        )));
    }
    if val.contains("{{") || val.contains("}}") {
        return Err(AppError::bad_request(format!(
            "参数 {key} 不允许包含模板定界符 {{{{ 或 }}}}"
        )));
    }
    Ok(())
}

/// 把参数渲染进 compose 模板。这是「一键安装」的第一步（第二步是把结果交给 docker compose）。
///
/// 依次做四件事，任何一步不过都拒绝渲染：
/// 1. 模板里出现的占位符必须都在参数表里声明过（挡下被篡改/引用未知变量的模板）；
/// 2. 用户传入的 key 必须都是声明过的参数（挡下拼写错误与注入未知字段）；
/// 3. 每个参数求值：优先用户值，其次默认值；required 且两者皆无 → 报错；
/// 4. 每个最终值先过 [`validate_value`] 防注入，再替换进模板。
pub fn render_compose(
    manifest: &AppManifest,
    values: &HashMap<String, String>,
) -> AppResult<String> {
    // 1. 模板占位符必须都已声明。
    for ph in placeholders(&manifest.compose_template)? {
        if manifest.param(&ph).is_none() {
            return Err(AppError::bad_request(format!(
                "模板出现未声明的占位符 {{{{{ph}}}}}"
            )));
        }
    }

    // 2. 用户传入的 key 必须都是声明过的参数。
    for k in values.keys() {
        if manifest.param(k).is_none() {
            return Err(AppError::bad_request(format!("未知参数 {k}")));
        }
    }

    // 3. 求值 + 4. 校验并替换。
    let mut out = manifest.compose_template.clone();
    for p in &manifest.params {
        let value = match values.get(&p.key).or(p.default.as_ref()) {
            Some(v) => v.clone(),
            None if p.required => {
                return Err(AppError::bad_request(format!("缺少必填参数 {}", p.key)));
            }
            // 非必填且无默认：置空串（模板里通常不会引用这种参数）。
            None => String::new(),
        };
        validate_value(&p.key, &value)?;
        out = out.replace(&format!("{{{{{}}}}}", p.key), &value);
    }

    Ok(out)
}

/// 一条安装记录。真实面板会把它落库，这里放内存。
#[derive(Debug, Clone, Serialize)]
pub struct InstalledApp {
    pub app_id: String,
    pub instance_name: String,
    /// 安装时用户填的参数（BTreeMap 让序列化顺序稳定，便于测试与观察）。
    pub values: BTreeMap<String, String>,
    /// 安装时间（Unix 秒）。
    pub installed_at: u64,
    /// 状态。本示例渲染完即记为 "rendered"（尚未真起容器）；真流程起完记 "running"。
    pub status: String,
}

/// 内存版安装记录仓库，可跨 axum handler 共享（`Arc<Mutex<..>>`）。
#[derive(Clone, Default)]
pub struct AppStore {
    installed: Arc<Mutex<Vec<InstalledApp>>>,
}

impl AppStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 记录一次安装，返回这条记录。
    pub fn record(
        &self,
        app_id: &str,
        instance_name: &str,
        values: BTreeMap<String, String>,
    ) -> InstalledApp {
        let app = InstalledApp {
            app_id: app_id.to_string(),
            instance_name: instance_name.to_string(),
            values,
            installed_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            status: "rendered".to_string(),
        };
        self.installed.lock().expect("锁未中毒").push(app.clone());
        app
    }

    /// 列出所有安装记录。
    pub fn list(&self) -> Vec<InstalledApp> {
        self.installed.lock().expect("锁未中毒").clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vals(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn catalog_not_empty_and_ids_unique() {
        let c = catalog();
        assert!(!c.is_empty());
        let mut ids: Vec<_> = c.iter().map(|m| m.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), c.len(), "应用 id 必须唯一");
    }

    #[test]
    fn manifest_roundtrips_json() {
        let m = find("postgres").unwrap();
        let s = serde_json::to_string(&m).unwrap();
        let back: AppManifest = serde_json::from_str(&s).unwrap();
        assert_eq!(back.id, "postgres");
        assert_eq!(back.params, m.params);
    }

    #[test]
    fn render_substitutes_all_placeholders() {
        let m = find("postgres").unwrap();
        let out = render_compose(&m, &vals(&[("password", "s3cret"), ("port", "6543")])).unwrap();
        // 用户值进去了，默认值（instance=pg / db=app）也进去了。
        assert!(out.contains("POSTGRES_PASSWORD: \"s3cret\""));
        assert!(out.contains("- \"6543:5432\""));
        assert!(out.contains("POSTGRES_DB: \"app\""));
        assert!(out.contains("pg_data:/var/lib/postgresql/data"));
        // 渲染后不应残留任何占位符。
        assert!(!out.contains("{{"), "还残留占位符: {out}");
    }

    #[test]
    fn missing_required_param_is_rejected() {
        let m = find("postgres").unwrap();
        // password 必填且无默认，不给就应报错。
        let err = render_compose(&m, &vals(&[("port", "5432")])).unwrap_err();
        assert!(matches!(err, AppError::BadRequest(msg) if msg.contains("password")));
    }

    #[test]
    fn unknown_param_key_is_rejected() {
        let m = find("postgres").unwrap();
        let err =
            render_compose(&m, &vals(&[("password", "x"), ("totally_unknown", "y")])).unwrap_err();
        assert!(matches!(err, AppError::BadRequest(msg) if msg.contains("未知参数")));
    }

    #[test]
    fn unknown_placeholder_in_template_is_rejected() {
        // 一份被做坏的模板：引用了参数表里没有的 {{secret}}。
        let m = AppManifest {
            id: "bad".into(),
            name: "bad".into(),
            version: "0".into(),
            description: "".into(),
            compose_template: "image: x\nenv: {{secret}}\n".into(),
            params: vec![],
        };
        let err = render_compose(&m, &HashMap::new()).unwrap_err();
        assert!(matches!(err, AppError::BadRequest(msg) if msg.contains("未声明的占位符")));
    }

    #[test]
    fn injection_via_newline_is_rejected() {
        let m = find("postgres").unwrap();
        // 攻击者想借密码换行注入 privileged: true。
        let malicious = "x\"\n    privileged: true\n    x: \"";
        let err = render_compose(&m, &vals(&[("password", malicious)])).unwrap_err();
        assert!(matches!(err, AppError::BadRequest(msg) if msg.contains("控制字符")));
    }

    #[test]
    fn store_records_and_lists() {
        let store = AppStore::new();
        assert!(store.list().is_empty());
        let mut vs = BTreeMap::new();
        vs.insert("password".to_string(), "x".to_string());
        let rec = store.record("postgres", "pg", vs);
        assert_eq!(rec.status, "rendered");
        assert_eq!(store.list().len(), 1);
        assert_eq!(store.list()[0].app_id, "postgres");
    }
}
