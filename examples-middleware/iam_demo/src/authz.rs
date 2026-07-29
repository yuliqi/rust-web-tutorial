//! 授权判定：纯逻辑，不碰数据库、不碰网络，因此可以被单元测试彻底覆盖。
//!
//! 把「能不能做」这件事从 IO 里剥离出来单独成模块，是安全代码的重要工程习惯——
//! 授权是系统的命门，它必须是**可穷举测试**的一小段纯函数，而不是散落在各个
//! handler、和 SQL 纠缠在一起的隐式判断。store.rs 负责「取出某账号的 grants」，
//! 判定则一律走这里。
//!
//! 两条核心规则：
//! 1. can()：默认拒绝（default deny）——没有任何一条 grant 命中，就是不允许。
//! 2. effective_grants()：子账号的有效权限 = 自身 ∩ 父账号，保证「子不越父」。

use crate::model::{Grant, GrantSpec};

/// 一条 grant 是否覆盖某个具体的 (resource_type, resource_id, action) 请求。
/// resource_type 必须精确匹配；resource_id / action 支持通配符 `*`。
fn grant_matches(g: &GrantSpec, resource_type: &str, resource_id: &str, action: &str) -> bool {
    g.resource_type == resource_type
        && (g.resource_id == "*" || g.resource_id == resource_id)
        && (g.action == "*" || g.action == action)
}

/// 授权判定的唯一入口：给定某账号持有的 grants，问「能否对该资源执行该动作」。
///
/// **默认拒绝**：只有当至少一条 grant 命中时才返回 true。这是授权系统的黄金默认值——
/// 「没写明允许 = 禁止」，而不是「没写明禁止 = 允许」。前者漏配一条只是少给了权限，
/// 后者漏配一条就是开了个后门。
pub fn can(grants: &[Grant], resource_type: &str, resource_id: &str, action: &str) -> bool {
    grants
        .iter()
        .any(|g| grant_matches(&g.spec(), resource_type, resource_id, action))
}

/// 一条「父账号视角」的 grant，是否足以覆盖子账号想要的一条 grant。
///
/// 关键点在通配方向：子账号若想要 `resource_id="*"`（全部），父账号必须也持有
/// `"*"` 才算覆盖；父账号只有具体的 `"A"` 是**盖不住**子账号的 `"*"` 的——
/// 否则子账号就凭一个通配符越权拿到了父账号没有的资源。action 同理。
fn parent_covers(parent: &[Grant], child: &GrantSpec) -> bool {
    parent.iter().any(|p| {
        let p = p.spec();
        p.resource_type == child.resource_type
            && (p.resource_id == "*" || p.resource_id == child.resource_id)
            && (p.action == "*" || p.action == child.action)
    })
}

/// 子账号的**有效权限** = 自身声明的权限 ∩ 父账号的权限。
///
/// 核心不变量：子账号权限永远不超过父账号。即便库里给子账号塞了一条父账号没有的
/// grant（配置错误、或历史上父账号被降权但子账号没跟着收），判定时也会被这里裁掉——
/// 「有效权限」以运行时的父子求交为准，不盲信子账号自己那张表。
///
/// 返回被父账号覆盖、因而真正生效的那些子 grant（保留原始 id 便于溯源）。
pub fn effective_grants(child: &[Grant], parent: &[Grant]) -> Vec<Grant> {
    child
        .iter()
        .filter(|c| parent_covers(parent, &c.spec()))
        .cloned()
        .collect()
}

