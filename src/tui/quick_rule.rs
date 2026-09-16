//! Turn live routing evidence into an editable native route rule.
use super::{
    editor, history, labels,
    modal::{Choice, Picker},
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
                "route" | "reject" | "bypass" | "hijack-dns"
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
        "ip" if !host.is_empty() => {
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
    } else if !host(&c).is_empty() {
        choices.push(Choice::new(
            "ip",
            format!("Only {}", host(&c)),
            "single-address CIDR",
        ));
    }
    if let Some(p) = &c.process {
        choices.push(Choice::new(
            "process",
            format!("App · {}", p.name()),
            "all traffic from this process name",
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
    let current = if prefer_process && choices.iter().any(|x| x.value == "process") {
        "process".to_string()
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
                    let at = insertion_position(app);
                    editor::create_at(app, "/route/rules", at, rule);
                }),
            ));
        }),
    ));
}

/// Process discovery is optional when no process rule exists. Turn it on so
/// Activity can keep showing app names without forcing the user into Config.
pub fn enable_process_discovery(app: &mut App) {
    app.request(
        Action::ReadNative("/route".into()),
        Box::new(|app, r| {
            let Some(mut edit) = r.edit.filter(|_| r.ok) else {
                return app.error(r.message);
            };
            edit.value["find_process"] = json!(true);
            app.request(
                Action::WriteNative(edit),
                Box::new(|app, r| {
                    if r.ok {
                        app.toast("App discovery enabled in draft · A applies");
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
}
