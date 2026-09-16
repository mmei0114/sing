//! Native documents are authoritative. Import metadata never rebuilds a document.
pub mod links;
pub mod resource;
pub mod review;
use crate::{config, model::Store};
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GroupChange {
    pub revision: String,
    pub original_tag: Option<String>,
    pub name: String,
    pub value: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionSetup {
    pub revision: String,
    pub target: Option<String>,
    pub mode: String,
    pub capture: String,
}
pub fn tun_template() -> Value {
    json!({"type":"tun","tag":"tun-in","address":["172.19.0.1/30"],"auto_route":true,"stack":"mixed","dns_mode":"hijack"})
}
pub fn connection_setup_review(before: &Store, next: &Store) -> Result<String> {
    // This is a save preview, not a statement about an active core.
    let _ = config::generate(next)?;
    let old = migration(before)?;
    let new = next.native.as_ref().context("Missing native draft")?;
    let mut paths = vec![];
    diff(&old, new, "", &mut paths);
    let target = if next.settings.route_mode == "global" {
        next.settings.global_target.as_str()
    } else {
        new.pointer("/route/final")
            .and_then(Value::as_str)
            .unwrap_or("(core default)")
    };
    Ok(format!("Preview only — Save Draft does not start or restart the core.\n\nMode: {} → {}\n{}: {}\nCapture: {}{}\nDNS and existing routing rules unchanged.\n{}\nChanged native fields:\n{}\n\nNext: review Start / Apply separately.{}",
        before.settings.route_mode,next.settings.route_mode,
        if next.settings.route_mode=="global"{"Global target"}else if next.settings.route_mode=="direct"{"Saved rule target (unused in Direct)"}else{"Unmatched traffic target"},crate::model::clean(target),
        if uses_tun(next){"TUN"}else{"Proxy Ports"},if next.settings.mode=="system"{" + System Proxy"}else{""},
        if before.native.is_none(){"Initializes native configuration with a private backup on save."}else{""},
        if paths.is_empty(){"  No native field changes".into()}else{paths.iter().map(|p|format!("  {p}")).collect::<Vec<_>>().join("\n")},
        if uses_tun(next){" TUN needs authorization and can interrupt SSH."}else{""}))
}
pub fn connection_setup(
    store: &Store,
    change: &ConnectionSetup,
    ssh: bool,
    system_supported: bool,
) -> Result<Store> {
    ensure!(
        change.revision == revision(store),
        "Draft changed; reopen connection setup"
    );
    ensure!(
        ["rule", "global", "direct"].contains(&change.mode.as_str()),
        "Invalid routing mode"
    );
    ensure!(
        ["keep", "port", "system", "tun"].contains(&change.capture.as_str()),
        "Invalid capture option"
    );
    let mut next = store.clone();
    if next.native.is_none() {
        let d = migration(&next)?;
        adopt(&mut next, d)?;
    }
    let tun = uses_tun(&next);
    let doc = next.native.as_mut().unwrap();
    if let Some(target) = &change.target {
        ensure!(
            ["/outbounds", "/endpoints"]
                .iter()
                .any(|p| array(doc, p).iter().any(|v| tag(v) == target)),
            "Selected target no longer exists"
        );
        ensure!(
            change.mode != "direct",
            "Direct mode does not use a proxy target; choose Keep current"
        );
        if change.mode == "global" {
            next.settings.global_target = target.clone();
        } else {
            set(doc, "/route/final", json!(target))?;
        }
    }
    next.settings.route_mode = change.mode.clone();
    match change.capture.as_str() {
        "system" => {
            ensure!(
                system_supported && !ssh,
                "System Proxy is available only on local macOS; SSH controls the remote host"
            );
            ensure!(!tun,"Existing TUN is preserved; edit it under Network / Inbounds before selecting System Proxy");
            next.settings.mode = "system".into();
        }
        "port" => {
            ensure!(!tun,"Existing TUN is preserved; edit it under Network / Inbounds before selecting Proxy Ports only");
            next.settings.mode = "port".into();
        }
        "tun" => {
            if !tun {
                ensure!(
                    !array(doc, "/inbounds").iter().any(|v| tag(v) == "tun-in"),
                    "TUN tag already exists; configure it under Network / Inbounds"
                );
                doc.as_object_mut()
                    .unwrap()
                    .entry("inbounds")
                    .or_insert(json!([]))
                    .as_array_mut()
                    .context("Inbounds must be a list")?
                    .push(tun_template());
            }
            next.settings.mode = "port".into();
        }
        _ => {}
    }
    shape(doc)?;
    Ok(next)
}

/// Validate the affected group's reachable outbound graph, allowing unrelated
/// unfinished draft objects to remain editable. Apply validates the whole doc.
pub fn validate_group(doc: &Value, group: &Value) -> Result<()> {
    let own = tag(group);
    ensure!(!own.is_empty(), "Group tag is required");
    ensure!(
        ["selector", "urltest"].contains(&group["type"].as_str().unwrap_or("")),
        "Choose Manual or Automatic"
    );
    let members = group["outbounds"]
        .as_array()
        .context("Choose group members")?;
    ensure!(!members.is_empty(), "Select at least one member");
    let mut unique = HashSet::new();
    for member in members {
        ensure!(
            unique.insert(member.as_str().context("Member must be an outbound tag")?),
            "Duplicate group member"
        );
    }
    if let Some(default) = group["default"].as_str().filter(|s| !s.is_empty()) {
        ensure!(
            members.iter().any(|m| m == default),
            "Default member must be selected"
        );
    }
    let mut objects = HashMap::new();
    for p in ["/outbounds", "/endpoints"] {
        for v in array(doc, p) {
            ensure!(
                objects.insert(tag(v), v).is_none(),
                "Duplicate outbound tag: {}",
                tag(v)
            );
        }
    }
    objects.insert(own, group);
    fn visit<'a>(
        t: &'a str,
        objects: &HashMap<&'a str, &'a Value>,
        active: &mut HashSet<&'a str>,
        done: &mut HashSet<&'a str>,
    ) -> Result<()> {
        if done.contains(t) {
            return Ok(());
        }
        ensure!(active.insert(t), "Circular group / outbound reference: {t}");
        let v = objects
            .get(t)
            .with_context(|| format!("Member no longer exists: {t}"))?;
        if ["selector", "urltest"].contains(&v["type"].as_str().unwrap_or("")) {
            ensure!(
                !array(v, "/outbounds").is_empty(),
                "Referenced group has no members: {t}"
            );
            if let Some(default) = v["default"].as_str().filter(|s| !s.is_empty()) {
                ensure!(
                    array(v, "/outbounds").iter().any(|m| m == default),
                    "Invalid default in referenced group: {t}"
                );
            }
        }
        for member in array(v, "/outbounds") {
            visit(
                member.as_str().context("Member must be an outbound tag")?,
                objects,
                active,
                done,
            )?;
        }
        if let Some(detour) = v["detour"].as_str().filter(|s| !s.is_empty()) {
            visit(detour, objects, active, done)?;
        }
        active.remove(t);
        done.insert(t);
        Ok(())
    }
    visit(own, &objects, &mut HashSet::new(), &mut HashSet::new())
}

