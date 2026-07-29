//! 告警规则与判定（第 28 章配套，**纯逻辑、重点可测**）。
//!
//! 告警的本质很简单：**拿当前指标去和一组阈值规则比一比，超了就产生一条告警**。
//! [`evaluate`] 就是这么一个纯函数——输入「规则 + 当前指标」，输出「触发的告警」，
//! 不碰时间、不碰 IO，因此能对各种比较符和边界（等于阈值、略超、不超）穷尽单测。
//! 这是监控系统里最该被测透的一环：漏报（该响不响）和误报（不该响乱响）都是事故。
//!
//! ## 生产还差什么（这里一句话带过）
//!
//! 真实告警系统在「比阈值」之外还要：**去抖动 / for 持续时间**（连续超过 N 分钟才报，
//! 避免毛刺一闪就惊动人）、**静默 / 抑制**（维护窗口内不报、高优告警触发时压掉相关低优）、
//! **分组 / 聚合**（同类告警合并成一条，别让一次机房故障刷出几百条通知）——这些合称
//! 「防**告警风暴**」，是 Alertmanager 这类组件的核心职责。本示例只做最内核的「判定」，
//! 把这些留给生态。

use serde::{Deserialize, Serialize};

use crate::metrics::Metric;

/// 比较运算符。判定就是 `metric.value <op> threshold`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Op {
    /// 大于（`>`）——最常见：CPU/内存**高于**阈值才危险。
    Gt,
    /// 小于（`<`）——例如「可用磁盘**低于**某值」「健康实例数**低于**下限」。
    Lt,
    /// 大于等于（`>=`）。
    Ge,
    /// 小于等于（`<=`）。
    Le,
}

impl Op {
    /// 对给定值与阈值求值：是否触发。纯比较，无副作用。
    pub fn matches(self, value: f64, threshold: f64) -> bool {
        match self {
            Op::Gt => value > threshold,
            Op::Lt => value < threshold,
            Op::Ge => value >= threshold,
            Op::Le => value <= threshold,
        }
    }
}

/// 一条告警规则：盯住某个指标名，用比较符和阈值判定，命中时按 `severity` 分级。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    /// 要判定的指标名（对应 [`Metric::name`]）。同名多条样本（不同 label）会**逐条**判定。
    pub metric: String,
    /// 比较运算符。
    pub op: Op,
    /// 阈值。
    pub threshold: f64,
    /// 严重级别（如 `"critical"` / `"warning"` / `"info"`）。
    ///
    /// **告警必须分级**：不同级别对应不同的处理路径与打扰程度——critical 半夜电话叫醒
    /// 值班，warning 进群里提醒，info 只记录。级别用字符串而非枚举，是为了让规则可从
    /// 配置文件动态加载、不被代码里的固定枚举限死（生产规则常存在数据库/配置中心）。
    pub severity: String,
}

impl Rule {
    /// 便捷构造。
    pub fn new(metric: impl Into<String>, op: Op, threshold: f64, severity: impl Into<String>) -> Self {
        Self {
            metric: metric.into(),
            op,
            threshold,
            severity: severity.into(),
        }
    }
}

/// 一条触发的告警。字段刻意做得「自解释」，前端/通知渠道拿到就能直接展示。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Alert {
    /// 触发它的规则描述（如 `cpu_usage_percent > 90`），便于人一眼看懂命中了哪条。
    pub rule: String,
    /// 触发时该指标的实际值——附上现场数据，通知里才有说服力。
    pub metric_value: f64,
    /// 触发时刻（Unix 毫秒）。取自**触发它的那条样本**的 `ts_ms`，
    /// 所以 [`evaluate`] 保持纯函数（不读系统时钟），可复现、可测。
    pub fired_at: i64,
    /// 严重级别，直接来自命中的 [`Rule::severity`]。
    pub severity: String,
    /// 面向人的一句话描述，含指标名、标签、实际值与阈值。
    pub message: String,
}

/// 逐条规则对当前指标判定，产出所有触发的告警（**纯函数**）。
///
/// 判定过程：对每条规则，扫描所有**同名**指标样本（同名可能有多条，靠 label 区分，
/// 比如每块磁盘各一条 `disk_used_percent`），逐条用 `op` 比阈值，命中就生成一条
/// [`Alert`]。所以「3 块盘都爆了」会产出 3 条告警，各自带自己的 `mount` 标签——
/// 这正是我们想要的粒度（是哪块盘满了）。
pub fn evaluate(rules: &[Rule], metrics: &[Metric]) -> Vec<Alert> {
    let mut alerts = Vec::new();
    for rule in rules {
        for m in metrics.iter().filter(|m| m.name == rule.metric) {
            if rule.op.matches(m.value, rule.threshold) {
                alerts.push(Alert {
                    rule: format!("{} {} {}", rule.metric, op_symbol(rule.op), rule.threshold),
                    metric_value: m.value,
                    fired_at: m.ts_ms,
                    severity: rule.severity.clone(),
                    message: format!(
                        "[{}] {}{} = {} {} 阈值 {}",
                        rule.severity,
                        rule.metric,
                        render_labels(&m.labels),
                        m.value,
                        op_symbol(rule.op),
                        rule.threshold,
                    ),
                });
            }
        }
    }
    alerts
}

