//! 指标采集与模型（第 28 章配套）。
//!
//! 分两半：
//! - **采集**（[`Collector`] / [`collect_system`]）：用 `sysinfo` 读当前机器的
//!   CPU / 内存 / 磁盘 / 负载，装进统一的 [`Metric`]。这部分与运行环境强相关，
//!   只能做「宽松断言」的测试（值在合理范围、非空）。
//! - **渲染**（[`render_prometheus`]）：把一批 [`Metric`] 渲染成 Prometheus 文本格式。
//!   它是**纯函数**——同样的输入永远得到同样的输出，可以穷尽单测（含 label 转义、
//!   `# HELP` / `# TYPE` 行）。
//!
//! ## 为什么用 Prometheus 文本格式
//!
//! Prometheus 是云原生监控的**事实标准**：约定一个 HTTP 端点（惯例 `/metrics`）返回
//! 一段纯文本，每行是 `指标名{标签="值",...} 数值`。Prometheus server 按固定间隔来
//! 「抓取」（pull）这个端点，存进它的时序库，Grafana 画图、Alertmanager 告警都直接
//! 接这套数据。只要你按这个格式吐字符串，就免费接入了整个生态——不用为每种监控系统
//! 各写一套适配。这就是「遵循事实标准」的杠杆。
//!
//! `# HELP`（人读的说明）和 `# TYPE`（指标类型：counter 只增、gauge 可增可减、
//! histogram 分桶……）是格式的一部分：`# TYPE` 让 Prometheus 知道该怎么聚合这个指标
//! （比如对 counter 要算速率 `rate()`，对 gauge 直接取值），`# HELP` 则显示在 UI 里
//! 帮人理解这个指标是什么。我们采的都是「当前瞬时值」，所以类型统一是 `gauge`。

use serde::{Deserialize, Serialize};

/// 一条指标样本：指标名 + 数值 + 一组标签 + 采集时刻（Unix 毫秒）。
///
/// `labels` 用 `Vec<(String, String)>` 而非 map，是为了**保序**——渲染成
/// Prometheus 文本时标签顺序稳定，输出可复现、便于测试与 diff。Prometheus 本身不
/// 关心标签顺序，但确定性的输出对教学与断言更友好。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metric {
    /// 指标名，如 `cpu_usage_percent`。Prometheus 命名惯例：小写 + 下划线，
    /// 并把单位写进名字（`_percent` / `_bytes` / `_seconds`）。
    pub name: String,
    /// 数值。统一用 `f64`：CPU 百分比、字节数、负载都能装下。
    pub value: f64,
    /// 维度标签，如 `("mount", "/")`。让同名指标能按维度细分（哪块盘、哪台主机）。
    pub labels: Vec<(String, String)>,
    /// 采集时刻（Unix 毫秒）。既用于前端排序，也是告警 [`fired_at`](crate::alert::Alert)
    /// 的来源——告警是「基于这一刻的样本」判定的。
    pub ts_ms: i64,
}

impl Metric {
    /// 便捷构造：用当前时间作为 `ts_ms`。
    pub fn now(name: impl Into<String>, value: f64, labels: Vec<(String, String)>) -> Self {
        Self {
            name: name.into(),
            value,
            labels,
            ts_ms: now_millis(),
        }
    }
}

/// 当前 Unix 毫秒时间戳（与 realtime_demo 的 `now_millis` 同一套做法）。
pub fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 持有 `sysinfo` 句柄的采集器，供**长期运行**的后台任务复用（见 [`crate::spawn_collector`]）。
///
/// 为什么要持有而不是每次新建：`sysinfo` 读 **CPU 使用率**是「两次采样求差」——它比较
/// 「这次读到的累计 CPU 时间」和「上次读到的」之间的增量，才能算出这段间隔内的使用率。
/// 所以**第一次**刷新拿到的 CPU 使用率总是 0（还没有「上一次」可比）。后台任务每 2 秒
/// 调一次 [`collect`](Collector::collect)，那 2 秒的间隔天然就是采样窗口，既准又**不阻塞**
/// ——不需要在采集函数里 `sleep` 等一个间隔出来。
pub struct Collector {
    sys: sysinfo::System,
    disks: sysinfo::Disks,
}

impl Collector {
    /// 新建采集器并做一次初始刷新（此刻 CPU 使用率还是 0，属正常）。
    pub fn new() -> Self {
        let mut sys = sysinfo::System::new();
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        let disks = sysinfo::Disks::new_with_refreshed_list();
        Self { sys, disks }
    }

