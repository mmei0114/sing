//! Turn live routing evidence into an editable native route rule.
use super::{
    editor, history, labels,
    modal::{Choice, Confirm, Picker},
    App,
};
use crate::{api, runtime::Action};
use serde_json::{json, Map, Value};

fn host(c: &api::Connection) -> String {
    history::host(c)
}

fn route(rule: &mut Map<String, Value>, target: &str) {
    if target == "reject" {
        rule.insert("action".into(), json!("reject"));
    } else {
        rule.insert("action".into(), json!("route"));
        rule.insert("outbound".into(), json!(target));
    }
}

fn insertion_position(app: &App) -> usize {
    crate::native::array(app.doc(), "/route/rules")
        .iter()
        .position(|r| {
            matches!(
                r["action"].as_str().unwrap_or("route"),
                "route" | "reject" | "bypass"
            )
        })
        .unwrap_or_else(|| crate::native::array(app.doc(), "/route/rules").len())
}

fn rule_for(c: &api::Connection, kind: &str) -> Result<Value, String> {
    let host = host(c);
    let process = c.process.as_ref();
    let mut rule = Map::new();
    match kind {
        "domain" if !c.domain.is_empty() => {
            rule.insert("domain".into(), json!([c.domain]));
        }
        "suffix" if !c.domain.is_empty() => {
            rule.insert("domain_suffix".into(), json!([c.domain]));
        }
        "regex" if !c.domain.is_empty() => {
            rule.insert(
                "domain_regex".into(),
                json!([format!("^{}$", regex::escape(&c.domain))]),
            );
        }
        "ip" if host.parse::<std::net::IpAddr>().is_ok() => {
            let suffix = if host.contains(':') { "/128" } else { "/32" };
            rule.insert("ip_cidr".into(), json!([format!("{host}{suffix}")]));
        }
        "process" => {
            let name = process
                .map(|p| p.name())
                .filter(|s| !s.is_empty())
                .ok_or("Process information is unavailable for this connection")?;
            rule.insert("process_name".into(), json!([name]));
        }
        "path" => {
            let path = process
                .map(|p| p.path.as_str())
                .filter(|s| !s.is_empty())
                .ok_or("Process path is unavailable for this connection")?;
            rule.insert("process_path".into(), json!([path]));
        }
        "process_domain" if !c.domain.is_empty() => {
            let name = process
                .map(|p| p.name())
                .filter(|s| !s.is_empty())
                .ok_or("Process information is unavailable for this connection")?;
            rule.insert("process_name".into(), json!([name]));
            rule.insert("domain".into(), json!([c.domain]));
        }
        _ => return Err("This connection does not contain the selected match evidence".into()),
    }
    Ok(Value::Object(rule))
}

pub fn from_connection(app: &mut App, c: api::Connection, prefer_process: bool) {
    if prefer_process && c.process.as_ref().is_none_or(|p| p.path.is_empty()) {
        return app.error("No executable identity for this traffic. f configures discovery; open Connections to create a domain/IP rule instead.");
    }
    let mut choices = vec![];
    if !c.domain.is_empty() {
        choices.extend([
            Choice::new("domain", format!("Only {}", c.domain), "exact domain"),
            Choice::new(
                "suffix",
                format!("{} and subdomains", c.domain),
                "domain_suffix",
            ),
            Choice::new(
                "regex",
                format!("Regex for {}", c.domain),
                "editable before saving",
            ),
        ]);
    } else if host(&c).parse::<std::net::IpAddr>().is_ok() {
        choices.push(Choice::new(
            "ip",
            format!("Only {}", host(&c)),
            "single-address CIDR",
        ));
    }
    if let Some(p) = c.process.as_ref().filter(|p| !p.path.is_empty()) {
        choices.push(Choice::new(
            "process",
            format!("Process name · {}", p.name()),
            "matches this executable name, not the whole app bundle",
        ));
        if !p.path.is_empty() {
            choices.push(Choice::new(
                "path",
                format!("Executable · {}", p.path),
                "exact process path",
            ));
        }
        if !c.domain.is_empty() {
            choices.push(Choice::new(
                "process_domain",
                format!("{} → {}", p.name(), c.domain),
                "both app and exact domain must match",
            ));
        }
    }
    if choices.is_empty() {
        return app
            .error("This connection has no domain, IP, or process evidence to turn into a rule");
    }
    let current = if prefer_process && choices.iter().any(|x| x.value == "path") {
        "path".to_string()
    } else {
        choices[0].value.clone()
    };
    app.push(Picker::single(
        "Create rule · what should match?",
        choices,
        &current,
        Box::new(move |app, picked| {
            let Some(kind) = picked.into_iter().next() else {
                return;
            };
            let Ok(mut rule) = rule_for(&c, &kind) else {
                return app.error("The selected evidence is no longer available");
            };
            let targets = labels::targets(&app.snap.store, app.doc(), true);
            if targets.is_empty() {
                return app.error("Create a proxy group or outbound before adding a rule");
            }
            let current = history::route_target(&c);
            app.push(Picker::single(
                "Create rule · send matches to",
                targets,
                &current,
                Box::new(move |app, picked| {
                    let Some(target) = picked.into_iter().next() else {
                        return;
                    };
                    if let Some(map) = rule.as_object_mut() {
                        route(map, &target);
                    }
                    // The shared editor is the final review: the user can broaden,
                    // narrow or combine the suggested fields before saving.
                    save_rule(app, rule);
                }),
            ));
        }),
    ));
}

