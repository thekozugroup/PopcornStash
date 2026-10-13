//! Policy routing for the engine user (section 5, step 3).
//!
//! Only the engine's uid and the DNS forwarder's marked sockets use routing
//! table [`ROUTE_TABLE`]. That table holds a single default route through the
//! tunnel; with no tunnel it is empty and the `prohibit` rules right after
//! the lookups make sure those packets never fall through to the main table.

use serde::Deserialize;
use stash_core::internal_api::{FWMARK, ROUTE_TABLE};

use crate::sys::{self, CommandError};

/// Rule priorities (lower runs first). Docker and the host use 0, 32766, 32767.
pub const PRIO_MARK_LOOKUP: u32 = 99;
pub const PRIO_UID_LOOKUP: u32 = 100;
pub const PRIO_UID_PROHIBIT: u32 = 101;
pub const PRIO_MARK_PROHIBIT: u32 = 102;
const OUR_PRIORITIES: [u32; 4] = [
    PRIO_MARK_LOOKUP,
    PRIO_UID_LOOKUP,
    PRIO_UID_PROHIBIT,
    PRIO_MARK_PROHIBIT,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    V4,
    V6,
}

impl Family {
    pub fn flag(self) -> &'static str {
        match self {
            Family::V4 => "-4",
            Family::V6 => "-6",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selector {
    UidRange(u32, u32),
    FwMark(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Lookup(u32),
    Prohibit,
}

/// One `ip rule`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule {
    pub priority: u32,
    pub selector: Selector,
    pub action: Action,
}

impl Rule {
    /// Arguments for `ip <family> rule <verb> ...`.
    pub fn args(&self, family: Family, verb: &str) -> Vec<String> {
        let mut args = vec![
            family.flag().to_string(),
            "rule".into(),
            verb.into(),
            "priority".into(),
            self.priority.to_string(),
        ];
        match self.selector {
            Selector::UidRange(start, end) => {
                args.push("uidrange".into());
                args.push(format!("{start}-{end}"));
            }
            Selector::FwMark(mark) => {
                args.push("fwmark".into());
                args.push(format!("{mark:#x}"));
            }
        }
        match self.action {
            Action::Lookup(table) => {
                args.push("lookup".into());
                args.push(table.to_string());
            }
            Action::Prohibit => args.push("prohibit".into()),
        }
        args
    }
}

/// The four rules of section 5 step 3 for engine uid `engine_uid`.
pub fn desired_rules(engine_uid: u32) -> Vec<Rule> {
    vec![
        Rule {
            priority: PRIO_MARK_LOOKUP,
            selector: Selector::FwMark(FWMARK),
            action: Action::Lookup(ROUTE_TABLE),
        },
        Rule {
            priority: PRIO_UID_LOOKUP,
            selector: Selector::UidRange(engine_uid, engine_uid),
            action: Action::Lookup(ROUTE_TABLE),
        },
        Rule {
            priority: PRIO_UID_PROHIBIT,
            selector: Selector::UidRange(engine_uid, engine_uid),
            action: Action::Prohibit,
        },
        Rule {
            priority: PRIO_MARK_PROHIBIT,
            selector: Selector::FwMark(FWMARK),
            action: Action::Prohibit,
        },
    ]
}

#[derive(Debug, Deserialize)]
struct JsonRule {
    priority: Option<u32>,
    uid_start: Option<u32>,
    uid_end: Option<u32>,
    fwmark: Option<String>,
    fwmask: Option<String>,
    table: Option<String>,
    action: Option<String>,
    // Any other selector (from, to, iif, ...) makes the rule "not ours".
    src: Option<String>,
    dst: Option<String>,
    iif: Option<String>,
    oif: Option<String>,
}

/// A rule as reported by `ip -j rule`. `rule` is None when it has selectors or
/// actions this module never creates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedRule {
    pub priority: u32,
    pub rule: Option<Rule>,
}

fn parse_u32(value: &str) -> Option<u32> {
    match value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => value.parse().ok(),
    }
}

