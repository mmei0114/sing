use crate::{
    model::{MatchRule, ProxyGroup, Settings, Store},
    ruleset,
};
use anyhow::{bail, ensure, Result};
use serde_json::{json, Value};

pub fn validate(s: &Settings) -> Result<()> {
    ensure!(
        ["rule", "global", "direct"].contains(&s.route_mode.as_str()),
        "Routing mode must be rule, global or direct"
    );
    ensure!(
        s.global_target == "proxy" || s.global_target.starts_with("g-"),
        "Global proxy target must be proxy or a group"
    );
    ensure!(
        ["port", "system", "tun"].contains(&s.mode.as_str()),
        "Mode must be port, system or tun"
    );
    ensure!(
        s.mode != "system" || cfg!(target_os = "macos"),
        "System proxy integration is macOS-only; use Port mode on Linux"
    );
    ensure!(
        ["proxy", "direct"].contains(&s.routing.as_str()) || s.routing.starts_with("g-"),
        "Routing must be proxy or direct"
    );
    ensure!(
        ["legacy", "paired"].contains(&s.dns_policy.as_str()),
        "Unknown DNS policy"
    );
    ensure!(
        ["ipv4_only", "prefer_ipv4", "prefer_ipv6", "ipv6_only"].contains(&s.dns_strategy.as_str()),
        "Unknown DNS strategy"
    );
    ensure!(
        s.port >= 1024 && s.api_port >= 1024 && s.port != s.api_port,
        "Ports must be different and at least 1024"
    );
    ensure!(
        s.dns.parse::<std::net::IpAddr>().is_ok(),
        "DNS must be a resolver IP, e.g. 1.1.1.1"
    );
    ensure!(
        ["en", "zh"].contains(&s.language.as_str()),
        "Language must be en or zh"
    );
    for r in &s.rules {
        ensure!(
            [
                "domain",
                "domain_suffix",
                "domain_keyword",
                "domain_regex",
                "ip_cidr",
                "process_name",
                "process_path"
            ]
            .contains(&r.kind.as_str()),
            "Unsupported rule kind"
        );
        ensure!(
            !r.value.trim().is_empty() && !r.value.chars().any(char::is_control),
            "Rule value cannot be blank or contain control characters"
        );
        ensure!(
            ["proxy", "direct", "reject"].contains(&r.target.as_str())
                || r.target.starts_with("g-"),
            "Rule target must be proxy, direct or reject"
        );
        ruleset::validate_match(&MatchRule {
            kind: r.kind.clone(),
            value: r.value.clone(),
        })?;
        if r.kind == "ip_cidr" {
            let (ip, prefix) = r
                .value
                .split_once('/')
                .ok_or_else(|| anyhow::anyhow!("CIDR needs a prefix: 10.0.0.0/8"))?;
            let ip: std::net::IpAddr = ip.parse()?;
            let p: u8 = prefix.parse()?;
            ensure!(
                p <= if ip.is_ipv4() { 32 } else { 128 },
                "Invalid CIDR prefix"
            );
        }
    }
    Ok(())
}