pub fn write_group(store: &mut Store, change: GroupChange) -> Result<()> {
    ensure!(
        change.revision == revision(store),
        "Draft changed. Reopen the group before saving."
    );
    let name = crate::model::clean(change.name.trim());
    ensure!(!name.is_empty(), "Group name is required");
    let group_tag = tag(&change.value).to_string();
    let mut next = store.clone();
    let doc = next
        .native
        .as_mut()
        .context("Initialize native configuration first")?;
    validate_group(doc, &change.value)?;
    ensure!(
        !array(doc, "/endpoints").iter().any(|v| tag(v) == group_tag),
        "Group tag is already used by an endpoint"
    );
    let list = doc["outbounds"]
        .as_array_mut()
        .context("Outbounds must be a list")?;
    if let Some(original) = &change.original_tag {
        ensure!(
            original == &group_tag,
            "Group editor preserves its native tag; use explicit native editing to rename it"
        );
        let item = list
            .iter_mut()
            .find(|v| tag(v) == original)
            .context("Group no longer exists")?;
        ensure!(
            ["selector", "urltest"].contains(&item["type"].as_str().unwrap_or("")),
            "Selected outbound is not a group"
        );
        *item = change.value;
    } else {
        ensure!(
            !list.iter().any(|v| tag(v) == group_tag),
            "Group tag already exists"
        );
        list.push(change.value);
    }
    shape(doc)?;
    next.display_names.insert(group_tag, name);
    *store = next;
    Ok(())
}

