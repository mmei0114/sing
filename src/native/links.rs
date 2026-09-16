//! Typed native tag references. Unknown fields remain untouched; this is not a
//! replacement for validation by the selected sing-box core.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObjectKind {
    Outbound,
    Inbound,
    Dns,
    RuleSet,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub kind: ObjectKind,
    pub tag: String,
    pub path: String,
}

fn values(value: &Value) -> Vec<&str> {
    match value {
        Value::String(s) if !s.is_empty() => vec![s],
        Value::Array(a) => a
            .iter()
            .filter_map(Value::as_str)
            .filter(|s| !s.is_empty())
            .collect(),
        _ => vec![],
    }
}

pub fn definitions(doc: &Value) -> Vec<Link> {
    use ObjectKind::*;
    let mut result = vec![];
    for (path, kind) in [
        ("/outbounds", Outbound),
        ("/endpoints", Outbound),
        ("/inbounds", Inbound),
        ("/endpoints", Inbound),
        ("/dns/servers", Dns),
        ("/route/rule_set", RuleSet),
    ] {
        for (i, v) in array(doc, path).iter().enumerate() {
            for tag in values(&v["tag"]) {
                result.push(Link {
                    kind,
                    tag: tag.into(),
                    path: format!("{path}/{i}"),
                });
            }
        }
    }
    result
}

pub fn links(doc: &Value) -> Vec<Link> {
    use ObjectKind::*;
    fn add(result: &mut Vec<Link>, kind: ObjectKind, v: &Value, path: &str) {
        for tag in values(v) {
            result.push(Link {
                kind,
                tag: tag.into(),
                path: path.into(),
            });
        }
    }
    fn walk(v: &Value, path: &str, result: &mut Vec<Link>) {
        match v {
            Value::Array(a) => {
                for (i, v) in a.iter().enumerate() {
                    walk(v, &format!("{path}/{i}"), result);
                }
            }
            Value::Object(m) => {
                let rule = path.starts_with("/route/rules/") || path.starts_with("/dns/rules/");
                for (key, value) in m {
                    let next = format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"));
                    let kind = match key.as_str() {
                        "final" if path == "/route" => Some(Outbound),
                        "final" if path == "/dns" => Some(Dns),
                        "outbounds" | "default"
                            if path.starts_with("/outbounds/")
                                && ["selector", "urltest"]
                                    .contains(&v["type"].as_str().unwrap_or("")) =>
                        {
                            Some(Outbound)
                        }
                        "outbound" | "preferred_by" if rule => Some(Outbound),
                        "inbound" if rule => Some(Inbound),
                        "rule_set" if rule => Some(RuleSet),
                        "route_address_set" | "route_exclude_address_set"
                            if path.starts_with("/inbounds/") && v["type"] == "tun" =>
                        {
                            Some(RuleSet)
                        }
                        "server"
                            if path.ends_with("/domain_resolver")
                                || path.ends_with("/default_domain_resolver")
                                || (rule
                                    && (path.starts_with("/dns/") || v["action"] == "resolve")) =>
                        {
                            Some(Dns)
                        }
                        "domain_resolver" | "default_domain_resolver" if value.is_string() => {
                            Some(Dns)
                        }
                        "detour"
                            if path.starts_with("/inbounds/") && path.split('/').count() == 3 =>
                        {
                            Some(Inbound)
                        }
                        "detour" | "download_detour" => Some(Outbound),
                        _ => None,
                    };
                    if let Some(kind) = kind {
                        add(result, kind, value, &next);
                    }
                    walk(value, &next, result);
                }
            }
            _ => {}
        }
    }
    let mut result = vec![];
    // Do not interpret arbitrary top-level extension objects as native fields.
    for root in [
        "outbounds",
        "endpoints",
        "inbounds",
        "dns",
        "route",
        "ntp",
        "services",
        "http_clients",
        "certificate",
    ] {
        walk(&doc[root], &format!("/{root}"), &mut result);
    }
    result
}

/// Only reject dangling references introduced by removing/renaming definitions.
/// Existing incomplete drafts and atomic refactorings remain editable.
pub fn check_removals(before: &Value, after: &Value) -> Result<()> {
    let present: HashSet<_> = definitions(after)
        .into_iter()
        .map(|l| (l.kind, l.tag))
        .collect();
    let remaining = links(after);
    for removed in definitions(before)
        .into_iter()
        .filter(|l| !present.contains(&(l.kind, l.tag.clone())))
    {
        let used: Vec<_> = remaining
            .iter()
            .filter(|l| l.kind == removed.kind && l.tag == removed.tag)
            .map(|l| l.path.as_str())
            .collect();
        ensure!(used.is_empty(), "Cannot remove or rename {}: still referenced at {}. Open References to edit these uses first, or update the native document atomically.", removed.tag, used.join(", "));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn namespaces_nested_rules_and_native_fields_are_distinct() {
        let doc = json!({"outbounds":[{"type":"selector","tag":"same","outbounds":["node"],"default":"node"},{"type":"trojan","tag":"node","server":"same","domain_resolver":{"server":"same"}}],"inbounds":[{"type":"tun","tag":"tun","route_address_set":["video"]}],"dns":{"servers":[{"type":"local","tag":"same"}],"final":"same","rules":[{"type":"logical","rules":[{"rule_set":"video"}],"server":"same"}]},"route":{"final":"same","rules":[{"inbound":["tun"],"rule_set":["video"],"outbound":"same"}],"rule_set":[{"type":"remote","tag":["video","music"],"http_client":{"detour":"same"}}]}});
        let refs = links(&doc);
        assert!(refs.contains(&Link {
            kind: ObjectKind::Dns,
            tag: "same".into(),
            path: "/outbounds/1/domain_resolver/server".into()
        }));
        assert!(!refs.iter().any(|l| l.path == "/outbounds/1/server"));
        assert_eq!(
            refs.iter()
                .filter(|l| l.tag == "video" && l.kind == ObjectKind::RuleSet)
                .count(),
            3
        );
        let mut next = doc.clone();
        next["outbounds"].as_array_mut().unwrap().remove(0);
        let error = check_removals(&doc, &next).unwrap_err().to_string();
        assert!(error.contains("/route/final") && !error.contains("/dns/final"));
        let mut next = doc.clone();
        next["route"]["rule_set"] = json!([]);
        let error = check_removals(&doc, &next).unwrap_err().to_string();
        assert!(
            error.contains("/dns/rules/0/rules/0/rule_set")
                && error.contains("/inbounds/0/route_address_set")
        );
        assert!(definitions(&doc).iter().any(|d| d.tag == "music"));
    }
    #[test]
    fn atomic_refactors_and_unrelated_incomplete_drafts_are_allowed() {
        let before = json!({"outbounds":[{"tag":"old","type":"direct"}],"route":{"final":"old","rules":[{"outbound":"already-missing"}]}});
        let mut next = before.clone();
        next["outbounds"][0]["tag"] = json!("new");
        assert!(check_removals(&before, &next).is_err());
        next["route"]["final"] = json!("new");
        check_removals(&before, &next).unwrap();
        next["outbounds"] = json!([]);
        next["route"].as_object_mut().unwrap().remove("final");
        check_removals(&before, &next).unwrap();
    }
}
