//! Local, deliberately bounded conversion of remote classification lists.
//! Unknown predicates are rejected as whole rules, never stripped from a rule.
use crate::model::{clean, MatchRule};
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashSet};

pub const LIMIT: usize = 8 * 1024 * 1024;
const MAX_RULES: usize = 50_000;
#[derive(Debug)]
pub struct Parsed {
    pub rules: Vec<MatchRule>,
    pub warnings: Vec<String>,
    pub policies: Vec<String>,
    pub input_count: usize,
    pub format: String,
}
/// Preserve native headless rules exactly; validity is checked by the selected
/// core on Apply, not approximated by the external-list converter.
pub fn native_document(text: &str, hint: &str) -> Result<Option<Value>> {
    ensure!(text.len() <= LIMIT, "Rule resource exceeds 8 MiB");
    let parsed = serde_json::from_str::<Value>(text.trim().trim_start_matches('\u{feff}'));
    let Ok(doc) = parsed else {
        let yaml = serde_yaml::from_str::<Value>(text).ok();
        ensure!(
            !yaml.is_some_and(|d| d.get("version").is_some() && d.get("rules").is_some()),
            "Native rule-sets must be JSON; no native conditions were converted"
        );
        ensure!(
            hint != "native",
            "Expected native JSON; use Add Native Rule Set for binary SRS"
        );
        return Ok(None);
    };
    if doc.get("version").is_none() || doc.get("rules").is_none() {
        ensure!(
            hint != "native",
            "Expected a standalone native rule-set, not a full configuration"
        );
        return Ok(None);
    }
    ensure!(
        hint == "auto" || hint == "native",
        "Native rule-set conflicts with selected format"
    );
    ensure!(
        doc.as_object()
            .is_some_and(|m| m.keys().all(|k| ["version", "rules"].contains(&k.as_str()))),
        "Expected a standalone native rule-set, not a full configuration"
    );
    ensure!(
        doc["version"]
            .as_u64()
            .is_some_and(|v| (1..=5).contains(&v)),
        "Unsupported native rule-set version"
    );
    let rules = doc["rules"]
        .as_array()
        .context("Native rules must be a list")?;
    ensure!(
        !rules.is_empty() && rules.len() <= MAX_RULES,
        "Native rule-set must contain 1–50000 rules"
    );
    ensure!(
        rules.iter().all(Value::is_object),
        "Each native rule must be an object"
    );
    Ok(Some(doc))
}
pub fn validate_match(rule: &MatchRule) -> Result<()> {
    let v = &rule.value;
    ensure!(
        !v.is_empty() && v.len() <= 4096 && !v.chars().any(char::is_control),
        "Invalid or oversized match value"
    );
    match rule.kind.as_str() {
        "domain" | "domain_suffix" => {
            let domain = if rule.kind == "domain_suffix" {
                v.trim_start_matches('.')
            } else {
                v
            };
            ensure!(
                !domain.is_empty()
                    && domain.len() <= 253
                    && domain.split('.').all(|label| !label.is_empty()
                        && label.len() <= 63
                        && label
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')),
                "Use an ASCII/punycode hostname, without wildcard, URL or port"
            );
        }
        "domain_keyword" => ensure!(
            !v.chars().any(char::is_whitespace) && !v.contains(['*', '?', '/', ':']),
            "Unsupported domain keyword"
        ),
        "domain_regex" => {
            regex::Regex::new(v).context("Unsupported domain regex")?;
        }
        "ip_cidr" => {
            let (ip, prefix) = v.split_once('/').context("IP rule requires CIDR prefix")?;
            let ip: std::net::IpAddr = ip.parse().context("Invalid IP address")?;
            let prefix: u8 = prefix.parse().context("Invalid IP prefix")?;
            ensure!(
                prefix <= if ip.is_ipv4() { 32 } else { 128 },
                "Invalid CIDR prefix"
            );
        }
        "process_name" | "process_path" => {}
        _ => bail!("Unsupported native predicate: {}", clean(&rule.kind)),
    }
    Ok(())
}
fn make(kind: &str, value: &str) -> Result<MatchRule> {
    let r = MatchRule {
        kind: kind.into(),
        value: if kind.starts_with("domain") && kind != "domain_regex" {
            value.to_ascii_lowercase()
        } else {
            value.into()
        },
    };
    validate_match(&r)?;
    Ok(r)
}
fn text_rule(line: &str, format: &str) -> Result<(MatchRule, Option<String>)> {
    let columns: Vec<_> = line.split(',').map(str::trim).collect();
    if columns.len() == 1 {
        ensure!(format != "qx" && format != "clash", "Expected a typed rule");
        if format == "ipcidr" || line.contains('/') {
            return Ok((make("ip_cidr", line)?, None));
        }
        if let Some(suffix) = line.strip_prefix("+.") {
            return Ok((make("domain_suffix", suffix)?, None));
        }
        // Wildcard dialects differ. Do not silently replace glob semantics.
        ensure!(
            !line.contains(['*', '?']) && !line.starts_with('.'),
            "Ambiguous wildcard; use native domain_regex or an explicit DOMAIN-SUFFIX rule"
        );
        return Ok((make("domain", line)?, None));
    }
    ensure!(
        columns.len() == 2 || columns.len() == 3,
        "Unsupported rule options (including no-resolve); rule was not weakened"
    );
    ensure!(
        format != "domain" && format != "ipcidr",
        "Typed rules conflict with the selected format"
    );
    let kind = match columns[0].to_ascii_uppercase().as_str() {
        "HOST" | "DOMAIN" => "domain",
        "HOST-SUFFIX" | "DOMAIN-SUFFIX" => "domain_suffix",
        "HOST-KEYWORD" | "DOMAIN-KEYWORD" => "domain_keyword",
        "DOMAIN-REGEX" => "domain_regex",
        "IP-CIDR" | "IP6-CIDR" | "IP-CIDR6" => "ip_cidr",
        "PROCESS-NAME" => "process_name",
        "PROCESS-PATH" => "process_path",
        "USER-AGENT" => bail!("USER-AGENT has no equivalent native routing predicate"),
        "HOST-WILDCARD" => {
            bail!("HOST-WILDCARD needs dialect-specific semantics; not converted in 0.3")
        }
        "GEOIP" | "GEOSITE" => {
            bail!("Geographic rule requires a separate verified rule-set dependency")
        }
        "FINAL" | "MATCH" => {
            bail!("Global catch-all is not a classification rule; configure Default route instead")
        }
        _ => bail!("Unsupported rule type: {}", clean(columns[0])),
    };
    let policy = columns.get(2).map(|s| clean(s));
    ensure!(
        !policy
            .as_deref()
            .is_some_and(|p| p.eq_ignore_ascii_case("no-resolve") || p.contains('=')),
        "Rule option cannot be treated as a policy name"
    );
    Ok((make(kind, columns[1])?, policy))
}
fn native_rule(v: &Value) -> Result<Vec<MatchRule>> {
    let obj = v.as_object().context("Expected a native rule object")?;
    ensure!(!obj.is_empty(), "Empty native rule would match everything");
    // Domain and destination-IP fields have OR semantics. Process fields are
    // ANDed with those: only standalone process predicates can be flattened.
    ensure!(
        !obj.keys().any(|k| k.starts_with("process_")) || obj.len() == 1,
        "Compound process rules cannot be flattened safely"
    );
    let mut out = vec![];
    for (kind, values) in obj {
        let values = match values {
            Value::String(s) => vec![s.as_str()],
            Value::Array(a) => a
                .iter()
                .map(|v| v.as_str().context("Match values must be strings"))
                .collect::<Result<Vec<_>>>()?,
            _ => bail!("Unsupported native predicate/value: {kind}"),
        };
        ensure!(!values.is_empty(), "Empty predicate list");
        for value in values {
            out.push(make(kind, value)?);
        }
    }
    Ok(out)
}
pub fn parse(text: &str, hint: &str) -> Result<Parsed> {
    ensure!(text.len() <= LIMIT, "Rule resource exceeds 8 MiB");
    ensure!(
        ["auto", "qx", "clash", "domain", "ipcidr", "native"].contains(&hint),
        "Unknown rule format"
    );
    let text = text.trim().trim_start_matches('\u{feff}').trim();
    ensure!(
        !text.is_empty() && !text.starts_with('<'),
        "Empty/HTML response; previous rules kept"
    );
    let mut parsed = Parsed {
        rules: vec![],
        warnings: vec![],
        policies: vec![],
        input_count: 0,
        format: hint.into(),
    };
    let mut seen = HashSet::new();
    let mut policies = BTreeSet::new();
    let object = serde_json::from_str::<Value>(text)
        .ok()
        .or_else(|| serde_yaml::from_str::<Value>(text).ok());
    let mut items: Vec<(String, Result<Vec<MatchRule>>)> = vec![];
    if object
        .as_ref()
        .is_some_and(|o| o.get("version").is_some() && o.get("rules").is_some())
    {
        ensure!(
            hint == "auto" || hint == "native",
            "Native rule-set conflicts with selected format"
        );
        let o = object.as_ref().unwrap();
        ensure!(
            o.as_object()
                .unwrap()
                .keys()
                .all(|k| ["version", "rules"].contains(&k.as_str())),
            "Expected a standalone native rule-set, not a full configuration"
        );
        ensure!(
            o["version"].as_u64().is_some_and(|n| (1..=5).contains(&n)),
            "Unsupported native rule-set version"
        );
        let rules = o["rules"]
            .as_array()
            .context("Native rules must be an array")?;
        ensure!(rules.len() <= MAX_RULES, "Too many rules");
        parsed.format = "native".into();
        for (i, rule) in rules.iter().enumerate() {
            items.push((format!("Rule {}", i + 1), native_rule(rule)));
        }
    } else {
        ensure!(
            hint != "native",
            "Expected native JSON rule-set; binary SRS is not supported by this importer yet"
        );
        let lines: Vec<(usize, String)> = if let Some(o) = object.as_ref().filter(|o| o.is_object())
        {
            ensure!(
                o.as_object().unwrap().keys().all(|k| k == "payload"),
                "Expected a classification payload, not a full YAML configuration"
            );
            o["payload"]
                .as_array()
                .context("Missing rule-provider payload")?
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    Ok((
                        i + 1,
                        v.as_str()
                            .context("Payload entries must be strings")?
                            .trim()
                            .to_string(),
                    ))
                })
                .collect::<Result<_>>()?
        } else {
            text.lines()
                .enumerate()
                .filter_map(|(i, l)| {
                    let l = l.trim();
                    (!l.is_empty() && !l.starts_with(['#', ';']) && !l.starts_with("//"))
                        .then(|| (i + 1, l.to_string()))
                })
                .collect()
        };
        ensure!(lines.len() <= MAX_RULES, "Too many rules");
        if hint == "auto" {
            parsed.format = if lines.iter().any(|(_, l)| !l.contains(',')) {
                // Keep auto mode when typed and untyped entries coexist, so
                // refreshing the same resource cannot change its interpretation.
                "auto"
            } else if lines
                .iter()
                .any(|(_, l)| l.to_ascii_uppercase().starts_with("HOST"))
            {
                "qx"
            } else if lines.iter().any(|(_, l)| l.contains(',')) {
                "clash"
            } else {
                "auto"
            }
            .into();
        }
        for (i, line) in lines {
            let result = text_rule(&line, hint).map(|(r, p)| {
                if let Some(p) = p {
                    policies.insert(p);
                }
                vec![r]
            });
            items.push((format!("Line {i}"), result));
        }
    }
    parsed.input_count = items.len();
    let mut warning_count = 0;
    for (label, result) in items {
        match result {
            Ok(rules) => {
                for rule in rules {
                    if seen.insert(rule.clone()) {
                        parsed.rules.push(rule);
                    }
                    ensure!(
                        parsed.rules.len() <= MAX_RULES,
                        "Too many expanded predicates"
                    );
                }
            }
            Err(e) => {
                warning_count += 1;
                if parsed.warnings.len() < 200 {
                    parsed
                        .warnings
                        .push(format!("{label}: {}", clean(&e.to_string())));
                }
            }
        }
    }
    if warning_count > 200 {
        parsed.warnings.push(format!(
            "{} further unsupported entries",
            warning_count - 200
        ));
    }
    parsed.policies = policies.into_iter().collect();
    ensure!(
        !parsed.rules.is_empty(),
        "No supported rules; previous resource kept. {}",
        parsed
            .warnings
            .first()
            .map(String::as_str)
            .unwrap_or("Check the format")
    );
    Ok(parsed)
}
pub fn native_rules(rules: &[MatchRule]) -> Vec<Value> {
    // Keep separate rules to preserve OR across all imported entries, including
    // process predicates. The core compiles the inline collection for matching.
    rules
        .iter()
        .map(|r| {
            let mut v = json!({});
            v[&r.kind] = json!([r.value]);
            v
        })
        .collect()
}
pub fn domain_rules(rules: &[MatchRule]) -> Vec<Value> {
    native_rules(
        &rules
            .iter()
            .filter(|r| r.kind.starts_with("domain"))
            .cloned()
            .collect::<Vec<_>>(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_document_is_not_a_lossy_converter_or_full_config_import() {
        let doc = json!({"version":3,"rules":[{"type":"logical","mode":"and","rules":[{"domain":["a.invalid"]},{"process_name":["app"]}],"future_field":true}]});
        assert_eq!(
            native_document(&doc.to_string(), "auto").unwrap(),
            Some(doc.clone())
        );
        assert!(native_document(&doc.to_string(), "qx").is_err());
        assert!(native_document("version: 3\nrules:\n  - domain: [a.invalid]", "auto").is_err());
        assert!(native_document(r#"{"version":3,"rules":[],"outbounds":[]}"#, "auto").is_err());
        assert!(native_document("not-json", "native").is_err());
        assert!(native_document("DOMAIN-SUFFIX,a.invalid", "auto")
            .unwrap()
            .is_none());
    }
    #[test]
    fn qx_common_predicates_and_explicit_loss_report() {
        let p = parse("# sample\nHOST-SUFFIX,googlevideo.com,YouTube\nHOST,music.youtube.com,YouTube\nIP6-CIDR,2620:120:e000::/40,YouTube\nUSER-AGENT,*youtube*,YouTube\nHOST-WILDCARD,youtube.*,YouTube", "auto").unwrap();
        assert_eq!(p.rules.len(), 3);
        assert_eq!(p.warnings.len(), 2);
        assert_eq!(p.input_count, 5);
        assert_eq!(p.policies, ["YouTube"]);
        assert_eq!(p.format, "qx");
    }
    #[test]
    fn clash_payload_and_domain_provider() {
        assert_eq!(
            parse(
                "payload:\n - DOMAIN-SUFFIX,example.com\n - IP-CIDR,10.0.0.0/8",
                "auto"
            )
            .unwrap()
            .rules
            .len(),
            2
        );
        let p = parse(
            "payload:\n - '+.example.com'\n - exact.example.org",
            "domain",
        )
        .unwrap();
        assert_eq!(p.rules[0].kind, "domain_suffix");
        assert_eq!(p.rules[1].kind, "domain");
    }
    #[test]
    fn dedup_and_case_normalization() {
        let p = parse("HOST,Example.COM,P\nDOMAIN,example.com,P", "auto").unwrap();
        assert_eq!(p.input_count, 2);
        assert_eq!(p.rules.len(), 1);
    }
    #[test]
    fn detected_format_is_safe_to_reuse_on_refresh() {
        let text = "DOMAIN,example.com,P\n+.example.org";
        let p = parse(text, "auto").unwrap();
        assert_eq!(parse(text, &p.format).unwrap().rules, p.rules);
    }
    #[test]
    fn unsupported_conditions_are_not_silently_weakened() {
        assert!(parse("IP-CIDR,10.0.0.0/8,no-resolve", "auto").is_err());
        assert!(parse("IP-CIDR,10.0.0.0/8,P,no-resolve", "auto").is_err());
        assert!(parse(
            r#"{"version":3,"rules":[{"domain":["example.com"],"invert":true}]}"#,
            "auto"
        )
        .is_err());
        assert!(parse(
            r#"{"version":3,"rules":[{"domain":["example.com"],"process_name":["curl"]}]}"#,
            "auto"
        )
        .is_err());
        assert!(parse("FINAL,proxy", "auto").is_err());
    }
    #[test]
    fn native_or_semantics_and_dns_projection() {
        let p = parse(r#"{"version":3,"rules":[{"domain":["example.com"],"ip_cidr":["10.0.0.0/8"]},{"process_name":["curl"]}]}"#, "auto").unwrap();
        assert_eq!(native_rules(&p.rules).len(), 3);
        assert_eq!(domain_rules(&p.rules).len(), 1);
    }
    #[test]
    fn rejects_empty_html_invalid_networks_and_full_config() {
        for s in [
            "",
            "# nothing",
            "<html>login</html>",
            "IP-CIDR,10.0.0.0/99,P",
            "*.example.com",
            "proxies: []\nrules: []",
        ] {
            assert!(parse(s, "auto").is_err(), "{s}");
        }
    }
    #[tokio::test]
    #[ignore = "Read-only download of the user's public YouTube rule-list example"]
    async fn public_youtube_rule_conversion() {
        let text = crate::subscription::fetch("https://raw.githubusercontent.com/blackmatrix7/ios_rule_script/master/rule/QuantumultX/YouTube/YouTube.list", "sing/0.3 test", None).await.unwrap();
        let p = parse(&text, "auto").unwrap();
        assert!(p.rules.len() > 150);
        assert!(p
            .warnings
            .iter()
            .all(|w| w.contains("USER-AGENT") || w.contains("HOST-WILDCARD")));
        println!(
            "Public YouTube list: {} entries, {} supported matches, {} warnings",
            p.input_count,
            p.rules.len(),
            p.warnings.len()
        );
    }
}
