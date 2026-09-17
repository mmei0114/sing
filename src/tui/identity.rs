//! Identity comes from the captured socket's owner, never from host guesses.
use super::App;
use crate::api::Connection;
use serde_json::Value;

pub fn lookup_requested(doc: &Value) -> bool {
    fn matches(v: &Value) -> bool {
        match v {
            Value::Object(o) => o.iter().any(|(k, v)| {
                ([
                    "process_name",
                    "process_path",
                    "process_path_regex",
                    "package_name",
                    "user",
                    "user_id",
                ]
                .contains(&k.as_str())
                    && !v.is_null()
                    && v.as_array().is_none_or(|a| !a.is_empty()))
                    || matches(v)
            }),
            Value::Array(a) => a.iter().any(matches),
            _ => false,
        }
    }
    doc.pointer("/route/find_process") == Some(&Value::Bool(true))
        || doc.pointer("/route/rules").is_some_and(matches)
        || doc.pointer("/route/rule_set").is_some_and(matches)
}

/// Bundle label is display-only; exact executable paths remain the rule key.
pub fn display(c: &Connection) -> String {
    let Some(p) = &c.process else {
        return String::new();
    };
    if let Some((prefix, _)) = p.path.split_once(".app/Contents/") {
        return crate::model::clean(prefix.rsplit('/').next().unwrap_or(prefix));
    }
    if !p.name().is_empty() {
        return crate::model::clean(p.name());
    }
    if let Some(package) = p.package_names.first().filter(|s| !s.is_empty()) {
        return crate::model::clean(package);
    }
    if p.pid != 0 {
        return format!("PID {}", p.pid);
    }
    String::new()
}

/// Do not merge different executables merely because they have the same name.
pub fn key(c: &Connection) -> String {
    let Some(p) = &c.process else {
        return String::new();
    };
    if !p.path.is_empty() {
        return p.path.clone();
    }
    if !p.package_names.is_empty() {
        return format!("packages:{}", p.package_names.join(","));
    }
    // An unidentified PID may be reused. Keep it connection-local.
    if p.pid != 0 {
        return format!("pid:{}:{}", p.pid, c.id);
    }
    String::new()
}

pub fn explanation(app: &App) -> &'static str {
    if !lookup_requested(app.doc()) {
        "Process lookup is off in the draft. Enable it with f in Activity, then review and apply."
    } else if app.snap.dirty {
        "The draft requests process lookup. Apply changes, then create a new connection; existing connections are not relabeled."
    } else if !app.snap.connected {
        "Process lookup is configured. Start the core and generate traffic to observe applications."
    } else {
        "The core did not report a socket owner. Remote clients, short-lived sockets, shared helpers or OS permissions can limit attribution. Check Activity logs; TUN is not a guarantee of app identity."
    }
}

pub fn missing_label(app: &App) -> &'static str {
    if !lookup_requested(app.doc()) {
        "Lookup off"
    } else if app.snap.dirty {
        "Check pending draft"
    } else {
        "Not reported by core"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ProcessInfo;
    use serde_json::json;
    #[test]
    fn bundle_label_does_not_replace_executable_identity() {
        let c = Connection {
            process: Some(ProcessInfo {
                path:
                    "/Applications/Browser.app/Contents/Frameworks/Helper.app/Contents/MacOS/Helper"
                        .into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(display(&c), "Browser");
        assert!(key(&c).ends_with("/Helper"));
        assert_eq!(c.process.unwrap().name(), "Helper");
    }
    #[test]
    fn finds_nested_process_rules_and_explicit_flag() {
        assert!(lookup_requested(
            &json!({"route":{"rules":[{"type":"logical","rules":[{"process_name":["curl"]}]}]}})
        ));
        assert!(lookup_requested(&json!({"route":{"find_process":true}})));
        assert!(!lookup_requested(
            &json!({"route":{"rules":[{"domain":["example.com"]}]}})
        ));
    }
}