/// Import a converted resource and its route together. All mutations are staged
/// in a clone so failed target/position/conflict checks leave the caller intact.
pub fn bind_rule_resource(
    store: &mut Store,
    resource: &crate::model::RuleResource,
    target: &str,
    position: usize,
) -> Result<()> {
    let mut next = store.clone();
    let doc = next
        .native
        .as_mut()
        .context("Initialize native configuration first")?;
    ensure!(
        !resource.native_rules().is_empty(),
        "Rule set contains no supported rules"
    );
    ensure!(
        target == "reject"
            || ["/outbounds", "/endpoints"]
                .iter()
                .any(|p| array(doc, p).iter().any(|v| tag(v) == target)),
        "Choose an existing target"
    );
    let count = array(doc, "/route/rules").len();
    ensure!(
        position <= count,
        "Insertion position changed. Review the rule order again."
    );
    let native_rules = json!(resource.native_rules());
    let mut sets = array(doc, "/route/rule_set").to_vec();
    if let Some(existing) = sets.iter_mut().find(|v| tag(v) == resource.tag()) {
        let original = store
            .rule_resources
            .iter()
            .find(|r| r.id == resource.id)
            .context("Rule-set tag is already used by a native object")?;
        ensure!(
            existing["type"] == "inline" && existing["rules"] == json!(original.native_rules()),
            "Rule set has local edits; review or detach it before importing again"
        );
        existing["rules"] = native_rules;
    } else {
        sets.push(json!({"type":"inline","tag":resource.tag(),"rules":native_rules}));
    }
    set(doc, "/route/rule_set", json!(sets))?;
    let rule = if target == "reject" {
        json!({"rule_set":[resource.tag()],"action":"reject"})
    } else {
        json!({"rule_set":[resource.tag()],"action":"route","outbound":target})
    };
    let mut rules = array(doc, "/route/rules").to_vec();
    rules.insert(position, rule);
    set(doc, "/route/rules", json!(rules))?;
    shape(doc)?;
    if let Some(old) = next.rule_resources.iter_mut().find(|r| r.id == resource.id) {
        *old = resource.clone();
    } else {
        next.rule_resources.push(resource.clone());
    }
    if !next.rule_bindings.iter().any(|b| b.resource == resource.id) {
        next.rule_bindings.push(crate::model::RuleBinding {
            id: crate::model::id(&format!("binding:{}", resource.id)),
            resource: resource.id.clone(),
            target: target.into(),
            enabled: true,
        });
    }
    *store = next;
    Ok(())
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
    links::check_removals(store.native.as_ref().unwrap(), &doc)?;
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
    ensure!(
        ["rule", "global", "direct"].contains(&store.settings.route_mode.as_str()),
        "Unknown routing mode"
    );
    live_modes(&mut doc, &store.settings)?;
    references(&doc)?;
    Ok(doc)
}

/// Selector generated for Global mode. It is part of the running configuration
/// only; the saved native document never contains it.
pub const GLOBAL_TAG: &str = "GLOBAL";
const DIRECT_FALLBACK: &str = "sing-direct";

