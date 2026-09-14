//! Native documents are authoritative. Import metadata never rebuilds a document.
use crate::{config, model::Store, ruleset};
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edit {
    pub revision: String,
    pub pointer: String,
    pub value: Value,
}

pub fn revision(store: &Store) -> String {
    crate::model::id(&serde_json::to_string(store).unwrap())
}

pub fn migration(store: &Store) -> Result<Value> {
    if let Some(doc) = &store.native {
        return Ok(doc.clone());
    }
    let mut old = store.clone();
    old.settings.route_mode = "rule".into();
    if old.nodes.is_empty() {
        old.settings.route_mode = "direct".into();
    }
    let mut doc = config::generate(&old)?;
    doc["dns"]
        .as_object_mut()
        .unwrap()
        .entry("rules")
        .or_insert(json!([]));
    doc["route"]
        .as_object_mut()
        .unwrap()
        .entry("rule_set")
        .or_insert(json!([]));
    if store.nodes.is_empty() {
        doc["outbounds"]
            .as_array_mut()
            .unwrap()
            .insert(0, json!({"type":"selector","tag":"proxy","outbounds":[]}));
        doc["route"]["final"] = json!("proxy");
    }
    // Imported rule policies are materialized once. Later resource updates only
    // replace the rule-set contents, never overwrite independent DNS rules.
    Ok(doc)
}

pub fn adopt(store: &mut Store, doc: Value) -> Result<()> {
    shape(&doc)?;
    store.native = Some(doc);
    store.schema = 2;
    store.settings.language = "en".into();
    if store.settings.mode == "tun" {
        store.settings.mode = "port".into();
    }
    Ok(())
}

pub fn read(store: &Store, pointer: String) -> Result<Edit> {
    let doc = store
        .native
        .as_ref()
        .context("Review the native configuration upgrade first")?;
    let value = if pointer.is_empty() {
        doc.clone()
    } else {
        doc.pointer(&pointer).cloned().unwrap_or(Value::Null)
    };
    Ok(Edit {
        revision: revision(store),
        pointer,
        value,
    })
}

pub fn write(store: &mut Store, edit: Edit) -> Result<()> {
    ensure!(
        edit.revision == revision(store),
        "Draft changed. Reopen the editor before saving."
    );
    let mut doc = store
        .native
        .clone()
        .context("Native configuration is not initialized")?;
    set(&mut doc, &edit.pointer, edit.value)?;
    shape(&doc)?;
    let present: HashSet<_> = array(&doc, "/route/rule_set")
        .iter()
        .map(|v| tag(v).to_string())
        .collect();
    store.rule_resources.retain(|r| present.contains(&r.tag()));
    store
        .rule_bindings
        .retain(|b| store.rule_resources.iter().any(|r| r.id == b.resource));
    store.native = Some(doc);
    Ok(())
}

/// Replace exactly one subtree, preserving every sibling and array order.
pub fn set(doc: &mut Value, pointer: &str, value: Value) -> Result<()> {
    if pointer.is_empty() {
        *doc = value;
        return Ok(());
    }
    let (parent, key) = pointer.rsplit_once('/').context("Invalid JSON pointer")?;
    if doc.pointer(parent).is_none() {
        let empty = if key == "-" { json!([]) } else { json!({}) };
        set(doc, parent, empty)?;
    }
    let key = key.replace("~1", "/").replace("~0", "~");
    let parent = doc
        .pointer_mut(parent)
        .context("Parent field is missing; edit its parent first")?;
    match parent {
        Value::Object(map) => {
            map.insert(key, value);
        }
        Value::Array(list) => {
            if key == "-" {
                list.push(value);
            } else {
                let i: usize = key.parse().context("Invalid list index")?;
                *list.get_mut(i).context("Object no longer exists")? = value;
            }
        }
        _ => bail!("Cannot edit a field inside a scalar"),
    }
    Ok(())
}

pub fn shape(doc: &Value) -> Result<()> {
    ensure!(
        doc.is_object(),
        "Native configuration must be a JSON object"
    );
    ensure!(
        serde_json::to_vec(doc)?.len() <= 8 * 1024 * 1024,
        "Configuration exceeds 8 MiB"
    );
    for key in ["inbounds", "outbounds", "endpoints", "services"] {
        if let Some(v) = doc.get(key) {
            ensure!(v.is_array(), "{key} must be an array");
        }
    }
    for key in ["dns", "route", "log"] {
        if let Some(v) = doc.get(key) {
            ensure!(v.is_object(), "{key} must be an object");
        }
    }
    Ok(())
}