// Only merge like-for-like match arrays. Adding a different field would mean
// AND in sing-box, not OR. Never rewrite a downloaded rule-set or mixed rule.
fn merge_field(existing: &Value, suggestion: &Value) -> Option<String> {
    let matcher = suggestion
        .as_object()?
        .keys()
        .find(|k| !["action", "outbound"].contains(&k.as_str()))?;
    if existing["action"] != suggestion["action"] || existing["outbound"] != suggestion["outbound"]
    {
        return None;
    }
    if !existing
        .as_object()?
        .keys()
        .all(|k| k == matcher || ["action", "outbound"].contains(&k.as_str()))
    {
        return None;
    }
    if !existing[matcher].is_array() && !existing[matcher].is_string() {
        return None;
    }
    if suggestion.as_object()?.len() != 2 + usize::from(suggestion.get("outbound").is_some()) {
        return None;
    }
    Some(matcher.clone())
}

fn save_rule(app: &mut App, rule: Value) {
    let mut choices = vec![Choice::new(
        "new",
        "New local rule",
        "Place before existing routing decisions; review before saving",
    )];
    for (i, existing) in crate::native::array(app.doc(), "/route/rules")
        .iter()
        .enumerate()
    {
        if merge_field(existing, &rule).is_some() {
            choices.push(Choice::new(
                i.to_string(),
                format!("Add to rule {}", i + 1),
                labels::matcher(&app.snap.store, existing),
            ));
        }
    }
    if choices.len() == 1 {
        return editor::create_at(app, "/route/rules", insertion_position(app), rule);
    }
    app.push(Picker::single(
        "Save routing correction",
        choices,
        "new",
        Box::new(move |app, picked| {
            let Some(id) = picked.first() else { return };
            if id == "new" {
                return editor::create_at(app, "/route/rules", insertion_position(app), rule);
            }
            let Ok(i) = id.parse::<usize>() else { return };
            let pointer = format!("/route/rules/{i}");
            let original = app.doc().pointer(&pointer).cloned();
            app.request(
                Action::ReadNative(pointer.clone()),
                Box::new(move |app, r| {
                    let Some(edit) = r.edit.filter(|_| r.ok) else {
                        return app.error(r.message);
                    };
                    if original.as_ref() != Some(&edit.value) {
                        return app.error(
                            "Rule changed. Reopen the correction to review its new position.",
                        );
                    }
                    let Some(field) = merge_field(&edit.value, &rule) else {
                        return app.error("Rule no longer accepts this match");
                    };
                    let mut value = edit.value;
                    let mut matches = value[&field]
                        .as_array()
                        .cloned()
                        .unwrap_or_else(|| vec![value[&field].clone()]);
                    for item in rule[&field].as_array().into_iter().flatten() {
                        if !matches.contains(item) {
                            matches.push(item.clone());
                        }
                    }
                    value[&field] = json!(matches);
                    app.push(editor::Editor::new(
                        super::schema::Object::RouteRule,
                        value,
                        editor::Target::Native {
                            pointer,
                            revision: edit.revision,
                        },
                        format!("Extend rule {} · review", i + 1),
                    ));
                }),
            );
        }),
    ));
}

/// Process discovery is optional when no process rule exists. Turn it on so
/// Activity can keep showing app names without forcing the user into Config.
pub fn enable_process_discovery(app: &mut App) {
    let explanation = super::identity::explanation(app);
    app.push(Confirm::new("Application discovery", format!("{explanation}\n\nEnable route.find_process in the saved draft? The core will look up owners of newly captured sockets. It cannot identify applications on another device or guarantee attribution for shared OS helpers.\n\nApplying requires a core restart; you will review it separately."), "Enable in draft", Box::new(save_process_discovery)));
}

fn save_process_discovery(app: &mut App) {
    app.request(
        Action::ReadNative("/route".into()),
        Box::new(|app, r| {
            let Some(mut edit) = r.edit.filter(|_| r.ok) else {
                return app.error(r.message);
            };
            if edit.value.is_null() {
                edit.value = json!({});
            }
            edit.value["find_process"] = json!(true);
            app.request(
                Action::WriteNative(edit),
                Box::new(|app, r| {
                    if r.ok {
                        app.toast("App discovery saved · apply, then open new connections");
                        super::flows::review_apply(app);
                    } else {
                        app.error(r.message);
                    }
                }),
            );
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection() -> api::Connection {
        api::Connection {
            domain: "cdn.example.com".into(),
            destination: "203.0.113.4:443".into(),
            process: Some(api::ProcessInfo {
                path: "/Applications/Browser.app/Contents/MacOS/Browser".into(),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn suggestions_use_native_match_fields() {
        assert_eq!(
            rule_for(&connection(), "domain").unwrap()["domain"],
            json!(["cdn.example.com"])
        );
        assert_eq!(
            rule_for(&connection(), "regex").unwrap()["domain_regex"],
            json!(["^cdn\\.example\\.com$"])
        );
        let both = rule_for(&connection(), "process_domain").unwrap();
        assert_eq!(both["process_name"], json!(["Browser"]));
        assert_eq!(both["domain"], json!(["cdn.example.com"]));
    }
    #[test]
    fn appending_preserves_or_semantics_and_never_mutates_mixed_rules() {
        let existing = json!({"domain":["one.example"],"action":"route","outbound":"proxy"});
        let suggested = json!({"domain":["two.example"],"action":"route","outbound":"proxy"});
        assert_eq!(merge_field(&existing, &suggested), Some("domain".into()));
        let mut mixed = existing.clone();
        mixed["process_name"] = json!(["Browser"]);
        assert!(merge_field(&mixed, &suggested).is_none());
        assert!(merge_field(&existing, &mixed).is_none());
        assert!(merge_field(
            &json!({"rule_set":["remote"],"action":"route","outbound":"proxy"}),
            &suggested
        )
        .is_none());
    }
}