/// Parse `ip -j rule` output.
pub fn parse_rules(json: &str) -> Result<Vec<ListedRule>, serde_json::Error> {
    if json.trim().is_empty() {
        return Ok(Vec::new());
    }
    let rules: Vec<JsonRule> = serde_json::from_str(json)?;
    Ok(rules
        .into_iter()
        .filter_map(|r| {
            let priority = r.priority?;
            let plain_src = r.src.as_deref().is_none_or(|s| s == "all");
            let plain = plain_src && r.dst.is_none() && r.iif.is_none() && r.oif.is_none();
            let mask_ok = r
                .fwmask
                .as_deref()
                .is_none_or(|m| parse_u32(m) == Some(u32::MAX));
            let selector = match (r.uid_start, r.uid_end, r.fwmark.as_deref()) {
                (Some(start), Some(end), None) => Some(Selector::UidRange(start, end)),
                (None, None, Some(mark)) if mask_ok => parse_u32(mark).map(Selector::FwMark),
                _ => None,
            };
            let action = match (r.action.as_deref(), r.table.as_deref()) {
                (Some("prohibit"), _) => Some(Action::Prohibit),
                (None | Some("to_tbl"), Some(table)) => parse_u32(table).map(Action::Lookup),
                _ => None,
            };
            let rule = match (plain, selector, action) {
                (true, Some(selector), Some(action)) => Some(Rule {
                    priority,
                    selector,
                    action,
                }),
                _ => None,
            };
            Some(ListedRule { priority, rule })
        })
        .collect())
}