/// 便捷判定：先把子账号权限对父账号求交，再在「有效权限」上跑 can()。
/// 这是子账号发起访问时应当走的完整判定链——它同时受自身授权和父账号上限双重约束。
pub fn can_effective(
    child: &[Grant],
    parent: &[Grant],
    resource_type: &str,
    resource_id: &str,
    action: &str,
) -> bool {
    let effective = effective_grants(child, parent);
    can(&effective, resource_type, resource_id, action)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(rt: &str, rid: &str, action: &str) -> Grant {
        Grant {
            id: 0,
            account_id: 0,
            resource_type: rt.into(),
            resource_id: rid.into(),
            action: action.into(),
        }
    }

    #[test]
    fn can_exact_match() {
        let grants = vec![g("cloud-account", "A", "read")];
        assert!(can(&grants, "cloud-account", "A", "read"));
    }

    #[test]
    fn can_default_deny_when_no_grant() {
        // 默认拒绝：空授权集合对任何请求都是 false。
        assert!(!can(&[], "cloud-account", "A", "read"));
    }

    #[test]
    fn can_denies_other_resource() {
        // 只被授权了 A，问 B 必须拒——这就是「子账号只能访问某个云账号」的落点。
        let grants = vec![g("cloud-account", "A", "read")];
        assert!(!can(&grants, "cloud-account", "B", "read"));
    }

    #[test]
    fn can_denies_wrong_action() {
        // 有 read 不等于有 write：action 不匹配同样拒绝。
        let grants = vec![g("cloud-account", "A", "read")];
        assert!(!can(&grants, "cloud-account", "A", "write"));
    }

    #[test]
    fn can_denies_wrong_resource_type() {
        let grants = vec![g("cloud-account", "A", "read")];
        assert!(!can(&grants, "database", "A", "read"));
    }

    #[test]
    fn can_wildcard_resource_id() {
        // resource_id="*"：该类型下所有资源都能 read。
        let grants = vec![g("cloud-account", "*", "read")];
        assert!(can(&grants, "cloud-account", "A", "read"));
        assert!(can(&grants, "cloud-account", "Z", "read"));
        // 但通配的是资源不是动作，write 仍拒。
        assert!(!can(&grants, "cloud-account", "A", "write"));
    }

    #[test]
    fn can_wildcard_action() {
        // action="*"：对资源 A 的任意动作都放行。
        let grants = vec![g("cloud-account", "A", "*")];
        assert!(can(&grants, "cloud-account", "A", "read"));
        assert!(can(&grants, "cloud-account", "A", "write"));
        assert!(can(&grants, "cloud-account", "A", "delete"));
        // 但 B 不在授权内。
        assert!(!can(&grants, "cloud-account", "B", "read"));
    }

    #[test]
    fn effective_grants_caps_child_at_parent() {
        // 父账号只有 A 的读写；子账号自己那张表里却多塞了一条 B:read（越权）。
        let parent = vec![g("cloud-account", "A", "*")];
        let child = vec![g("cloud-account", "A", "read"), g("cloud-account", "B", "read")];
        let eff = effective_grants(&child, &parent);
        // 越权的 B:read 被裁掉，只剩父账号覆盖得住的 A:read。
        assert_eq!(eff.len(), 1);
        assert_eq!(eff[0].resource_id, "A");
        assert_eq!(eff[0].action, "read");
    }

    #[test]
    fn effective_grants_child_wildcard_not_covered_by_specific_parent() {
        // 子账号想要 A 的全部动作（*），但父账号只有 A:read——
        // 具体的父盖不住通配的子，这条被裁掉（子不能凭 * 越权）。
        let parent = vec![g("cloud-account", "A", "read")];
        let child = vec![g("cloud-account", "A", "*")];
        assert!(effective_grants(&child, &parent).is_empty());
    }

    #[test]
    fn effective_grants_wildcard_parent_covers_specific_child() {
        // 反过来：父账号是通配的 *:*，子账号要具体的 A:read，能覆盖，保留。
        let parent = vec![g("cloud-account", "*", "*")];
        let child = vec![g("cloud-account", "A", "read")];
        assert_eq!(effective_grants(&child, &parent).len(), 1);
    }

    #[test]
    fn can_effective_cross_account_denied() {
        // 组合演示（对应 main.rs）：子账号自己被授了 B:read，但父账号只有 A——
        // 有效权限里没有 B，跨账号访问被拒。
        let parent = vec![g("cloud-account", "A", "*")];
        let child = vec![g("cloud-account", "B", "read")];
        assert!(!can_effective(&child, &parent, "cloud-account", "B", "read"));
        // 而它对 A 的读，只要父账号允许且自身也声明了才行；这里自身没声明 A，也拒。
        assert!(!can_effective(&child, &parent, "cloud-account", "A", "read"));
    }
}
