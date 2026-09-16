//! Human names for native objects. Tags stay stable; names are what people read.
use super::modal::Choice;
use crate::{model::Store, native};
use serde_json::Value;

pub fn label(store: &Store, tag: &str) -> String {
    if tag == native::GLOBAL_TAG {
        return "Global".into();
    }
    if let Some(r) = store.rule_resources.iter().find(|r| r.tag() == tag) {
        return crate::model::clean(&r.name);
    }
    if let Some(name) = store.display_names.get(tag) {
        return crate::model::clean(name);
    }
    if let Some(n) = store.nodes.iter().find(|n| n.tag() == tag) {
        return crate::model::clean(&n.name);
    }
    if let Some(g) = store.proxy_groups.iter().find(|g| g.tag() == tag) {
        return crate::model::clean(&g.name);
    }
    match tag {
        "direct" => "Direct".into(),
        "proxy" => "Proxy".into(),
        _ => crate::model::clean(tag),
    }
}

pub fn is_group(v: &Value) -> bool {
    ["selector", "urltest"].contains(&v["type"].as_str().unwrap_or(""))
}

/// Protocol name as people say it.
pub fn protocol(kind: &str) -> &str {
    match kind {
        "selector" => "Manual",
        "urltest" => "Auto",
        "shadowsocks" => "SS",
        "shadowtls" => "ShadowTLS",
        "hysteria2" => "Hysteria2",
        "vmess" => "VMess",
        "vless" => "VLESS",
        "trojan" => "Trojan",
        "tuic" => "TUIC",
        "wireguard" => "WireGuard",
        "direct" => "Direct",
        "block" => "Block",
        "socks" => "SOCKS",
        "http" => "HTTP",
        "anytls" => "AnyTLS",
        "ssh" => "SSH",
        "tor" => "Tor",
        other => other,
    }
}

/// Outbounds a rule or group can target: groups first, then direct, then
/// nodes and endpoints. `reject` is offered where the caller allows it.
pub fn targets(store: &Store, doc: &Value, reject: bool) -> Vec<Choice> {
    let mut groups = vec![];
    let mut others = vec![];
    let mut nodes = vec![];
    for path in ["/outbounds", "/endpoints"] {
        for v in native::array(doc, path) {
            let tag = native::tag(v);
            if tag.is_empty() {
                continue;
            }
            let kind = v["type"].as_str().unwrap_or("");
            let choice = Choice::new(tag, label(store, tag), protocol(kind));
            if is_group(v) {
                groups.push(choice);
            } else if ["direct", "block"].contains(&kind) {
                others.push(choice);
            } else {
                nodes.push(choice);
            }
        }
    }
    groups.extend(others);
    if reject {
        groups.push(Choice::new("reject", "Reject", "block the connection"));
    }
    groups.extend(nodes);
    groups
}

/// Short predicate description used in rule lists and reviews.
pub fn matcher(store: &Store, rule: &Value) -> String {
    if rule["type"] == "logical" {
        let parts: Vec<String> = native::array(rule, "/rules")
            .iter()
            .map(|r| matcher(store, r))
            .collect();
        let joined = parts.join(if rule["mode"] == "or" {
            " or "
        } else {
            " and "
        });
        return if rule["invert"] == true {
            format!("not ({joined})")
        } else {
            joined
        };
    }
    let Some(map) = rule.as_object() else {
        return "?".into();
    };
    let mut parts = vec![];
    for (k, v) in map {
        if [
            "action", "outbound", "server", "invert", "method", "no_drop", "sniffer", "timeout",
            "strategy", "type",
        ]
        .contains(&k.as_str())
        {
            continue;
        }
        let name = match k.as_str() {
            "domain" => "domain",
            "domain_suffix" => "suffix",
            "domain_keyword" => "keyword",
            "domain_regex" => "regex",
            "ip_cidr" => "ip",
            "ip_is_private" => "private ip",
            "source_ip_cidr" => "source ip",
            "process_name" => "app",
            "process_path" => "app path",
            "process_path_regex" => "app path regex",
            "package_name" => "package",
            "rule_set" => "set",
            "protocol" => "protocol",
            "network" => "network",
            "port" => "port",
            "clash_mode" => "mode",
            other => other,
        };
        let values: Vec<String> = match v {
            Value::Array(a) => a
                .iter()
                .map(|x| {
                    let s = x
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| x.to_string());
                    if k == "rule_set" {
                        label(store, &s)
                    } else {
                        s
                    }
                })
                .collect(),
            Value::Bool(true) => vec![],
            Value::String(s) if k == "rule_set" => vec![label(store, s)],
            Value::String(s) => vec![s.clone()],
            other => vec![other.to_string()],
        };
        parts.push(match values.len() {
            0 => name.to_string(),
            1 => format!("{name} {}", values[0]),
            n => format!("{name} {} +{}", values[0], n - 1),
        });
    }
    let text = if parts.is_empty() {
        "everything".into()
    } else {
        parts.join(" · ")
    };
    if rule["invert"] == true {
        format!("not {text}")
    } else {
        crate::model::clean(&text)
    }
}

/// (verb, target tag) for a route rule.
pub fn action(rule: &Value) -> (String, String) {
    let action = rule["action"].as_str().unwrap_or("route");
    match action {
        "route" | "bypass" => (
            action.into(),
            rule["outbound"].as_str().unwrap_or("").to_string(),
        ),
        "reject" => ("reject".into(), "reject".into()),
        other => (other.into(), String::new()),
    }
}

pub fn dns_action(rule: &Value) -> (String, String) {
    let action = rule["action"].as_str().unwrap_or("route");
    (
        action.into(),
        rule["server"].as_str().unwrap_or("").to_string(),
    )
}