pub fn target_exists(store: &Store, target: &str, reject: bool) -> bool {
    if let Some(doc) = &store.native {
        return (reject && target == "reject")
            || ["/outbounds", "/endpoints"].iter().any(|p| {
                crate::native::array(doc, p)
                    .iter()
                    .any(|v| crate::native::tag(v) == target)
            });
    }
    ["proxy", "direct"].contains(&target)
        || (reject && target == "reject")
        || store.proxy_groups.iter().any(|g| g.tag() == target)
}
pub fn validate_group(store: &Store, group: &ProxyGroup) -> Result<()> {
    ensure!(
        !group.id.is_empty() && group.id.bytes().all(|c| c.is_ascii_hexdigit()),
        "Invalid group identifier"
    );
    ensure!(
        !group.name.trim().is_empty()
            && group.name == crate::model::clean(&group.name)
            && group.name.len() <= 80,
        "Group name must be 1–80 bytes without control characters"
    );
    ensure!(
        !["proxy", "direct", "reject"].contains(&group.name.to_lowercase().as_str()),
        "Group name is reserved"
    );
    ensure!(
        !store
            .proxy_groups
            .iter()
            .any(|g| g.id != group.id && g.name.to_lowercase() == group.name.to_lowercase()),
        "Group name already exists"
    );
    ensure!(
        ["selector", "urltest"].contains(&group.kind.as_str()),
        "Unknown group type"
    );
    ensure!(
        !group.members.is_empty(),
        "Select at least one group member"
    );
    let mut ids = std::collections::HashSet::new();
    for id in &group.members {
        ensure!(ids.insert(id), "Duplicate group member");
        ensure!(
            store.nodes.iter().any(|n| &n.id == id),
            "Group '{}' contains an unavailable node; edit its members before applying",
            group.name
        );
    }
    ensure!(
        group
            .selected
            .as_ref()
            .is_some_and(|id| group.members.contains(id)),
        "Choose a default member for group '{}'",
        group.name
    );
    Ok(())
}
pub fn validate_references(store: &Store) -> Result<()> {
    ensure!(
        target_exists(store, &store.settings.global_target, false),
        "Global proxy target references a missing group"
    );
    ensure!(
        target_exists(store, &store.settings.routing, false),
        "Default route references a missing group"
    );
    for r in &store.settings.rules {
        ensure!(
            target_exists(store, &r.target, true),
            "Manual rule references a missing group"
        );
    }
    let mut ids = std::collections::HashSet::new();
    for g in &store.proxy_groups {
        ensure!(ids.insert(g.id.clone()), "Duplicate group ID");
    }
    ids.clear();
    for resource in &store.rule_resources {
        ensure!(!resource.rules.is_empty(), "Rule resource is empty");
        ensure!(
            ids.insert(resource.id.clone()),
            "Duplicate rule resource ID"
        );
        for r in &resource.rules {
            ruleset::validate_match(r)?;
        }
    }
    ids.clear();
    for b in &store.rule_bindings {
        ensure!(ids.insert(b.id.clone()), "Duplicate rule binding ID");
        ensure!(
            store.rule_resources.iter().any(|r| r.id == b.resource),
            "Missing rule resource"
        );
        ensure!(
            target_exists(store, &b.target, true),
            "Rule binding references a missing group"
        );
    }
    Ok(())
}

pub fn generate(store: &Store) -> Result<Value> {
    if store.native.is_some() {
        return crate::native::effective(store);
    }
    validate(&store.settings)?;
    let mut effective = store.clone();
    if store.settings.route_mode != "rule" {
        effective.settings.rules.clear();
        effective.rule_bindings.clear();
        effective.rule_resources.clear();
        // Mode override is compiled into native configuration. Saved policies
        // stay intact, and DNS follows the override instead of stale rule DNS.
        effective.settings.dns_policy = "paired".into();
        if store.settings.route_mode == "direct" {
            effective.settings.routing = "direct".into();
            effective.settings.global_target = "proxy".into();
            effective.proxy_groups.clear();
            effective.nodes.clear();
        } else {
            effective.settings.routing = store.settings.global_target.clone();
            effective
                .proxy_groups
                .retain(|g| g.tag() == store.settings.global_target);
        }
    }
    generate_effective(&effective)
}