/// 比较符的可读符号，用于 message / rule 描述。
fn op_symbol(op: Op) -> &'static str {
    match op {
        Op::Gt => ">",
        Op::Lt => "<",
        Op::Ge => ">=",
        Op::Le => "<=",
    }
}

/// 把标签渲染成 `{k=v,...}` 附在指标名后（仅用于人读的 message，非 Prometheus 格式）。
fn render_labels(labels: &[(String, String)]) -> String {
    if labels.is_empty() {
        return String::new();
    }
    let inner = labels
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("{{{inner}}}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metric(name: &str, value: f64) -> Metric {
        Metric {
            name: name.into(),
            value,
            labels: vec![],
            ts_ms: 1_700_000_000_000,
        }
    }

    /// 四种比较符在「略超 / 恰好等于 / 不超」三种关系上的真值表——这是告警的命门，穷尽验。
    #[test]
    fn op_matches_truth_table() {
        // Gt：严格大于。等于不算、略超算。
        assert!(!Op::Gt.matches(90.0, 90.0), "等于阈值不应触发 Gt");
        assert!(Op::Gt.matches(90.1, 90.0), "略超应触发 Gt");
        assert!(!Op::Gt.matches(89.9, 90.0), "不超不应触发 Gt");

        // Ge：大于等于。等于就算。
        assert!(Op::Ge.matches(90.0, 90.0), "等于阈值应触发 Ge");
        assert!(Op::Ge.matches(90.1, 90.0));
        assert!(!Op::Ge.matches(89.9, 90.0));

        // Lt：严格小于。
        assert!(!Op::Lt.matches(10.0, 10.0), "等于阈值不应触发 Lt");
        assert!(Op::Lt.matches(9.9, 10.0));
        assert!(!Op::Lt.matches(10.1, 10.0));

        // Le：小于等于。
        assert!(Op::Le.matches(10.0, 10.0), "等于阈值应触发 Le");
        assert!(Op::Le.matches(9.9, 10.0));
        assert!(!Op::Le.matches(10.1, 10.0));
    }

    /// evaluate：超阈值产生告警，且告警字段（值、时间、级别）取自现场。
    #[test]
    fn evaluate_fires_on_breach() {
        let rules = vec![Rule::new("cpu_usage_percent", Op::Gt, 90.0, "critical")];
        let metrics = vec![metric("cpu_usage_percent", 95.0)];

        let alerts = evaluate(&rules, &metrics);
        assert_eq!(alerts.len(), 1);
        let a = &alerts[0];
        assert_eq!(a.severity, "critical");
        assert_eq!(a.metric_value, 95.0);
        assert_eq!(a.fired_at, 1_700_000_000_000, "fired_at 应取自样本的 ts_ms");
        assert!(a.rule.contains("cpu_usage_percent"));
        assert!(a.message.contains("95"));
    }

    /// 不超阈值：不产生告警（含「恰好等于」在 Gt 下不触发的边界）。
    #[test]
    fn evaluate_no_breach_no_alert() {
        let rules = vec![Rule::new("cpu_usage_percent", Op::Gt, 90.0, "critical")];
        assert!(evaluate(&rules, &[metric("cpu_usage_percent", 90.0)]).is_empty());
        assert!(evaluate(&rules, &[metric("cpu_usage_percent", 50.0)]).is_empty());
    }

    /// 同名多样本（多块盘）逐条判定：两块盘都超，产出两条告警，各带自己的标签。
    #[test]
    fn evaluate_per_sample_for_same_name() {
        let rules = vec![Rule::new("disk_used_percent", Op::Ge, 80.0, "warning")];
        let metrics = vec![
            Metric {
                name: "disk_used_percent".into(),
                value: 85.0,
                labels: vec![("mount".into(), "/".into())],
                ts_ms: 1,
            },
            Metric {
                name: "disk_used_percent".into(),
                value: 50.0, // 这块没超
                labels: vec![("mount".into(), "/data".into())],
                ts_ms: 1,
            },
            Metric {
                name: "disk_used_percent".into(),
                value: 95.0,
                labels: vec![("mount".into(), "/logs".into())],
                ts_ms: 1,
            },
        ];
        let alerts = evaluate(&rules, &metrics);
        assert_eq!(alerts.len(), 2, "两块超阈值的盘各产一条");
        assert!(alerts[0].message.contains("mount=/"));
        assert!(alerts[1].message.contains("mount=/logs"));
    }

    /// 规则盯的指标当前不存在：安全跳过，不 panic、不误报。
    #[test]
    fn evaluate_missing_metric_is_skipped() {
        let rules = vec![Rule::new("not_collected", Op::Gt, 0.0, "info")];
        assert!(evaluate(&rules, &[metric("cpu_usage_percent", 100.0)]).is_empty());
    }

    /// Alert 的 JSON round-trip（通知渠道/前端按这份 schema 消费）。
    #[test]
    fn alert_json_round_trip() {
        let rules = vec![Rule::new("memory_used_percent", Op::Gt, 90.0, "critical")];
        let alerts = evaluate(&rules, &[metric("memory_used_percent", 99.0)]);
        let a = &alerts[0];
        let s = serde_json::to_string(a).unwrap();
        let back: Alert = serde_json::from_str(&s).unwrap();
        assert_eq!(*a, back);
    }
}