/// Rule / Global / Direct are sing-box clash modes, switched live through the
/// management API. Saved rules, DNS and outbounds are never rewritten: mode
/// rules are inserted before the first rule that decides a route.
fn live_modes(doc: &mut Value, settings: &crate::model::Settings) -> Result<()> {
    let outbounds = array(doc, "/outbounds").to_vec();
    ensure!(
        !outbounds
            .iter()
            .chain(array(doc, "/endpoints"))
            .any(|v| tag(v) == GLOBAL_TAG),
        "Outbound tag {GLOBAL_TAG} is reserved for Global mode; rename that object"
    );
    let direct = outbounds
        .iter()
        .find(|v| v["type"] == "direct")
        .map(|v| tag(v).to_string())
        .unwrap_or_else(|| DIRECT_FALLBACK.into());
    let mut members: Vec<String> = outbounds
        .iter()
        .filter(|v| ["selector", "urltest"].contains(&v["type"].as_str().unwrap_or("")))
        .chain(outbounds.iter().filter(|v| {
            !["selector", "urltest", "block", "dns"].contains(&v["type"].as_str().unwrap_or(""))
        }))
        .chain(array(doc, "/endpoints"))
        .map(|v| tag(v).to_string())
        .filter(|t| !t.is_empty())
        .collect();
    if !members.contains(&direct) {
        members.push(direct.clone());
    }
    let default = if members.contains(&settings.global_target) {
        settings.global_target.clone()
    } else {
        members[0].clone()
    };
    let list = doc
        .as_object_mut()
        .context("Native configuration must be a JSON object")?
        .entry("outbounds")
        .or_insert(json!([]))
        .as_array_mut()
        .context("outbounds must be an array")?;
    if direct == DIRECT_FALLBACK {
        list.push(json!({"type":"direct","tag":DIRECT_FALLBACK}));
    }
    list.push(json!({"type":"selector","tag":GLOBAL_TAG,"outbounds":members,"default":default}));
    let route = doc
        .as_object_mut()
        .unwrap()
        .entry("route")
        .or_insert(json!({}))
        .as_object_mut()
        .context("route must be an object")?;
    let rules = route
        .entry("rules")
        .or_insert(json!([]))
        .as_array_mut()
        .context("route.rules must be an array")?;
    let at = rules
        .iter()
        .position(|r| {
            !["sniff", "resolve", "route-options", "hijack-dns"]
                .contains(&r["action"].as_str().unwrap_or("route"))
        })
        .unwrap_or(rules.len());
    let mut injected = vec![json!({"clash_mode":"direct","action":"route","outbound":direct})];
    if settings.bypass_lan {
        injected.push(
            json!({"clash_mode":"global","ip_is_private":true,"action":"route","outbound":direct}),
        );
    }
    injected.push(json!({"clash_mode":"global","action":"route","outbound":GLOBAL_TAG}));
    for (i, rule) in injected.into_iter().enumerate() {
        rules.insert(at + i, rule);
    }
    let experimental = doc
        .as_object_mut()
        .unwrap()
        .entry("experimental")
        .or_insert(json!({}))
        .as_object_mut()
        .context("experimental must be an object")?;
    let clash = experimental
        .entry("clash_api")
        .or_insert(json!({}))
        .as_object_mut()
        .context("experimental.clash_api must be an object")?;
    clash.insert("default_mode".into(), json!(settings.route_mode));
    Ok(())
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
        let removed: Vec<_> = old
            .nodes
            .iter()
            .filter(|n| !new.nodes.iter().any(|v| v.id == n.id))
            .map(|n| n.tag())
            .collect();
        for tag in &removed {
            let uses = reference_paths(doc, tag);
            ensure!(uses.is_empty(), "Removed subscription node {tag} is still referenced at {}. Edit those references or detach the node before updating.",uses.join(", "));
        }
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
        if before.is_some_and(|r| {
            r.rules == resource.rules && r.native_document == resource.native_document
        }) {
            continue;
        }
        let sets = doc["route"]
            .as_object_mut()
            .context("route must be an object")?
            .entry("rule_set")
            .or_insert(json!([]))
            .as_array_mut()
            .context("rule_set must be an array")?;
        let value = json!({"type":"inline","tag":resource.tag(),"rules":resource.native_rules()});
        if let Some(i) = sets.iter().position(|s| tag(s) == resource.tag()) {
            if let Some(before) = before {
                ensure!(
                    sets[i]["rules"] == json!(before.native_rules()),
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

pub fn reference_paths(doc: &Value, tag: &str) -> Vec<String> {
    links::links(doc)
        .into_iter()
        .filter(|l| l.kind == links::ObjectKind::Outbound && l.tag == tag)
        .map(|l| l.path)
        .collect()
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
        result.push_str("Global / Direct take precedence over your rules; switching mode later is live and needs no restart. DNS servers/rules and their outbound paths are UNCHANGED.\n\n");
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
    result.push_str(&review::summary(saved, &before, &proposed));
    result.push_str("\n\nChanged sections (open Native Diff for values):\n");
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
    fn group_change(s: &Store) -> GroupChange {
        GroupChange {
            revision: revision(s),
            original_tag: None,
            name: "Media".into(),
            value: json!({"type":"selector","tag":"media","outbounds":["proxy","direct"],"default":"proxy","future_option":{"keep":true}}),
        }
    }
    #[test]
    fn connection_setup_preserves_native_policy_and_custom_tun() {
        let mut s = store();
        let before = s.native.clone().unwrap();
        let mut change = ConnectionSetup {
            revision: revision(&s),
            target: Some("direct".into()),
            mode: "global".into(),
            capture: "keep".into(),
        };
        let next = connection_setup(&s, &change, false, true).unwrap();
        assert_eq!(next.native, s.native);
        assert_eq!(next.settings.global_target, "direct");
        change.capture = "system".into();
        assert!(connection_setup(&s, &change, true, true).is_err());
        assert!(connection_setup(&s, &change, false, false).is_err());
        change.capture = "tun".into();
        let next = connection_setup(&s, &change, false, true).unwrap();
        assert_eq!(next.native.as_ref().unwrap()["dns"], before["dns"]);
        assert_eq!(next.native.as_ref().unwrap()["route"], before["route"]);
        s = next;
        change.revision = revision(&s);
        let keep = connection_setup(&s, &change, false, true).unwrap();
        assert_eq!(keep.native, s.native);
        change.capture = "port".into();
        assert!(connection_setup(&s, &change, false, true).is_err());
    }
    #[test]
    fn group_transaction_keeps_native_fields_and_stable_references() {
        let mut s = store();
        let before = s.native.clone().unwrap();
        let c = group_change(&s);
        write_group(&mut s, c.clone()).unwrap();
        assert_eq!(s.display_names["media"], "Media");
        assert_eq!(s.native.as_ref().unwrap()["dns"], before["dns"]);
        assert_eq!(s.native.as_ref().unwrap()["route"], before["route"]);
        let mut rename = c.clone();
        rename.revision = revision(&s);
        rename.original_tag = Some("media".into());
        rename.name = "Video".into();
        write_group(&mut s, rename).unwrap();
        assert_eq!(s.display_names["media"], "Video");
        assert_eq!(
            array(s.native.as_ref().unwrap(), "/outbounds")
                .last()
                .unwrap(),
            &c.value
        );
        let snapshot = serde_json::to_value(&s).unwrap();
        assert!(write_group(&mut s, c)
            .unwrap_err()
            .to_string()
            .contains("Draft changed"));
        assert_eq!(serde_json::to_value(&s).unwrap(), snapshot);
        let dir = tempfile::tempdir().unwrap();
        s.save(dir.path()).unwrap();
        assert_eq!(
            Store::load(dir.path()).unwrap().display_names,
            s.display_names
        );
    }
    #[test]
    fn invalid_group_changes_leave_entire_store_untouched() {
        let mut s = store();
        for value in [
            json!({"type":"selector","tag":"media","outbounds":[]}),
            json!({"type":"selector","tag":"media","outbounds":["absent"]}),
            json!({"type":"selector","tag":"media","outbounds":["direct"],"default":"proxy"}),
            json!({"type":"selector","tag":"media","outbounds":["media"]}),
            json!({"type":"selector","tag":"media","outbounds":["direct","direct"]}),
            json!({"type":"selector","tag":"direct","outbounds":["proxy"]}),
        ] {
            let before = serde_json::to_value(&s).unwrap();
            let mut c = group_change(&s);
            c.value = value;
            assert!(write_group(&mut s, c).is_err());
            assert_eq!(serde_json::to_value(&s).unwrap(), before);
        }
        s.native.as_mut().unwrap()["endpoints"] = json!([{"type":"wireguard","tag":"media"}]);
        let c = group_change(&s);
        assert!(write_group(&mut s, c)
            .unwrap_err()
            .to_string()
            .contains("endpoint"));
        s.native.as_mut().unwrap()["endpoints"] = json!([]);
        s.native.as_mut().unwrap()["outbounds"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"selector","tag":"nested","outbounds":["media"]}));
        let mut c = group_change(&s);
        c.value["outbounds"] = json!(["nested"]);
        c.value["default"] = json!("nested");
        assert!(write_group(&mut s, c)
            .unwrap_err()
            .to_string()
            .contains("Circular"));
    }
    #[test]
    fn referenced_object_removal_is_atomic_and_dns_names_do_not_block_outbounds() {
        let mut s = store();
        s.native = Some(
            json!({"outbounds":[{"type":"direct","tag":"same"}],"dns":{"servers":[{"type":"local","tag":"same"}],"final":"same","rules":[{"rule_set":["video"],"server":"same"}]},"route":{"rule_set":[{"type":"inline","tag":"video","rules":[{"domain_suffix":["example.test"]}]}]}}),
        );
        let before = serde_json::to_value(&s).unwrap();
        let edit = Edit {
            revision: revision(&s),
            pointer: "/route/rule_set".into(),
            value: json!([]),
        };
        assert!(write(&mut s, edit)
            .unwrap_err()
            .to_string()
            .contains("/dns/rules/0/rule_set"));
        assert_eq!(serde_json::to_value(&s).unwrap(), before);
        let edit = Edit {
            revision: revision(&s),
            pointer: "/outbounds".into(),
            value: json!([]),
        };
        write(&mut s, edit).unwrap();
        assert_eq!(s.native.as_ref().unwrap()["dns"]["final"], "same");
        let mut doc = s.native.clone().unwrap();
        doc["route"]["rule_set"] = json!([]);
        doc["dns"]["rules"] = json!([]);
        let edit = Edit {
            revision: revision(&s),
            pointer: String::new(),
            value: doc,
        };
        write(&mut s, edit).unwrap();
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
            assert_eq!(c["route"]["rules"], before["route"]["rules"]);
            assert_eq!(c["experimental"]["clash_api"]["default_mode"], mode);
        }
    }
    #[test]
    fn live_modes_precede_first_route_rule_and_keep_saved_document() {
        let mut s = store();
        s.native.as_mut().unwrap()["route"]["rules"] = json!([
            {"action":"sniff"},
            {"protocol":"dns","action":"hijack-dns"},
            {"domain_suffix":["example.invalid"],"action":"route","outbound":"direct"}
        ]);
        let saved = s.native.clone().unwrap();
        let c = effective(&s).unwrap();
        let rules = array(&c, "/route/rules");
        assert_eq!(rules[2]["clash_mode"], "direct");
        assert_eq!(rules[3]["ip_is_private"], true);
        assert_eq!(rules[4]["outbound"], GLOBAL_TAG);
        assert_eq!(rules[5]["domain_suffix"][0], "example.invalid");
        let global = array(&c, "/outbounds")
            .iter()
            .find(|v| tag(v) == GLOBAL_TAG)
            .unwrap();
        assert!(array(global, "/outbounds").iter().any(|m| m == "proxy"));
        assert_eq!(global["default"], "proxy");
        assert_eq!(s.native.as_ref().unwrap(), &saved);
        s.native.as_mut().unwrap()["outbounds"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"direct","tag":GLOBAL_TAG}));
        assert!(effective(&s).is_err());
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
            native_document: None,
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