fn generate_effective(store: &Store) -> Result<Value> {
    let s = &store.settings;
    validate(s)?;
    validate_references(store)?;
    for g in &store.proxy_groups {
        validate_group(store, g)?;
    }
    if store.nodes.is_empty() && s.route_mode != "direct" {
        bail!("Import a subscription before connecting");
    }
    let tags: Vec<String> = store.nodes.iter().map(|n| n.tag()).collect();
    let selected = store
        .selected
        .as_ref()
        .and_then(|id| store.nodes.iter().find(|n| &n.id == id))
        .or_else(|| store.nodes.first())
        .map(|n| n.tag());
    let mut outbounds = vec![];
    if let Some(selected) = selected {
        outbounds.push(json!({"type":"selector","tag":"proxy","outbounds":tags,"default":selected,"interrupt_exist_connections":false}));
    }
    outbounds.push(json!({"type":"direct","tag":"direct"}));
    for g in &store.proxy_groups {
        let members: Vec<_> = g.members.iter().map(|id| format!("n-{id}")).collect();
        let outbound = if g.kind == "selector" {
            json!({"type":"selector","tag":g.tag(),"outbounds":members,"default":format!("n-{}",g.selected.as_ref().unwrap()),"interrupt_exist_connections":false})
        } else {
            json!({"type":"urltest","tag":g.tag(),"outbounds":members,"url":"https://www.gstatic.com/generate_204","interval":"3m","tolerance":50,"interrupt_exist_connections":false})
        };
        outbounds.push(outbound);
    }
    for node in &store.nodes {
        let mut n = node.outbound.clone();
        n["tag"] = json!(node.tag());
        outbounds.push(n);
    }
    let mut inbounds =
        vec![json!({"type":"mixed","tag":"mixed-in","listen":"127.0.0.1","listen_port":s.port})];
    if s.mode == "tun" {
        inbounds.push(json!({"type":"tun","tag":"tun-in","address":["172.19.0.1/30"],"auto_route":true,"strict_route":false,"stack":"mixed"}));
    }
    let mut rules = vec![
        json!({"action":"sniff"}),
        json!({"protocol":"dns","action":"hijack-dns"}),
    ];
    if s.bypass_lan {
        rules.push(json!({"ip_is_private":true,"outbound":"direct"}));
    }
    for r in &s.rules {
        let mut rule = json!({});
        rule[&r.kind] = json!([r.value]);
        if r.target == "reject" {
            rule["action"] = json!("reject");
        } else {
            rule["outbound"] = json!(r.target);
        }
        rules.push(rule);
    }
    let mut sets = vec![];
    for resource in &store.rule_resources {
        sets.push(json!({"type":"inline","tag":resource.tag(),"rules":ruleset::native_rules(&resource.rules)}));
    }
    for b in store.rule_bindings.iter().filter(|b| b.enabled) {
        let resource = store
            .rule_resources
            .iter()
            .find(|r| r.id == b.resource)
            .unwrap();
        let mut rule = json!({"rule_set":[resource.tag()],"action":"route","outbound":b.target});
        if b.target == "reject" {
            rule = json!({"rule_set":[resource.tag()],"action":"reject"});
        }
        rules.push(rule);
    }
    let mut c = json!({
        "log":{"level":"warn","timestamp":true},
        "dns":{"servers":[{"type":"local","tag":"bootstrap"},{"type":"https","tag":"secure-dns","server":s.dns,"server_port":443,"path":"/dns-query","detour":s.routing}],"final":"secure-dns","strategy":s.dns_strategy},
        "inbounds":inbounds,"outbounds":outbounds,
        "route":{"rules":rules,"rule_set":sets,"final":s.routing,"auto_detect_interface":true,"default_domain_resolver":"bootstrap"},
        "services":[{"type":"api","tag":"management","listen":"127.0.0.1","listen_port":s.api_port,"secret":store.secret}]
    });
    // Direct dialing is represented by the absence of a detour. A detour to
    // an unconfigured direct outbound passes check but fails at core startup.
    if s.routing == "direct" {
        c["dns"]["servers"][1]
            .as_object_mut()
            .unwrap()
            .remove("detour");
    }
    if s.dns_policy == "paired" {
        let dns_tag = |target: &str| {
            if target == "direct" {
                "bootstrap".to_string()
            } else {
                format!("dns-{target}")
            }
        };
        let mut servers = vec![json!({"type":"local","tag":"bootstrap"})];
        for target in (!store.nodes.is_empty())
            .then_some("proxy".to_string())
            .into_iter()
            .chain(store.proxy_groups.iter().map(|g| g.tag()))
        {
            servers.push(json!({"type":"https","tag":dns_tag(&target),"server":s.dns,"server_port":443,"path":"/dns-query","detour":target}));
        }
        let mut dns_rules =
            vec![json!({"domain_suffix":["local"],"action":"route","server":"bootstrap"})];
        let action = |mut rule: Value, target: &str| {
            if target == "reject" {
                rule["action"] = json!("reject");
            } else {
                rule["action"] = json!("route");
                rule["server"] = json!(dns_tag(target));
            }
            rule
        };
        for r in &s.rules {
            if r.kind.starts_with("domain") {
                let mut rule = json!({});
                rule[&r.kind] = json!([r.value]);
                dns_rules.push(action(rule, &r.target));
            }
        }
        for b in store.rule_bindings.iter().filter(|b| b.enabled) {
            let resource = store
                .rule_resources
                .iter()
                .find(|r| r.id == b.resource)
                .unwrap();
            let domains = ruleset::domain_rules(&resource.rules);
            if !domains.is_empty() {
                let tag = format!("dns-{}-{}", resource.tag(), b.id);
                c["route"]["rule_set"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"type":"inline","tag":tag,"rules":domains}));
                dns_rules.push(action(json!({"rule_set":[tag]}), &b.target));
            }
        }
        c["dns"] = json!({"servers":servers,"rules":dns_rules,"final":dns_tag(&s.routing),"strategy":s.dns_strategy,"disable_cache":false});
    }
    Ok(c)
}