/// What to change to get from `current` to `desired`: rules at our priorities
/// that are not desired are deleted (stale uid, earlier run); missing ones added.
/// Rules we cannot express (unknown selectors at our priorities) are deleted by priority.
pub fn plan_rule_changes(current: &[ListedRule], desired: &[Rule]) -> (Vec<RuleDelete>, Vec<Rule>) {
    let mut deletes = Vec::new();
    let mut seen = Vec::new();
    for listed in current
        .iter()
        .filter(|r| OUR_PRIORITIES.contains(&r.priority))
    {
        match listed.rule {
            Some(rule) if desired.contains(&rule) && !seen.contains(&rule) => seen.push(rule),
            Some(rule) => deletes.push(RuleDelete::Exact(rule)),
            None => deletes.push(RuleDelete::Priority(listed.priority)),
        }
    }
    let adds = desired
        .iter()
        .filter(|r| !seen.contains(r))
        .copied()
        .collect();
    (deletes, adds)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleDelete {
    Exact(Rule),
    Priority(u32),
}

impl RuleDelete {
    pub fn args(&self, family: Family) -> Vec<String> {
        match self {
            RuleDelete::Exact(rule) => rule.args(family, "del"),
            RuleDelete::Priority(priority) => vec![
                family.flag().into(),
                "rule".into(),
                "del".into(),
                "priority".into(),
                priority.to_string(),
            ],
        }
    }
}

async fn list_rules(family: Family) -> Result<Vec<ListedRule>, CommandError> {
    let out = sys::run_str("ip", &["-j", family.flag(), "rule", "show"]).await?;
    parse_rules(&out).map_err(|e| CommandError {
        program: "ip".into(),
        args: "-j rule show".into(),
        message: format!("unreadable output: {e}"),
        code: None,
    })
}

/// Make the rules for `family` exactly the desired ones (idempotent).
pub async fn apply_rules(family: Family, engine_uid: u32) -> Result<(), CommandError> {
    let desired = desired_rules(engine_uid);
    let current = list_rules(family).await?;
    let (deletes, adds) = plan_rule_changes(&current, &desired);
    for delete in deletes {
        sys::run("ip", &delete.args(family), None).await?;
    }
    for rule in adds {
        sys::run("ip", &rule.args(family, "add"), None).await?;
    }
    Ok(())
}

/// Remove every rule at our priorities (shutdown).
pub async fn remove_rules(family: Family) -> Result<(), CommandError> {
    let current = list_rules(family).await?;
    let (deletes, _) = plan_rule_changes(&current, &[]);
    for delete in deletes {
        if let Err(e) = sys::run("ip", &delete.args(family), None).await {
            tracing::debug!(error = %e, "could not delete a routing rule");
        }
    }
    Ok(())
}

/// True when the rules for `family` are exactly in place.
pub async fn rules_in_place(family: Family, engine_uid: u32) -> Result<bool, CommandError> {
    let current = list_rules(family).await?;
    let (deletes, adds) = plan_rule_changes(&current, &desired_rules(engine_uid));
    Ok(deletes.is_empty() && adds.is_empty())
}

/// `ip <family> route replace default dev <ifname> table 51820`.
pub fn default_route_args(family: Family, ifname: &str) -> Vec<String> {
    vec![
        family.flag().into(),
        "route".into(),
        "replace".into(),
        "default".into(),
        "dev".into(),
        ifname.into(),
        "table".into(),
        ROUTE_TABLE.to_string(),
    ]
}

pub async fn set_default_route(family: Family, ifname: &str) -> Result<(), CommandError> {
    sys::run("ip", &default_route_args(family, ifname), None)
        .await
        .map(drop)
}

/// Flush table 51820 (the tunnel is gone). Missing table is fine.
pub async fn flush_table(family: Family) {
    let table = ROUTE_TABLE.to_string();
    if let Err(e) = sys::run_str("ip", &[family.flag(), "route", "flush", "table", &table]).await {
        tracing::debug!(error = %e, "flushing the tunnel routing table");
    }
}

#[derive(Debug, Deserialize)]
struct JsonRoute {
    dst: Option<String>,
    dev: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedRoute {
    pub dst: String,
    pub dev: Option<String>,
}

/// Parse `ip -j route show table N`.
pub fn parse_routes(json: &str) -> Result<Vec<ListedRoute>, serde_json::Error> {
    if json.trim().is_empty() {
        return Ok(Vec::new());
    }
    let routes: Vec<JsonRoute> = serde_json::from_str(json)?;
    Ok(routes
        .into_iter()
        .map(|r| ListedRoute {
            dst: r.dst.unwrap_or_default(),
            dev: r.dev,
        })
        .collect())
}

/// True when table 51820 has `default dev <ifname>`. A missing table counts as empty.
pub async fn has_default_route(family: Family, ifname: &str) -> bool {
    let table = ROUTE_TABLE.to_string();
    match sys::run_str(
        "ip",
        &["-j", family.flag(), "route", "show", "table", &table],
    )
    .await
    {
        Ok(out) => parse_routes(&out)
            .map(|routes| {
                routes
                    .iter()
                    .any(|r| r.dst == "default" && r.dev.as_deref() == Some(ifname))
            })
            .unwrap_or(false),
        Err(_) => false,
    }
}

/// True when the kernel has IPv6 (some hosts and test VMs do not).
pub async fn ipv6_available() -> bool {
    std::path::Path::new("/proc/net/if_inet6").exists()
        && sys::run_str("ip", &["-6", "rule", "show"]).await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_args() {
        let rules = desired_rules(6881);
        let rendered: Vec<String> = rules
            .iter()
            .map(|r| r.args(Family::V4, "add").join(" "))
            .collect();
        assert_eq!(
            rendered,
            vec![
                "-4 rule add priority 99 fwmark 0x5150 lookup 51820",
                "-4 rule add priority 100 uidrange 6881-6881 lookup 51820",
                "-4 rule add priority 101 uidrange 6881-6881 prohibit",
                "-4 rule add priority 102 fwmark 0x5150 prohibit",
            ]
        );
        assert_eq!(
            rules[2].args(Family::V6, "del").join(" "),
            "-6 rule del priority 101 uidrange 6881-6881 prohibit"
        );
        assert_eq!(
            default_route_args(Family::V4, "psw0").join(" "),
            "-4 route replace default dev psw0 table 51820"
        );
    }

    // Captured from `ip -j rule` (iproute2 6.1) after applying the rules.
    const APPLIED: &str = r#"[{"priority":0,"src":"all","table":"local"},{"priority":99,"src":"all","fwmark":"0x5150","table":"51820"},{"priority":100,"src":"all","uid_start":6881,"uid_end":6881,"table":"51820"},{"priority":101,"src":"all","uid_start":6881,"uid_end":6881,"action":"prohibit"},{"priority":102,"src":"all","fwmark":"0x5150","action":"prohibit"},{"priority":32766,"src":"all","table":"main"},{"priority":32767,"src":"all","table":"default"}]"#;
    const FRESH: &str = r#"[{"priority":0,"src":"all","table":"local"},{"priority":32766,"src":"all","table":"main"},{"priority":32767,"src":"all","table":"default"}]"#;

    #[test]
    fn parses_applied_rules() {
        let listed = parse_rules(APPLIED).unwrap();
        assert_eq!(listed.len(), 7);
        let ours: Vec<Rule> = listed
            .iter()
            .filter(|l| OUR_PRIORITIES.contains(&l.priority))
            .filter_map(|l| l.rule)
            .collect();
        assert_eq!(ours.len(), 4);
        for rule in desired_rules(6881) {
            assert!(ours.contains(&rule), "{rule:?}");
        }
        assert!(parse_rules("").unwrap().is_empty());
    }

    #[test]
    fn plan_is_idempotent() {
        let (deletes, adds) =
            plan_rule_changes(&parse_rules(APPLIED).unwrap(), &desired_rules(6881));
        assert!(deletes.is_empty());
        assert!(adds.is_empty());
        let (deletes, adds) = plan_rule_changes(&parse_rules(FRESH).unwrap(), &desired_rules(6881));
        assert!(deletes.is_empty());
        assert_eq!(adds, desired_rules(6881));
    }

    #[test]
    fn plan_replaces_stale_uid_and_foreign_rules() {
        let stale = APPLIED.replace("6881", "7000").replace(
            r#"{"priority":32766"#,
            r#"{"priority":100,"src":"10.0.0.0","srclen":8,"table":"main"},{"priority":32766"#,
        );
        let (deletes, adds) =
            plan_rule_changes(&parse_rules(&stale).unwrap(), &desired_rules(6881));
        assert_eq!(adds.len(), 2, "uid rules re-added: {adds:?}");
        assert!(deletes.contains(&RuleDelete::Priority(100)));
        assert!(deletes.iter().any(
            |d| matches!(d, RuleDelete::Exact(r) if r.selector == Selector::UidRange(7000, 7000))
        ));
        // Removal plan deletes everything at our priorities, nothing else.
        let (deletes, _) = plan_rule_changes(&parse_rules(APPLIED).unwrap(), &[]);
        assert_eq!(deletes.len(), 4);
        assert!(deletes.iter().all(|d| match d {
            RuleDelete::Exact(r) => OUR_PRIORITIES.contains(&r.priority),
            RuleDelete::Priority(p) => OUR_PRIORITIES.contains(p),
        }));
    }

    #[test]
    fn parses_routes() {
        let json = r#"[{"dst":"default","dev":"psw0","scope":"link","flags":[]}]"#;
        let routes = parse_routes(json).unwrap();
        assert_eq!(
            routes,
            vec![ListedRoute {
                dst: "default".into(),
                dev: Some("psw0".into())
            }]
        );
        assert!(parse_routes("").unwrap().is_empty());
        assert!(parse_routes("[]").unwrap().is_empty());
    }
}
