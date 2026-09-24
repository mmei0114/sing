//! First-run defaults, not a migration or a policy enforced on existing drafts.
use crate::model::Store;
use serde_json::{json, Value};

pub fn document(store: &Store) -> Value {
    json!({
        "log": {"level": "warn", "timestamp": true},
        "dns": {
            "servers": [
                {"type": "https", "tag": "dns-proxy", "server": "1.1.1.1",
                 "server_port": 443, "path": "/dns-query", "detour": "proxy",
                 "tls": {"enabled": true, "server_name": "cloudflare-dns.com"}},
                {"type": "https", "tag": "dns-bootstrap", "server": "223.5.5.5",
                 "server_port": 443, "path": "/dns-query",
                 "tls": {"enabled": true, "server_name": "dns.alidns.com"}},
                {"type": "local", "tag": "local"}
            ],
            "rules": [{"domain_suffix": ["local"], "action": "route", "server": "local"}],
            "final": "dns-proxy", "strategy": "ipv4_only",
            "reverse_mapping": true, "timeout": "10s"
        },
        "inbounds": [{"type": "mixed", "tag": "mixed-in", "listen": "127.0.0.1",
                      "listen_port": store.settings.port}],
        "outbounds": [
            {"type": "selector", "tag": "proxy", "outbounds": []},
            {"type": "direct", "tag": "direct", "domain_resolver": "dns-proxy"}
        ],
        "route": {
            "rules": [
                {"action": "sniff"},
                {"type": "logical", "mode": "or", "rules": [
                    {"protocol": "dns"}, {"port": 53}
                ], "action": "hijack-dns"},
                {"ip_is_private": true, "action": "route", "outbound": "direct"}
            ],
            "rule_set": [], "final": "proxy", "auto_detect_interface": true,
            "find_process": true, "default_domain_resolver": "dns-bootstrap"
        },
        "services": [{"type": "api", "tag": "management", "listen": "127.0.0.1",
                      "listen_port": store.settings.api_port, "secret": store.secret}]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config, model::Node, native};

    #[test]
    fn first_run_is_private_editable_and_requires_nodes_before_start() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::load(dir.path()).unwrap();
        let doc = s.native.as_ref().unwrap();
        assert_eq!(s.schema, 2);
        assert!(s.nodes.is_empty() && s.subscriptions.is_empty());
        assert_eq!(doc["dns"]["final"], "dns-proxy");
        assert_eq!(doc["dns"]["servers"][0]["detour"], "proxy");
        assert_eq!(doc["route"]["default_domain_resolver"], "dns-bootstrap");
        assert_eq!(doc["outbounds"][1]["domain_resolver"], "dns-proxy");
        assert!(!native::uses_tun(&s));
        assert_eq!(s.settings.mode, "port");
        assert!(config::generate(&s)
            .unwrap_err()
            .to_string()
            .contains("Import"));
        assert!(!dir.path().join("runtime.json").exists());
        assert_ne!(s.secret, Store::load(dir.path()).unwrap().secret);
    }

    #[test]
    fn import_refresh_and_rules_preserve_dns_and_local_edits() {
        let dir = tempfile::tempdir().unwrap();
        let before = Store::load(dir.path()).unwrap();
        let mut s = before.clone();
        s.nodes.push(Node {
            id: "example".into(),
            name: "Example".into(),
            provider: "fixture".into(),
            outbound: json!({"type":"trojan", "server":"node.example.invalid",
                             "server_port":443, "password":"fixture"}),
            favorite: false,
        });
        native::reconcile(&before, &mut s).unwrap();
        assert_eq!(
            s.native.as_ref().unwrap()["outbounds"][0]["outbounds"],
            json!(["n-example"])
        );
        config::generate(&s).unwrap(); // Includes bootstrap dependency cycle checks.
        let dns = s.native.as_ref().unwrap()["dns"].clone();
        let old = s.clone();
        s.nodes[0].outbound["server_port"] = json!(8443);
        native::reconcile(&old, &mut s).unwrap();
        assert_eq!(s.native.as_ref().unwrap()["dns"], dns);
        s.native.as_mut().unwrap()["dns"]["timeout"] = json!("7s");
        s.native.as_mut().unwrap()["route"]["rules"]
            .as_array_mut()
            .unwrap()
            .push(json!({"domain_suffix":["example.org"], "outbound":"direct"}));
        s.save(dir.path()).unwrap();
        let saved = Store::load(dir.path()).unwrap();
        assert_eq!(saved.native, s.native);
        assert_eq!(saved.secret, s.secret);
        for mode in ["rule", "global", "direct"] {
            s.settings.route_mode = mode.into();
            assert_eq!(
                config::generate(&s).unwrap()["dns"],
                saved.native.as_ref().unwrap()["dns"]
            );
        }
    }

    #[test]
    fn existing_legacy_profile_is_not_silently_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::new().unwrap();
        s.save(dir.path()).unwrap();
        let saved = Store::load(dir.path()).unwrap();
        assert!(saved.native.is_none());
        assert_eq!(saved.secret, s.secret);
    }
}