pub fn target_label(store: &Store, tag: &str) -> String {
    store
        .proxy_groups
        .iter()
        .find(|g| g.tag() == tag)
        .map(|g| g.name.clone())
        .or_else(|| {
            store
                .nodes
                .iter()
                .find(|n| n.tag() == tag)
                .map(|n| n.name.clone())
        })
        .unwrap_or_else(|| tag.into())
}

pub fn diagnostics(saved: &Store, running: Option<&Store>) -> String {
    if saved.native.is_some() {
        let describe = |s: &Store| {
            match crate::native::effective(s) {
            Ok(d) => format!("Routing: {}\nFinal outbound: {}\nFinal DNS: {}\nInbounds: {} · Outbounds: {} · DNS servers: {}\nReference checks: passed\n",
                s.settings.route_mode, d["route"]["final"], d["dns"]["final"], crate::native::array(&d,"/inbounds").len(),
                crate::native::array(&d,"/outbounds").len(), crate::native::array(&d,"/dns/servers").len()),
            Err(e) => format!("Configuration issue: {e}\n"),
        }
        };
        return format!("DRAFT\n{}\nRUNNING\n{}\nThis is a configuration report, not a network measurement.\nSystem proxy covers cooperating applications. TUN may configure interface DNS and routing; application DoH can differ.\nProcess data depends on capture and OS visibility. DNS latency, throughput and UDP loss are not measured here.",describe(saved),running.map(describe).unwrap_or_else(||"Not running\n".into()));
    }
    let zh = saved.settings.language == "zh";
    let t = |en, cn| if zh { cn } else { en };
    let mut text = t(
        "LOCAL CONFIGURATION DIAGNOSTICS\nNo DNS request or speed test was sent.\n\n",
        "本地配置诊断\n未发送 DNS 或测速请求。\n\n",
    )
    .to_string();
    if running.is_none() {
        text.push_str(t(
            "Core is stopped. Nothing below is currently active.\n\n",
            "核心未运行，以下为待应用配置。\n\n",
        ));
    }
    let describe = |store: &Store, title: &str| {
        let s = &store.settings;
        let mode = match s.route_mode.as_str() {
            "global" => t("Global proxy", "全局代理"),
            "direct" => t("Direct", "直连"),
            _ => t("Rule-based", "规则分流"),
        };
        let mut out = format!(
            "{title}\n{}: {} → 127.0.0.1:{}\n{}: {mode}\n{}: {}\n",
            t("Capture", "接管方式"),
            s.mode,
            s.port,
            t("Routing", "路由模式"),
            t("Private IP exception", "内网直连例外"),
            if s.bypass_lan {
                t("on", "开启")
            } else {
                t("off", "关闭")
            }
        );
        match s.route_mode.as_str() {
            "global" => out.push_str(&format!("{}: {}\n{}\n",t("Global target","全局目标"),target_label(store,&s.global_target),t("Saved rules are bypassed; the enabled private-IP exception still applies.","保留但不执行用户规则；内网直连例外若开启，仍然生效。"))),
            "direct" => out.push_str(t("No proxy nodes/groups are loaded or tested.\n","不加载代理节点或自动测速组。\n")),
            _ => out.push_str(&format!("{}: {}\n{}\n",t("Unmatched traffic target","未匹配流量目标"),target_label(store,&s.routing),t("Manual rules → enabled rule subscriptions → unmatched target. Matched traffic uses each rule's target.","手工规则 → 已启用规则集 → 未匹配目标；匹配流量使用各条规则指定的目标。"))),
        }
        match generate(store) {
            Ok(c) => {
                let policy = if s.route_mode != "rule" {
                    t("mode override", "模式覆盖")
                } else if s.dns_policy == "legacy" {
                    t("legacy", "兼容策略")
                } else {
                    t("paired", "配套策略")
                };
                out.push_str(&format!(
                    "\n{}: {} · {}\n",
                    t("Effective DNS", "有效 DNS"),
                    policy,
                    s.dns_strategy
                ));
                for server in c["dns"]["servers"].as_array().into_iter().flatten() {
                    let tag = server["tag"].as_str().unwrap_or("?");
                    if server["type"] == "local" {
                        out.push_str(&format!("  {tag}: {}\n", t("OS resolver", "系统解析器")));
                    } else {
                        let target = server["detour"]
                            .as_str()
                            .map(|v| target_label(store, v))
                            .unwrap_or_else(|| t("direct dial", "直接连接").into());
                        out.push_str(&format!(
                            "  {tag}: HTTPS {} {} {}\n",
                            server["server"].as_str().unwrap_or("?"),
                            t("via", "经"),
                            target
                        ));
                    }
                }
                out.push_str(&format!(
                    "  {}: {}\n  {}: {}\n",
                    t("Final DNS", "DNS 默认"),
                    c["dns"]["final"].as_str().unwrap_or("?"),
                    t("DNS routing entries", "DNS 规则数"),
                    c["dns"]["rules"].as_array().map_or(0, Vec::len)
                ));
            }
            Err(e) => out.push_str(&format!(
                "\n{}: {}\n",
                t("Configuration issue", "配置问题"),
                crate::model::clean(&e.to_string())
            )),
        }
        out
    };
    if let Some(r) = running {
        text.push_str(&describe(r, t("RUNNING", "当前生效")));
        text.push('\n');
    }
    text.push_str(&describe(
        saved,
        t("SAVED (c applies)", "已保存（c 应用后生效）"),
    ));
    text.push_str(t(
        "\nBOUNDARIES\n• System proxy covers only apps that honor it. TUN is experimental; SSH controls the remote host.\n• DNS policies affect only queries reaching sing-box DNS. App DoH, OS queries and remote-node resolution may differ. Bootstrap uses the OS resolver.\n• Connections shows core observations, not predictions. Missing process information does not prove an app bypassed sing.\n• DNS timing, packet loss, UDP/QUIC throughput and website reachability are not measured here. Use v for a separate explicit HTTPS check.\n",
        "\n覆盖边界\n• 系统代理仅覆盖遵循代理设置的应用；TUN 为实验功能，SSH 操作的是远端主机。\n• DNS 策略仅影响交给核心的查询。应用自带 DoH、系统查询与节点端解析可能不同；引导解析仍使用系统解析器。\n• 连接页显示实际观察，不预测命中。进程信息为空不代表应用绕过代理。\n• 本报告不测 DNS 耗时、丢包、UDP/QUIC 带宽或网站可达性；v 可另行确认并检查 HTTPS 路径。\n"));
    text
}