pub fn uses_tun(store: &Store) -> bool {
    store
        .native
        .as_ref()
        .map_or(store.settings.mode == "tun", |d| {
            array(d, "/inbounds").iter().any(|i| i["type"] == "tun")
        })
}
pub fn array<'a>(doc: &'a Value, pointer: &str) -> &'a [Value] {
    doc.pointer(pointer)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}
pub fn tag(v: &Value) -> &str {
    v["tag"].as_str().unwrap_or("")
}

pub fn effective(store: &Store) -> Result<Value> {
    let mut doc = store.native.clone().context("Missing native document")?;
    shape(&doc)?;
    let api = array(&doc, "/services")
        .iter()
        .find(|v| tag(v) == "management")
        .context("Keep the management API service (tag: management) for sing control")?;
    ensure!(
        api["type"] == "api"
            && api["listen"] == "127.0.0.1"
            && api["listen_port"] == store.settings.api_port
            && api["secret"] == store.secret
            && api.pointer("/tls/enabled") != Some(&json!(true)),
        "Management API must keep its loopback address, port, secret and non-TLS transport"
    );
    if store.settings.mode == "system" {
        ensure!(
            cfg!(target_os = "macos"),
            "System proxy integration is macOS-only"
        );
        ensure!(array(&doc, "/inbounds").iter().any(|i|
            i["type"] == "mixed" && i["listen"] == "127.0.0.1"
            && i["listen_port"] == store.settings.port
            && i.get("users").is_none_or(|u| u.as_array().is_some_and(Vec::is_empty))),
            "System integration needs an unauthenticated mixed inbound at 127.0.0.1 on its configured port");
    }
    match store.settings.route_mode.as_str() {
        "rule" => {}
        "global" | "direct" => {
            // Overrides are temporary; DNS, outbounds and their dependencies remain
            // intact. The review explicitly discloses unchanged DNS and exceptions.
            let mut rules: Vec<_> = array(&doc, "/route/rules")
                .iter()
                .filter(|r| r["action"] == "sniff" || r["action"] == "hijack-dns")
                .cloned()
                .collect();
            if store.settings.bypass_lan {
                rules.push(json!({"ip_is_private":true,"action":"route","outbound":"direct"}));
            }
            doc["route"]["rules"] = json!(rules);
            doc["route"]["final"] = if store.settings.route_mode == "direct" {
                json!("direct")
            } else {
                json!(store.settings.global_target)
            };
        }
        _ => bail!("Unknown routing override"),
    }
    references(&doc)?;
    Ok(doc)
}