    /// 采集一次，返回当前所有指标。
    ///
    /// 采集要**快、别阻塞**：这里只做几次内存/系统调用级的刷新，微秒到毫秒级即可返回，
    /// 不做任何网络或磁盘 IO 密集操作。若将来要采「很重」的指标（比如逐进程扫描），
    /// 应放进 `tokio::task::spawn_blocking`，别占用异步 worker 线程。
    pub fn collect(&mut self) -> Vec<Metric> {
        // 只刷新我们要用的部分，省开销。CPU 使用率是相对「上一次刷新」的增量。
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.disks.refresh(true);

        let ts = now_millis();
        let mut out = Vec::new();

        // —— CPU：全局平均使用率（0-100）——
        out.push(Metric {
            name: "cpu_usage_percent".into(),
            value: self.sys.global_cpu_usage() as f64,
            labels: vec![],
            ts_ms: ts,
        });

        // —— 内存：总量 / 已用（字节）+ 已用百分比 ——
        // sysinfo 0.30 起内存单位统一为**字节**（老版本是 KB，是常见踩坑点）。
        let mem_total = self.sys.total_memory();
        let mem_used = self.sys.used_memory();
        out.push(Metric {
            name: "memory_total_bytes".into(),
            value: mem_total as f64,
            labels: vec![],
            ts_ms: ts,
        });
        out.push(Metric {
            name: "memory_used_bytes".into(),
            value: mem_used as f64,
            labels: vec![],
            ts_ms: ts,
        });
        out.push(Metric {
            name: "memory_used_percent".into(),
            value: percent(mem_used as f64, mem_total as f64),
            labels: vec![],
            ts_ms: ts,
        });

        // —— 磁盘：逐个挂载点，用 label `mount` 区分——
        for disk in self.disks.list() {
            let mount = disk.mount_point().to_string_lossy().to_string();
            let total = disk.total_space();
            let avail = disk.available_space();
            let labels = vec![("mount".to_string(), mount)];
            out.push(Metric {
                name: "disk_total_bytes".into(),
                value: total as f64,
                labels: labels.clone(),
                ts_ms: ts,
            });
            out.push(Metric {
                name: "disk_available_bytes".into(),
                value: avail as f64,
                labels: labels.clone(),
                ts_ms: ts,
            });
            out.push(Metric {
                name: "disk_used_percent".into(),
                value: percent((total - avail) as f64, total as f64),
                labels,
                ts_ms: ts,
            });
        }

        // —— 负载：1/5/15 分钟平均（类 Unix 才有；Windows 上 sysinfo 返回 0）——
        let load = sysinfo::System::load_average();
        for (window, v) in [("1", load.one), ("5", load.five), ("15", load.fifteen)] {
            out.push(Metric {
                name: "load_average".into(),
                value: v,
                labels: vec![("window".to_string(), window.to_string())],
                ts_ms: ts,
            });
        }

        out
    }
}

impl Default for Collector {
    fn default() -> Self {
        Self::new()
    }
}

/// 一次性采集的便捷函数：适合命令行 / 测试里「取一次当前指标」。
///
/// 因为是**一次性**的，为了拿到有意义的 CPU 使用率（而不是初始的 0），这里必须先刷新、
/// 等一个最小采样间隔、再刷新——所以它会**阻塞**当前线程约
/// [`MINIMUM_CPU_UPDATE_INTERVAL`](sysinfo::MINIMUM_CPU_UPDATE_INTERVAL)（几百毫秒）。
/// 长期运行的服务不要用它循环，而应持有一个 [`Collector`] 每个 tick 调一次
/// （那样 tick 间隔就是采样窗口，无需 sleep），见本模块开头的说明。
pub fn collect_system() -> Vec<Metric> {
    let mut c = Collector::new();
    // 睡一个最小采样间隔，让下一次 collect 里的 CPU 差值算得出来。
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    c.collect()
}

/// 安全求百分比：分母为 0 时返回 0，避免 NaN/Inf 污染输出。
fn percent(part: f64, whole: f64) -> f64 {
    if whole <= 0.0 {
        0.0
    } else {
        part / whole * 100.0
    }
}

/// 把一批指标渲染成 Prometheus 文本格式（**纯函数**，可穷尽测试）。
///
/// 输出形如：
/// ```text
/// # HELP cpu_usage_percent monitor_demo metric: cpu_usage_percent
/// # TYPE cpu_usage_percent gauge
/// cpu_usage_percent 12.5
/// # HELP disk_used_percent monitor_demo metric: disk_used_percent
/// # TYPE disk_used_percent gauge
/// disk_used_percent{mount="/"} 43.2
/// ```
///
/// 规则要点：
/// - 同名指标的 `# HELP` / `# TYPE` 各只出现一次，放在该名字第一条样本之前
///   （Prometheus 解析器要求每个指标名的 HELP/TYPE 至多一行，重复会报错）。
/// - 标签值必须**转义**：反斜杠 `\` → `\\`、双引号 `"` → `\"`、换行 → `\n`。
///   否则含特殊字符的标签值会破坏格式、甚至被 Prometheus 判为解析错误。
/// - 我们采的都是瞬时值，`# TYPE` 统一写 `gauge`。
pub fn render_prometheus(metrics: &[Metric]) -> String {
    let mut out = String::new();
    // 记录哪些指标名已经写过 HELP/TYPE 头，保证每名只写一次。
    // 用 Vec 而非 HashSet：既能去重又保留「首次出现」的顺序，输出可复现。
    let mut seen: Vec<&str> = Vec::new();

    for m in metrics {
        if !seen.contains(&m.name.as_str()) {
            seen.push(m.name.as_str());
            out.push_str(&format!(
                "# HELP {name} monitor_demo metric: {name}\n# TYPE {name} gauge\n",
                name = m.name
            ));
        }
        out.push_str(&m.name);
        if !m.labels.is_empty() {
            out.push('{');
            for (i, (k, v)) in m.labels.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&format!("{k}=\"{}\"", escape_label_value(v)));
            }
            out.push('}');
        }
        // 数值与指标名之间用一个空格分隔；f64 的默认格式对整数/小数都合适。
        out.push_str(&format!(" {}\n", m.value));
    }
    out
}