pub fn redacted(value: &Value) -> Value {
    match value {
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        if [
                            "password",
                            "uuid",
                            "secret",
                            "private_key",
                            "public_key",
                            "token",
                            "authorization",
                            "headers",
                            "certificate",
                            "key",
                            "pre_shared_key",
                            "url",
                            "download_url",
                        ]
                        .contains(&k.as_str())
                        {
                            json!("••••••")
                        } else {
                            redacted(v)
                        },
                    )
                })
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(redacted).collect()),
        x => x.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subscription;
    fn store() -> Store {
        let mut s = Store::new().unwrap();
        s.nodes = subscription::parse("trojan://test@example.com:443#Sample", "p")
            .unwrap()
            .nodes;
        s
    }
    #[test]
    fn generated_native_api_and_stable_tags() {
        let s = store();
        let c = generate(&s).unwrap();
        assert_eq!(c["services"][0]["type"], "api");
        assert_eq!(c["outbounds"][2]["tag"], s.nodes[0].tag());
        assert!(c.get("experimental").is_none());
    }
    #[test]
    fn modes_override_routes_and_dns_without_mutating_saved_policies() {
        let mut s = with_rules();
        s.settings.rules.push(crate::model::Rule {
            kind: "domain_suffix".into(),
            value: "blocked.invalid".into(),
            target: "reject".into(),
        });
        let original = generate(&s).unwrap();
        s.settings.route_mode = "global".into();
        s.settings.global_target = "g-abc123".into();
        s.settings.dns_policy = "legacy".into();
        let global = generate(&s).unwrap();
        assert_eq!(global["route"]["final"], "g-abc123");
        assert_eq!(global["dns"]["final"], "dns-g-abc123");
        assert_eq!(global["route"]["rules"].as_array().unwrap().len(), 3);
        assert!(global["route"]["rule_set"].as_array().unwrap().is_empty());
        assert_eq!(s.settings.rules.len(), 1);
        assert_eq!(s.rule_bindings.len(), 1);
        s.settings.route_mode = "direct".into();
        let direct = generate(&s).unwrap();
        assert_eq!(direct["route"]["final"], "direct");
        assert_eq!(direct["dns"]["final"], "bootstrap");
        assert_eq!(
            direct["outbounds"],
            json!([{"type":"direct","tag":"direct"}])
        );
        assert_eq!(direct["dns"]["servers"].as_array().unwrap().len(), 1);
        s.settings.route_mode = "rule".into();
        s.settings.dns_policy = "paired".into();
        assert_eq!(generate(&s).unwrap(), original);
    }
    #[test]
    fn direct_without_nodes_and_optional_lan_exception() {
        let mut s = Store::new().unwrap();
        assert!(generate(&s).is_err());
        s.settings.route_mode = "direct".into();
        s.settings.bypass_lan = false;
        let c = generate(&s).unwrap();
        assert_eq!(c["route"]["rules"].as_array().unwrap().len(), 2);
        assert_eq!(c["inbounds"][0]["type"], "mixed");
        assert!(c.get("experimental").is_none());
    }
    #[test]
    fn global_target_validation_and_inactive_groups() {
        let mut s = with_rules();
        s.settings.route_mode = "global".into();
        s.proxy_groups[0].members.push("missing".into());
        // An unused group must not prevent the built-in global proxy loading.
        assert!(generate(&s).is_ok());
        s.settings.global_target = "g-abc123".into();
        assert!(generate(&s).is_err());
        s.settings.global_target = "g-missing".into();
        assert!(generate(&s).is_err());
        s.settings.global_target = "direct".into();
        assert!(validate(&s.settings).is_err());
    }
    #[test]
    fn diagnostics_separates_saved_and_running_without_claiming_dns_measurement() {
        let running = store();
        let mut saved = running.clone();
        saved.settings.route_mode = "direct".into();
        let text = diagnostics(&saved, Some(&running));
        assert!(text.contains("RUNNING") && text.contains("SAVED"));
        assert!(text.contains("DNS timing"));
        assert!(!text.contains(&saved.secret));
        assert!(!text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)));
        saved.settings.language = "zh".into();
        let text = diagnostics(&saved, Some(&running));
        assert!(text.contains("当前生效") && text.contains("已保存"));
        assert!(!text.contains("BOUNDARIES") && !text.contains("RUNNING"));
    }
    #[test]
    fn credentials_redacted() {
        let c = redacted(&generate(&store()).unwrap());
        assert_eq!(c["outbounds"][2]["password"], "••••••");
    }
    #[test]
    fn legacy_direct_dns_dials_without_a_direct_detour() {
        let mut s = with_rules();
        s.settings.dns_policy = "legacy".into();
        s.settings.routing = "direct".into();
        let c = generate(&s).unwrap();
        assert!(c["dns"]["servers"][1].get("detour").is_none());
        assert_eq!(c["dns"]["servers"][1]["type"], "https");
        assert_eq!(c["dns"]["final"], "secure-dns");
        assert_eq!(s.settings.dns_policy, "legacy");
        s.settings.routing = "g-abc123".into();
        assert_eq!(
            generate(&s).unwrap()["dns"]["servers"][1]["detour"],
            "g-abc123"
        );
    }
    #[test]
    fn invalid_ports_rejected() {
        let mut s = Settings::default();
        s.api_port = s.port;
        assert!(validate(&s).is_err());
    }
    fn with_rules() -> Store {
        let mut s = store();
        s.proxy_groups.push(ProxyGroup {
            id: "abc123".into(),
            name: "Video".into(),
            kind: "selector".into(),
            members: vec![s.nodes[0].id.clone()],
            selected: Some(s.nodes[0].id.clone()),
        });
        s.rule_resources.push(crate::model::RuleResource {
            id: "abcd".into(),
            name: "Video sites".into(),
            native_document: None,
            source: "fixture".into(),
            format: "qx".into(),
            updated_at: 0,
            digest: "fixture".into(),
            input_count: 2,
            rules: vec![
                MatchRule {
                    kind: "domain_suffix".into(),
                    value: "example.com".into(),
                },
                MatchRule {
                    kind: "process_name".into(),
                    value: "curl".into(),
                },
            ],
            warnings: vec![],
        });
        s.rule_bindings.push(crate::model::RuleBinding {
            id: "123".into(),
            resource: "abcd".into(),
            target: "g-abc123".into(),
            enabled: true,
        });
        s
    }
    #[test]
    fn paired_dns_and_order_follow_bindings_without_process_projection() {
        let mut s = with_rules();
        s.rule_bindings.push(crate::model::RuleBinding {
            id: "456".into(),
            resource: "abcd".into(),
            target: "reject".into(),
            enabled: true,
        });
        let c = generate(&s).unwrap();
        assert_eq!(c["route"]["rules"][3]["outbound"], "g-abc123");
        assert_eq!(c["route"]["rules"][4]["action"], "reject");
        assert_eq!(c["dns"]["rules"][1]["server"], "dns-g-abc123");
        assert_eq!(c["dns"]["rules"][2]["action"], "reject");
        let sets = c["route"]["rule_set"].as_array().unwrap();
        assert_eq!(sets.len(), 3);
        assert_ne!(sets[1]["tag"], sets[2]["tag"]);
        assert_eq!(sets[1]["rules"].as_array().unwrap().len(), 1);
        assert!(sets[1]["rules"][0].get("process_name").is_none());
        s.rule_bindings[0].enabled = false;
        let c = generate(&s).unwrap();
        assert_eq!(c["route"]["rules"][3]["action"], "reject");
        assert_eq!(c["dns"]["rules"][1]["action"], "reject");
    }
    #[test]
    fn dangling_targets_and_stale_members_never_fall_back_silently() {
        let mut s = with_rules();
        s.proxy_groups.clear();
        assert!(generate(&s).is_err());
        let mut s = with_rules();
        s.proxy_groups[0].members.push("gone".into());
        assert!(generate(&s).is_err());
        let mut s = with_rules();
        s.proxy_groups[0].name = "PROXY".into();
        assert!(generate(&s).is_err());
    }
    #[test]
    fn legacy_settings_keep_dns_and_new_settings_use_paired() {
        let old: Settings =
            serde_json::from_value(json!({"dns":"9.9.9.9","routing":"direct"})).unwrap();
        assert_eq!(old.dns_policy, "legacy");
        assert_eq!(old.dns_strategy, "ipv4_only");
        assert_eq!(Settings::default().dns_policy, "paired");
        let mut s = store();
        s.settings = old;
        let c = generate(&s).unwrap();
        assert_eq!(c["dns"]["servers"][1]["server"], "9.9.9.9");
        assert!(c["dns"]["servers"][1].get("detour").is_none());
    }
}