pub fn references(doc: &Value) -> Result<()> {
    let mut out = HashMap::new();
    for p in ["/outbounds", "/endpoints"] {
        for v in array(doc, p) {
            if !tag(v).is_empty() {
                ensure!(
                    out.insert(tag(v), v).is_none(),
                    "Duplicate outbound/endpoint tag: {}",
                    tag(v)
                );
            }
        }
    }
    let mut dns = HashMap::new();
    for v in array(doc, "/dns/servers") {
        if !tag(v).is_empty() {
            ensure!(
                dns.insert(tag(v), v).is_none(),
                "Duplicate DNS tag: {}",
                tag(v)
            );
        }
    }
    for (key, values) in [("outbound", &out), ("server", &dns)] {
        let p = if key == "outbound" {
            "/route/final"
        } else {
            "/dns/final"
        };
        if let Some(t) = doc
            .pointer(p)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            ensure!(values.contains_key(t), "{p} references missing {key}: {t}");
        }
    }
    // One graph covers outbound chains and DNS bootstrap dependencies.
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    for (prefix, objects) in [("out", &out), ("dns", &dns)] {
        for (t, v) in objects {
            let mut edges = vec![];
            if let Some(d) = v["detour"].as_str().filter(|s| !s.is_empty()) {
                ensure!(out.contains_key(d), "{t}: missing detour {d}");
                if prefix == "dns"
                    && out[d]["type"] == "direct"
                    && out[d]
                        .as_object()
                        .is_some_and(|o| o.keys().all(|k| k == "type" || k == "tag"))
                {
                    bail!("DNS {t}: clear detour to dial directly; an empty direct outbound cannot be used as a DNS detour");
                }
                edges.push(format!("out:{d}"));
            }
            let resolver = if v["detour"].as_str().is_some_and(|s| !s.is_empty()) {
                None
            } else {
                v.get("domain_resolver").or_else(|| {
                    (prefix == "out"
                        && v["server"]
                            .as_str()
                            .is_some_and(|s| s.parse::<std::net::IpAddr>().is_err()))
                    .then(|| doc.pointer("/route/default_domain_resolver"))
                    .flatten()
                })
            };
            if let Some(d) = resolver
                .and_then(|v| v.as_str().or_else(|| v["server"].as_str()))
                .filter(|s| !s.is_empty())
            {
                ensure!(dns.contains_key(d), "{t}: missing domain resolver {d}");
                edges.push(format!("dns:{d}"));
            }
            if prefix == "out"
                && ["selector", "urltest"].contains(&v["type"].as_str().unwrap_or(""))
            {
                let members = v["outbounds"]
                    .as_array()
                    .context("Group members must be an array")?;
                ensure!(!members.is_empty(), "Group {t} has no members");
                for member in members {
                    let m = member
                        .as_str()
                        .context("Group member must be an outbound tag")?;
                    ensure!(out.contains_key(m), "Group {t}: missing member {m}");
                    edges.push(format!("out:{m}"));
                }
                if let Some(d) = v["default"].as_str().filter(|s| !s.is_empty()) {
                    ensure!(
                        members.iter().any(|m| m == d),
                        "Group {t}: default is not a member"
                    );
                }
            }
            graph.insert(format!("{prefix}:{t}"), edges);
        }
    }
    fn visit(
        node: &str,
        graph: &HashMap<String, Vec<String>>,
        active: &mut HashSet<String>,
        done: &mut HashSet<String>,
    ) -> Result<()> {
        if done.contains(node) {
            return Ok(());
        }
        ensure!(
            active.insert(node.into()),
            "Circular outbound / DNS dependency at {node}"
        );
        for next in graph.get(node).into_iter().flatten() {
            visit(next, graph, active, done)?;
        }
        active.remove(node);
        done.insert(node.into());
        Ok(())
    }
    let mut done = HashSet::new();
    for node in graph.keys() {
        visit(node, &graph, &mut HashSet::new(), &mut done)?;
    }
    Ok(())
}

/// Update subscription-owned objects only. A locally edited node is a conflict,
/// not an excuse to overwrite it. Unrelated objects and all rule order survive.
pub fn reconcile(old: &Store, new: &mut Store) -> Result<()> {
    let Some(doc) = new.native.as_mut() else {
        return Ok(());
    };
    let nodes_changed = old
        .nodes
        .iter()
        .map(|n| (&n.id, &n.outbound))
        .ne(new.nodes.iter().map(|n| (&n.id, &n.outbound)));
    if nodes_changed {
        let list = doc["outbounds"]
            .as_array_mut()
            .context("outbounds must be an array")?;
        for before in &old.nodes {
            let after = new.nodes.iter().find(|n| n.id == before.id);
            if after.is_some_and(|n| n.outbound == before.outbound) {
                continue;
            }
            if let Some(i) = list.iter().position(|v| tag(v) == before.tag()) {
                let mut expected = before.outbound.clone();
                expected["tag"] = json!(before.tag());
                ensure!(list[i] == expected, "Subscription node {} has local edits. Detach it by changing its tag before refreshing.", before.name);
                if let Some(after) = after {
                    let mut value = after.outbound.clone();
                    value["tag"] = json!(after.tag());
                    list[i] = value;
                } else {
                    list.remove(i);
                }
            } else if let Some(after) = after {
                let mut value = after.outbound.clone();
                value["tag"] = json!(after.tag());
                list.push(value);
            }
        }
        let mut added = vec![];
        for n in &new.nodes {
            if old.nodes.iter().any(|o| o.id == n.id) {
                continue;
            }
            ensure!(
                !list.iter().any(|v| tag(v) == n.tag()),
                "Imported node tag conflicts with native outbound"
            );
            let mut v = n.outbound.clone();
            v["tag"] = json!(n.tag());
            list.push(v);
            added.push(n.tag());
        }
        // Only fill a fresh empty default selector. Existing group membership is
        // deliberately explicit and is never silently expanded or repaired.
        if let Some(g) = list
            .iter_mut()
            .find(|v| tag(v) == "proxy" && v["type"] == "selector")
        {
            if g["outbounds"].as_array().is_some_and(Vec::is_empty) && !added.is_empty() {
                g["outbounds"] = json!(added);
                g["default"] = g["outbounds"][0].clone();
            }
        }
    }
    for resource in &new.rule_resources {
        let before = old.rule_resources.iter().find(|r| r.id == resource.id);
        if before.is_some_and(|r| r.rules == resource.rules) {
            continue;
        }
        let sets = doc["route"]
            .as_object_mut()
            .context("route must be an object")?
            .entry("rule_set")
            .or_insert(json!([]))
            .as_array_mut()
            .context("rule_set must be an array")?;
        let value = json!({"type":"inline","tag":resource.tag(),"rules":ruleset::native_rules(&resource.rules)});
        if let Some(i) = sets.iter().position(|s| tag(s) == resource.tag()) {
            if let Some(before) = before {
                ensure!(
                    sets[i]["rules"] == json!(ruleset::native_rules(&before.rules)),
                    "Rule-set has local edits; detach before refresh"
                );
            }
            sets[i]["rules"] = value["rules"].clone();
        } else {
            sets.push(value);
        }
        if before.is_none() {
            if let Some(b) = new
                .rule_bindings
                .iter()
                .find(|b| b.resource == resource.id && b.enabled)
            {
                let rule = if b.target == "reject" {
                    json!({"rule_set":[resource.tag()],"action":"reject"})
                } else {
                    json!({"rule_set":[resource.tag()],"action":"route","outbound":b.target})
                };
                doc["route"]
                    .as_object_mut()
                    .unwrap()
                    .entry("rules")
                    .or_insert(json!([]))
                    .as_array_mut()
                    .context("rules must be an array")?
                    .push(rule);
            }
        }
    }
    Ok(())
}

