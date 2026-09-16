use super::*;

fn label(store: &Store, tag: &str) -> String {
    crate::model::clean(
        store
            .display_names
            .get(tag)
            .map(String::as_str)
            .or_else(|| {
                store
                    .nodes
                    .iter()
                    .find(|n| n.tag() == tag)
                    .map(|n| n.name.as_str())
            })
            .unwrap_or(tag),
    )
}
fn rule(store: &Store, v: &Value) -> String {
    let matches = if let Some(sets) = v.get("rule_set") {
        if let Some(a) = sets.as_array() {
            a.iter()
                .filter_map(Value::as_str)
                .map(|s| label(store, s))
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            label(store, sets.as_str().unwrap_or(""))
        }
    } else {
        v.as_object()
            .map(|m| {
                m.keys()
                    .filter(|k| !["action", "outbound", "server"].contains(&k.as_str()))
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "all traffic".into())
    };
    let target = v["outbound"]
        .as_str()
        .or_else(|| v["server"].as_str())
        .unwrap_or("");
    format!(
        "{matches} → {} {}",
        v["action"].as_str().unwrap_or("route"),
        label(store, target)
    )
}
pub fn summary(store: &Store, before: &Value, after: &Value) -> String {
    let mut lines = vec!["Configuration changes".to_string()];
    for (path, name) in [
        ("/outbounds", "Outbounds / groups"),
        ("/inbounds", "Inbounds"),
        ("/endpoints", "Endpoints"),
        ("/route/rule_set", "Rule sets"),
        ("/dns/servers", "DNS servers"),
    ] {
        let old = array(before, path);
        let new = array(after, path);
        let added = new
            .iter()
            .filter(|n| !old.iter().any(|o| o["tag"] == n["tag"]))
            .count();
        let removed = old
            .iter()
            .filter(|o| !new.iter().any(|n| o["tag"] == n["tag"]))
            .count();
        let changed = new
            .iter()
            .filter(|n| old.iter().any(|o| o["tag"] == n["tag"] && o != *n))
            .count();
        lines.push(format!(
            "  {name}: +{added} / -{removed} / {changed} edited{}",
            if old != new && added + removed + changed == 0 {
                " · order changed"
            } else {
                ""
            }
        ));
        for v in new.iter().filter(|n| !old.contains(n)).take(8) {
            lines.push(format!(
                "    {} ({})",
                label(store, tag(v)),
                v["type"].as_str().unwrap_or("native")
            ));
        }
    }
    for (path, name) in [("/route/rules", "Routing"), ("/dns/rules", "DNS rules")] {
        let old = array(before, path);
        let new = array(after, path);
        if old == new {
            lines.push(format!("  {name}: unchanged"));
        } else {
            lines.push(format!(
                "  {name}: {} → {} ordered rules",
                old.len(),
                new.len()
            ));
            for (i, v) in new
                .iter()
                .enumerate()
                .filter(|(i, v)| old.get(*i) != Some(*v))
                .take(10)
            {
                lines.push(format!("    {}. {}", i + 1, rule(store, v)));
            }
        }
    }
    for (path, name) in [
        ("/route/final", "Default route"),
        ("/dns/final", "Final DNS"),
    ] {
        if before.pointer(path) != after.pointer(path) {
            lines.push(format!(
                "  {name}: {} → {}",
                label(
                    store,
                    before
                        .pointer(path)
                        .and_then(Value::as_str)
                        .unwrap_or("implicit")
                ),
                label(
                    store,
                    after
                        .pointer(path)
                        .and_then(Value::as_str)
                        .unwrap_or("implicit")
                )
            ));
        }
    }
    lines.push(
        if before.get("dns") == after.get("dns") {
            "DNS unchanged."
        } else {
            "DNS changed; inspect Native Diff for resolver, rule and dial details."
        }
        .into(),
    );
    lines.push(
        "Native Diff includes all changed paths; summaries above may shorten large lists.".into(),
    );
    lines.join("\n")
}

/// Compare raw values first, then display redacted values, so a secret-only
/// change is visible without revealing either secret.
pub fn detailed(before: &Value, after: &Value) -> String {
    fn walk(a: Option<&Value>, b: Option<&Value>, path: &str, out: &mut Vec<String>) {
        if a == b {
            return;
        }
        if let (Some(a), Some(b)) = (a.and_then(Value::as_object), b.and_then(Value::as_object)) {
            for key in a
                .keys()
                .chain(b.keys())
                .collect::<std::collections::BTreeSet<_>>()
            {
                walk(
                    a.get(key),
                    b.get(key),
                    &format!("{path}/{}", key.replace('~', "~0").replace('/', "~1")),
                    out,
                );
            }
        } else if let (Some(a), Some(b)) =
            (a.and_then(Value::as_array), b.and_then(Value::as_array))
        {
            for i in 0..a.len().max(b.len()) {
                walk(a.get(i), b.get(i), &format!("{path}/{i}"), out);
            }
        } else {
            out.push(path.into());
        }
    }
    // Diff values must not use the 512-character label sanitizer: changes near
    // the end of a long value would disappear. Escape terminal controls instead.
    fn visible(s: &str) -> String {
        s.chars()
            .map(|c| {
                if c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
                {
                    format!("\\u{:04x}", c as u32)
                } else {
                    c.to_string()
                }
            })
            .collect()
    }
    fn value(v: Option<&Value>) -> String {
        v.map(|v| visible(&v.to_string()))
            .unwrap_or_else(|| "<absent>".into())
    }
    let mut paths = vec![];
    walk(Some(before), Some(after), "", &mut paths);
    let a = config::redacted(before);
    let b = config::redacted(after);
    let mut result=String::from("Native Diff · known credentials redacted\nJSON pointers follow effective runtime configuration. <absent> means no field.\n\n");
    for path in paths {
        let old = a.pointer(&path);
        let new = b.pointer(&path);
        result.push_str(&format!(
            "{}\n  - {}\n  + {}{}\n",
            visible(&path),
            value(old),
            value(new),
            if old == new {
                " (redacted value changed)"
            } else {
                ""
            }
        ));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn semantic_summary_and_diff_agree_without_revealing_secrets() {
        let mut s = Store::new().unwrap();
        s.display_names.insert("media".into(), "Media".into());
        let a = json!({"outbounds":[{"type":"trojan","tag":"node","password":"old-secret"}],"dns":{"final":"dns"},"route":{"rules":[]}});
        let b = json!({"outbounds":[{"type":"trojan","tag":"node","password":"new-secret"}],"dns":{"final":"dns"},"route":{"rules":[{"rule_set":["video"],"action":"route","outbound":"media"}]}});
        let summary = summary(&s, &a, &b);
        let details = detailed(&a, &b);
        assert!(summary.contains("video → route Media") && summary.contains("DNS unchanged"));
        assert!(
            details.contains("/outbounds/0/password") && details.contains("redacted value changed")
        );
        assert!(!details.contains("old-secret") && !details.contains("new-secret"));
        let escaped = detailed(&json!({"a/b":0}), &json!({"a/b":1}));
        assert!(escaped.contains("/a~1b"));
    }

    #[test]
    fn diff_distinguishes_absent_null_and_keeps_long_values() {
        let a = json!({"removed":null, "items":[null], "long":"a".repeat(600)});
        let b = json!({"added":null, "items":[], "long":format!("{}END", "a".repeat(600)), "bidi":"\u{202e}"});
        let diff = detailed(&a, &b);
        assert!(diff.contains("/removed\n  - null\n  + <absent>"));
        assert!(diff.contains("/added\n  - <absent>\n  + null"));
        assert!(diff.contains("/items/0\n  - null\n  + <absent>"));
        assert!(diff.contains(&format!("{}END", "a".repeat(600))));
        assert!(diff.contains("\\u202e") && !diff.contains('\u{202e}'));
    }
}