/// 转义 Prometheus 标签值：见 [`render_prometheus`] 的规则说明。
fn escape_label_value(v: &str) -> String {
    let mut s = String::with_capacity(v.len());
    for ch in v.chars() {
        match ch {
            '\\' => s.push_str("\\\\"),
            '"' => s.push_str("\\\""),
            '\n' => s.push_str("\\n"),
            other => s.push(other),
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(name: &str, value: f64, labels: &[(&str, &str)]) -> Metric {
        Metric {
            name: name.into(),
            value,
            labels: labels
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            ts_ms: 1_700_000_000_000,
        }
    }

    /// 无标签指标：一行 HELP、一行 TYPE、一行数据。
    #[test]
    fn render_no_labels() {
        let out = render_prometheus(&[m("cpu_usage_percent", 12.5, &[])]);
        assert_eq!(
            out,
            "# HELP cpu_usage_percent monitor_demo metric: cpu_usage_percent\n\
             # TYPE cpu_usage_percent gauge\n\
             cpu_usage_percent 12.5\n"
        );
    }

    /// 带标签指标：标签渲染成 `{k="v"}`，多标签用逗号分隔且保序。
    #[test]
    fn render_with_labels_keeps_order() {
        let out = render_prometheus(&[m(
            "disk_used_percent",
            43.0,
            &[("mount", "/"), ("fs", "apfs")],
        )]);
        assert!(out.ends_with("disk_used_percent{mount=\"/\",fs=\"apfs\"} 43\n"), "实得：{out}");
    }

    /// 同名指标的 HELP/TYPE 只出现一次，且在首条之前。
    #[test]
    fn render_dedups_help_type_per_name() {
        let out = render_prometheus(&[
            m("disk_total_bytes", 100.0, &[("mount", "/")]),
            m("disk_total_bytes", 200.0, &[("mount", "/data")]),
        ]);
        assert_eq!(out.matches("# HELP disk_total_bytes").count(), 1);
        assert_eq!(out.matches("# TYPE disk_total_bytes gauge").count(), 1);
        // 两条数据都在。
        assert!(out.contains("disk_total_bytes{mount=\"/\"} 100\n"));
        assert!(out.contains("disk_total_bytes{mount=\"/data\"} 200\n"));
    }

    /// 标签值转义：反斜杠、双引号、换行都要被转义，否则破坏格式。
    #[test]
    fn render_escapes_label_values() {
        let out = render_prometheus(&[m(
            "disk_total_bytes",
            1.0,
            &[("mount", "C:\\a\"b\nc")],
        )]);
        assert!(
            out.contains(r#"disk_total_bytes{mount="C:\\a\"b\nc"} 1"#),
            "转义结果不对：{out}"
        );
    }

    /// percent 的边界：分母为 0 返回 0，不产生 NaN/Inf。
    #[test]
    fn percent_guards_zero_denominator() {
        assert_eq!(percent(1.0, 0.0), 0.0);
        assert_eq!(percent(50.0, 200.0), 25.0);
    }

    /// Metric 的 JSON round-trip（「消息即契约」的最小保证，前端要按这份 schema 解析）。
    #[test]
    fn metric_json_round_trip() {
        let m0 = m("cpu_usage_percent", 12.5, &[("host", "web-1")]);
        let s = serde_json::to_string(&m0).unwrap();
        let back: Metric = serde_json::from_str(&s).unwrap();
        assert_eq!(m0, back);
    }

    /// 环境相关的**宽松**断言：真机采一次，指标非空且各值落在合理范围。
    /// 之所以「宽松」——CPU/内存随机器和时刻变化，只能断言「不越界」而非具体值。
    #[test]
    fn collect_system_returns_sane_metrics() {
        let metrics = collect_system();
        assert!(!metrics.is_empty(), "采集结果不应为空");

        for m in &metrics {
            assert!(m.value.is_finite(), "{} 的值不应是 NaN/Inf", m.name);
            assert!(m.ts_ms > 0, "时间戳应为正");
            match m.name.as_str() {
                // 各种「百分比」都应落在 0..=100（留一点浮点余量）。
                n if n.ends_with("_percent") => {
                    assert!(
                        (0.0..=100.5).contains(&m.value),
                        "{n} 百分比越界：{}",
                        m.value
                    );
                }
                // 字节数、负载不为负。
                _ => assert!(m.value >= 0.0, "{} 不应为负：{}", m.name, m.value),
            }
        }

        // CPU 使用率这一条必然存在。
        assert!(
            metrics.iter().any(|m| m.name == "cpu_usage_percent"),
            "应包含 cpu_usage_percent"
        );
    }
}