pub fn review(saved: &Store, running: Option<&Store>) -> Result<String> {
    let proposed = config::generate(saved)?;
    let mut result = format!(
        "{}\nCapture: {}{}\nRouting: {}\n\n",
        if running.is_some() {
            "Apply restarts the core and interrupts existing connections."
        } else {
            "Start the core with this configuration."
        },
        if uses_tun(saved) {
            "TUN"
        } else {
            "Local listeners"
        },
        if saved.settings.mode == "system" {
            " + macOS system proxy"
        } else {
            ""
        },
        saved.settings.route_mode
    );
    if saved.settings.route_mode != "rule" {
        result.push_str("Override replaces routing decisions; sniff/DNS actions and optional private-IP exception remain. DNS servers/rules and their outbound paths are UNCHANGED. Direct is a traffic-routing override, not a no-proxy guarantee for internal DNS.\n\n");
    }
    if uses_tun(saved) {
        result.push_str(
            "TUN changes routes and may configure interface DNS; SSH can be interrupted.\n\n",
        );
    }
    if array(&proposed, "/inbounds").iter().any(|i| {
        i["listen"]
            .as_str()
            .is_some_and(|s| !["127.0.0.1", "::1"].contains(&s))
    }) {
        result.push_str("Review listener exposure: one or more inbounds bind beyond loopback.\n\n");
    }
    let before = running
        .map(config::generate)
        .transpose()?
        .unwrap_or(json!({}));
    let mut paths = vec![];
    diff(&before, &proposed, "", &mut paths);
    result.push_str("Changed fields (values hidden):\n");
    for p in paths.iter().take(150) {
        result.push_str(&format!("  {p}\n"));
    }
    if paths.is_empty() {
        result.push_str("  No native configuration changes.\n");
    }
    if paths.len() > 150 {
        result.push_str("  … Open native preview for the complete document.\n");
    }
    result.push_str("\nCore validation runs before stopping the current instance. Startup failure attempts to restore the previous running configuration.");
    Ok(result)
}
fn diff(a: &Value, b: &Value, path: &str, out: &mut Vec<String>) {
    if a == b {
        return;
    }
    if let (Some(a), Some(b)) = (a.as_object(), b.as_object()) {
        let keys: std::collections::BTreeSet<_> = a.keys().chain(b.keys()).collect();
        for k in keys {
            diff(
                a.get(k).unwrap_or(&Value::Null),
                b.get(k).unwrap_or(&Value::Null),
                &format!("{path}/{k}"),
                out,
            );
        }
    } else {
        out.push(if path.is_empty() {
            "/".into()
        } else {
            path.into()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn store() -> Store {
        let mut s = Store::new().unwrap();
        s.nodes = crate::subscription::parse("trojan://fictional@host.invalid:443#Demo", "test")
            .unwrap()
            .nodes;
        let d = migration(&s).unwrap();
        adopt(&mut s, d).unwrap();
        s
    }
    #[test]
    fn roundtrip_and_scoped_edits_preserve_unknown_fields() {
        let mut s = store();
        s.native.as_mut().unwrap()["future"] = json!({"nested":[1,{"x":true}]});
        let before = s.native.clone().unwrap();
        let mut e = read(&s, "/dns".into()).unwrap();
        e.value["timeout"] = json!("4s");
        write(&mut s, e.clone()).unwrap();
        assert_eq!(s.native.as_ref().unwrap()["future"], before["future"]);
        assert_eq!(s.native.as_ref().unwrap()["outbounds"], before["outbounds"]);
        assert!(write(&mut s, e).is_err());
        let d = tempfile::tempdir().unwrap();
        s.save(d.path()).unwrap();
        assert_eq!(Store::load(d.path()).unwrap().native, s.native);
    }
    #[test]
    fn override_does_not_rewrite_dns_or_discard_dependencies() {
        let mut s = store();
        let before = effective(&s).unwrap();
        for mode in ["global", "direct"] {
            s.settings.route_mode = mode.into();
            let c = effective(&s).unwrap();
            assert_eq!(c["dns"], before["dns"]);
            assert_eq!(c["outbounds"], before["outbounds"]);
        }
    }
    #[test]
    fn dependency_cycle_detected_across_dns_and_outbound() {
        let mut s = store();
        let d = s.native.as_mut().unwrap();
        d["outbounds"][2]["domain_resolver"] = json!("dns-proxy");
        assert!(references(d).unwrap_err().to_string().contains("Circular"));
    }
    #[test]
    fn nested_groups_allowed_and_dangling_rejected() {
        let mut s = store();
        let d = s.native.as_mut().unwrap();
        d["outbounds"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"selector","tag":"media","outbounds":["proxy","direct"]}));
        references(d).unwrap();
        d["outbounds"][0]["default"] = json!("missing");
        assert!(references(d).is_err());
    }
    #[test]
    fn dns_direct_dial_rejects_empty_direct_outbound_detour() {
        let mut s = store();
        let d = s.native.as_mut().unwrap();
        d["dns"]["servers"][1]["detour"] = json!("direct");
        assert!(references(d)
            .unwrap_err()
            .to_string()
            .contains("clear detour"));
        d["dns"]["servers"][1]
            .as_object_mut()
            .unwrap()
            .remove("detour");
        references(d).unwrap();
    }
    #[test]
    fn resource_refresh_leaves_dns_and_rule_order_alone() {
        let old = store();
        let mut new = old.clone();
        new.rule_resources.push(crate::model::RuleResource {
            id: "abc".into(),
            name: "Video".into(),
            source: "private".into(),
            format: "qx".into(),
            updated_at: 0,
            digest: "x".into(),
            input_count: 1,
            rules: vec![crate::model::MatchRule {
                kind: "domain_suffix".into(),
                value: "example.invalid".into(),
            }],
            warnings: vec![],
        });
        reconcile(&old, &mut new).unwrap();
        assert_eq!(
            new.native.as_ref().unwrap()["dns"],
            old.native.as_ref().unwrap()["dns"]
        );
        assert_eq!(
            new.native.as_ref().unwrap()["route"]["rules"],
            old.native.as_ref().unwrap()["route"]["rules"]
        );
    }
    #[test]
    fn refresh_conflicts_preserve_user_edits() {
        let mut old = store();
        old.native.as_mut().unwrap()["outbounds"][2]["connect_timeout"] = json!("9s");
        let mut new = old.clone();
        new.nodes[0].outbound["server_port"] = json!(8443);
        assert!(reconcile(&old, &mut new).is_err());
    }
    #[test]
    fn management_is_protected_and_tun_is_orthogonal() {
        let mut s = store();
        s.native.as_mut().unwrap()["inbounds"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"tun"}));
        assert!(uses_tun(&s));
        assert_eq!(s.settings.mode, "port");
        s.native.as_mut().unwrap()["services"][0]["listen"] = json!("0.0.0.0");
        assert!(effective(&s).is_err());
    }
}
