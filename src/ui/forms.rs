use super::*;

#[derive(Clone)]
pub(super) struct Input {
    pub value: String,
    pub cursor: usize,
}
impl Input {
    pub fn new(value: String) -> Self {
        let cursor = value.len();
        Self { value, cursor }
    }
    pub fn insert(&mut self, s: &str) {
        let s: String = s
            .chars()
            .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
            .collect();
        self.value.insert_str(self.cursor, &s);
        self.cursor += s.len();
    }
    pub fn key(&mut self, k: KeyEvent, multiline: bool) {
        match k.code {
            K::Left => {
                self.cursor = self.value[..self.cursor]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(i, _)| i)
            }
            K::Right => {
                if let Some(c) = self.value[self.cursor..].chars().next() {
                    self.cursor += c.len_utf8();
                }
            }
            K::Home => self.cursor = self.value[..self.cursor].rfind('\n').map_or(0, |i| i + 1),
            K::End => {
                self.cursor += self.value[self.cursor..]
                    .find('\n')
                    .unwrap_or(self.value.len() - self.cursor)
            }
            K::Backspace if self.cursor > 0 => {
                let i = self.value[..self.cursor]
                    .char_indices()
                    .next_back()
                    .unwrap()
                    .0;
                self.value.replace_range(i..self.cursor, "");
                self.cursor = i;
            }
            K::Delete if self.cursor < self.value.len() => {
                let n = self.value[self.cursor..].chars().next().unwrap().len_utf8();
                self.value.replace_range(self.cursor..self.cursor + n, "");
            }
            K::Enter if multiline => self.insert("\n"),
            K::Up | K::Down if multiline => {
                let start = self.value[..self.cursor].rfind('\n').map_or(0, |i| i + 1);
                let col = self.value[start..self.cursor].chars().count();
                let target = if k.code == K::Up {
                    if start == 0 {
                        return;
                    }
                    self.value[..start - 1].rfind('\n').map_or(0, |i| i + 1)
                } else {
                    let Some(n) = self.value[self.cursor..].find('\n') else {
                        return;
                    };
                    self.cursor + n + 1
                };
                let line = self.value[target..].split('\n').next().unwrap_or("");
                self.cursor = target + line.char_indices().nth(col).map_or(line.len(), |(i, _)| i);
            }
            K::Char('u') if k.modifiers.contains(M::CONTROL) => {
                self.value.clear();
                self.cursor = 0;
            }
            K::Char(c) if !k.modifiers.intersects(M::CONTROL | M::ALT) => {
                self.insert(&c.to_string())
            }
            _ => {}
        }
    }
}
#[derive(Clone)]
pub(super) enum Kind {
    String,
    Number,
    Bool,
    Json,
    TextList,
    Choice(Vec<String>),
    Members(Vec<String>),
}
#[derive(Clone)]
pub(super) struct Field {
    pub key: String,
    pub label: String,
    pub input: Input,
    pub kind: Kind,
    original: String,
}
impl Field {
    pub fn new(key: &str, label: &str, value: &Value, kind: Kind) -> Self {
        let s = if matches!(kind, Kind::TextList) {
            value
                .as_array()
                .map(|a| a.iter().map(text).collect::<Vec<_>>().join(", "))
                .unwrap_or_else(|| text(value))
        } else if matches!(kind, Kind::Json | Kind::Members(_)) && !value.is_null() {
            value.to_string()
        } else {
            text(value)
        };
        Self {
            key: key.into(),
            label: label.into(),
            input: Input::new(s.clone()),
            kind,
            original: s,
        }
    }
    fn value(&self) -> Result<Value> {
        let s = &self.input.value;
        Ok(match self.kind {
            Kind::Number => json!(s
                .parse::<u64>()
                .with_context(|| format!("{} needs a whole number", self.label))?),
            Kind::Bool => json!(s.parse::<bool>().context("Use true or false")?),
            Kind::TextList => json!(s
                .split([',', '\n'])
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()),
            Kind::Json | Kind::Members(_) => serde_json::from_str(s)
                .with_context(|| format!("{} needs valid JSON", self.label))?,
            _ => json!(s),
        })
    }
}
#[derive(Clone)]
pub(super) enum FormAction {
    Native(Edit),
    Import,
    Convert,
    Settings(model::Settings),
}
#[derive(Clone)]
pub(super) struct Form {
    pub title: String,
    pub fields: Vec<Field>,
    pub selected: usize,
    pub action: FormAction,
}
impl Form {
    pub fn submit(&self) -> Result<Action> {
        let get = |key: &str| {
            self.fields
                .iter()
                .find(|f| f.key == key)
                .map(|f| f.input.value.clone())
                .unwrap_or_default()
        };
        match &self.action {
            FormAction::Native(edit) => {
                let mut e = edit.clone();
                if e.value.is_null() {
                    e.value = json!({});
                }
                for f in &self.fields {
                    if f.input.value == f.original {
                        continue;
                    }
                    if f.input.value.is_empty() {
                        e.value
                            .as_object_mut()
                            .context("Object expected")?
                            .remove(&f.key);
                    } else {
                        e.value[&f.key] = f.value()?;
                    }
                }
                Ok(Action::WriteNative(e))
            }
            FormAction::Import => Ok(Action::Import {
                source: get("source"),
                name: get("name"),
                user_agent: get("user_agent"),
            }),
            FormAction::Convert => Ok(Action::ImportRules {
                source: get("source"),
                name: get("name"),
                format: get("format"),
                target: get("target"),
            }),
            FormAction::Settings(s) => {
                let mut s = s.clone();
                s.language = "en".into();
                for f in &self.fields {
                    match f.key.as_str() {
                        "mode" => s.mode = get("mode"),
                        "port" => s.port = get("port").parse().context("Invalid port")?,
                        "api_port" => {
                            s.api_port = get("api_port").parse().context("Invalid API port")?
                        }
                        "core" => s.core = get("core"),
                        "route_mode" => s.route_mode = get("route_mode"),
                        "global_target" => s.global_target = get("global_target"),
                        "bypass_lan" => s.bypass_lan = get("bypass_lan") == "true",
                        _ => {}
                    }
                }
                Ok(Action::SaveSettings(s))
            }
        }
    }
}
pub(super) fn object_form(edit: Edit, doc: &Value) -> Result<Form> {
    ensure!(
        edit.value.is_object() || edit.value.is_null(),
        "Use E for native JSON on this object"
    );
    let v = &edit.value;
    let p = &edit.pointer;
    let mut specs: Vec<(&str, &str, Kind)> = vec![];
    let mut out = names(doc, "/outbounds");
    out.extend(names(doc, "/endpoints"));
    let dns = names(doc, "/dns/servers");
    let optional = |mut v: Vec<String>| {
        v.insert(0, String::new());
        v
    };
    if p.starts_with("/inbounds/") {
        specs.extend([("tag", "Tag", Kind::String), ("type", "Type", Kind::String)]);
        if v["type"] == "tun" {
            specs.extend([
                ("address", "Interface addresses (JSON)", Kind::Json),
                ("auto_route", "Automatic routes", Kind::Bool),
                ("strict_route", "Strict routing", Kind::Bool),
                (
                    "stack",
                    "Stack",
                    Kind::Choice(vec!["mixed".into(), "system".into(), "gvisor".into()]),
                ),
                (
                    "dns_mode",
                    "Interface DNS mode",
                    Kind::Choice(vec!["hijack".into(), "native".into(), "disabled".into()]),
                ),
                ("mtu", "MTU", Kind::Number),
            ]);
        } else {
            specs.extend([
                ("listen", "Listen address", Kind::String),
                ("listen_port", "Port", Kind::Number),
                ("users", "Authentication users (JSON)", Kind::Json),
            ]);
        }
    } else if p.starts_with("/outbounds/") {
        specs.extend([("tag", "Tag", Kind::String), ("type", "Type", Kind::String)]);
        if v["type"] == "selector" || v["type"] == "urltest" {
            out.retain(|s| s != native::tag(v));
            specs.push((
                "outbounds",
                "Members · Space to choose",
                Kind::Members(out.clone()),
            ));
            if v["type"] == "selector" {
                specs.push(("default", "Default member", Kind::Choice(optional(out))));
            } else {
                specs.extend([
                    ("url", "Test URL", Kind::String),
                    ("interval", "Interval", Kind::String),
                    ("tolerance", "Tolerance (ms)", Kind::Number),
                ]);
            }
            specs.push((
                "interrupt_exist_connections",
                "Interrupt existing connections",
                Kind::Bool,
            ));
        } else {
            if v["type"] != "direct" {
                specs.extend([
                    ("server", "Server", Kind::String),
                    ("server_port", "Port", Kind::Number),
                ]);
            }
            specs.extend([
                (
                    "detour",
                    "Upstream outbound (blank: direct dial)",
                    Kind::Choice(optional(out)),
                ),
                (
                    "domain_resolver",
                    "Server hostname resolver",
                    Kind::Choice(optional(dns)),
                ),
                ("connect_timeout", "Connect timeout", Kind::String),
            ]);
        }
    } else if p.starts_with("/dns/servers/") {
        out.retain(|t| {
            !native::array(doc, "/outbounds").iter().any(|v| {
                native::tag(v) == t
                    && v["type"] == "direct"
                    && v.as_object()
                        .is_some_and(|o| o.keys().all(|k| k == "tag" || k == "type"))
            })
        });
        specs.extend([("tag", "Tag", Kind::String), ("type", "Type", Kind::String)]);
        if !["local", "fakeip", "hosts"].contains(&v["type"].as_str().unwrap_or("")) {
            specs.extend([
                ("server", "Server hostname / IP", Kind::String),
                (
                    "server_port",
                    "Port (blank: protocol default)",
                    Kind::Number,
                ),
            ]);
            if v["type"] == "https" || v["type"] == "h3" {
                specs.push(("path", "Path", Kind::String));
            }
            specs.extend([
                (
                    "detour",
                    "Via outbound (blank: direct dial)",
                    Kind::Choice(optional(out)),
                ),
                (
                    "domain_resolver",
                    "Bootstrap resolver",
                    Kind::Choice(optional(dns)),
                ),
            ]);
        }
        if v["type"] == "fakeip" {
            specs.extend([
                ("inet4_range", "IPv4 range", Kind::String),
                ("inet6_range", "IPv6 range", Kind::String),
            ]);
        }
    } else if p == "/dns" {
        specs.extend([
            ("final", "Default resolver", Kind::Choice(optional(dns))),
            (
                "strategy",
                "Address preference",
                Kind::Choice(vec![
                    "".into(),
                    "prefer_ipv4".into(),
                    "prefer_ipv6".into(),
                    "ipv4_only".into(),
                    "ipv6_only".into(),
                ]),
            ),
            ("timeout", "Query timeout", Kind::String),
            ("disable_cache", "Disable cache", Kind::Bool),
            ("cache_capacity", "Cache capacity", Kind::Number),
            ("optimistic", "Optimistic cache (JSON)", Kind::Json),
            ("reverse_mapping", "Reverse mapping", Kind::Bool),
        ]);
    } else if p == "/route" {
        specs.extend([
            ("final", "Default outbound", Kind::Choice(optional(out))),
            (
                "default_domain_resolver",
                "Default server hostname resolver",
                Kind::Choice(optional(dns)),
            ),
            (
                "auto_detect_interface",
                "Detect outbound interface",
                Kind::Bool,
            ),
        ]);
    } else if p.starts_with("/route/rule_set/") {
        specs.extend([("tag", "Tag", Kind::String), ("type", "Type", Kind::String)]);
        if v["type"] == "inline" {
            specs.push(("rules", "Native headless rules (JSON)", Kind::Json));
        } else {
            specs.push((
                "format",
                "Format",
                Kind::Choice(vec!["source".into(), "binary".into()]),
            ));
            if v["type"] == "local" {
                specs.push(("path", "File path", Kind::String));
            } else {
                specs.extend([
                    ("url", "Remote URL", Kind::String),
                    ("http_client", "HTTP client tag", Kind::String),
                    ("update_interval", "Update interval", Kind::String),
                    ("initial_path", "Initial local cache", Kind::String),
                ]);
            }
        }
    } else if p.starts_with("/route/rules/") || p.starts_with("/dns/rules/") {
        if v["type"] == "logical" {
            specs.extend([
                (
                    "mode",
                    "Logic",
                    Kind::Choice(vec!["and".into(), "or".into()]),
                ),
                ("rules", "Nested rules (JSON)", Kind::Json),
            ]);
        } else {
            specs.extend([
                (
                    "domain_suffix",
                    "Domain suffixes (comma separated)",
                    Kind::TextList,
                ),
                ("domain", "Exact domains (comma separated)", Kind::TextList),
                (
                    "rule_set",
                    "Rule sets · Space to choose",
                    Kind::Members(names(doc, "/route/rule_set")),
                ),
                (
                    "ip_cidr",
                    "Destination CIDRs (comma separated)",
                    Kind::TextList,
                ),
                (
                    "process_name",
                    "Process names (comma separated)",
                    Kind::TextList,
                ),
                (
                    "process_path",
                    "Process paths (comma separated)",
                    Kind::TextList,
                ),
                (
                    "network",
                    "Network",
                    Kind::Choice(vec!["".into(), "tcp".into(), "udp".into(), "icmp".into()]),
                ),
                ("port", "Destination ports (JSON)", Kind::Json),
            ]);
        }
        specs.push(("invert", "Invert match", Kind::Bool));
        if p.starts_with("/dns") {
            specs.extend([
                (
                    "action",
                    "Action",
                    Kind::Choice(vec![
                        "route".into(),
                        "reject".into(),
                        "route-options".into(),
                        "predefined".into(),
                    ]),
                ),
                ("server", "Resolver", Kind::Choice(optional(dns))),
            ]);
        } else {
            specs.extend([
                (
                    "action",
                    "Action",
                    Kind::Choice(vec![
                        "route".into(),
                        "reject".into(),
                        "sniff".into(),
                        "hijack-dns".into(),
                        "resolve".into(),
                        "route-options".into(),
                    ]),
                ),
                ("outbound", "Outbound", Kind::Choice(optional(out))),
            ]);
        }
    }
    ensure!(!specs.is_empty(), "Use E for the native JSON editor");
    let fields = specs
        .into_iter()
        .map(|(k, l, t)| Field::new(k, l, &v[k], t))
        .collect();
    Ok(Form {
        title: edit.pointer.clone(),
        fields,
        selected: 0,
        action: FormAction::Native(edit),
    })
}
pub(super) fn templates(path: &str) -> Vec<(String, Value)> {
    let values = match path {
        "/inbounds" => vec![
            json!({"type":"mixed","tag":"local-proxy","listen":"127.0.0.1","listen_port":2081}),
            json!({"type":"tun","tag":"tun-in","address":["172.19.0.1/30"],"auto_route":true,"stack":"mixed","dns_mode":"hijack"}),
        ],
        "/outbounds" => vec![
            json!({"type":"selector","tag":"new-group","outbounds":[]}),
            json!({"type":"urltest","tag":"auto-group","outbounds":[],"url":"https://www.gstatic.com/generate_204","interval":"3m","tolerance":50}),
            json!({"type":"direct","tag":"new-direct"}),
            json!({"type":"socks","tag":"socks-proxy","server":"127.0.0.1","server_port":1080}),
        ],
        "/dns/servers" => [
            "local", "udp", "tcp", "tls", "https", "quic", "h3", "fakeip",
        ]
        .iter()
        .map(|t| {
            let mut v = json!({"type":t,"tag":format!("dns-{t}")});
            if *t == "fakeip" {
                v["inet4_range"] = json!("198.18.0.0/15");
                v["inet6_range"] = json!("fc00::/18");
            } else if *t != "local" {
                v["server"] = json!("1.1.1.1");
                if *t == "https" || *t == "h3" {
                    v["path"] = json!("/dns-query");
                }
            }
            v
        })
        .collect(),
        "/route/rules" => vec![
            json!({"domain_suffix":["example.invalid"],"action":"route","outbound":"proxy"}),
            json!({"process_name":["example-process"],"action":"route","outbound":"proxy"}),
            json!({"type":"logical","mode":"and","rules":[],"action":"route","outbound":"proxy"}),
        ],
        "/dns/rules" => vec![
            json!({"domain_suffix":["example.invalid"],"action":"route","server":"bootstrap"}),
            json!({"type":"logical","mode":"and","rules":[],"action":"route","server":"bootstrap"}),
        ],
        "/route/rule_set" => vec![
            json!({"type":"remote","tag":"new-rules","format":"binary","url":"https://example.invalid/rules.srs","update_interval":"1d"}),
            json!({"type":"local","tag":"local-rules","format":"source","path":"rules.json"}),
            json!({"type":"inline","tag":"inline-rules","rules":[]}),
        ],
        _ => vec![],
    };
    values
        .into_iter()
        .map(|v| {
            let label = if v["type"] == "selector" {
                "Manual group (selector)".into()
            } else if v["type"] == "urltest" {
                "Automatic group (urltest)".into()
            } else if v.get("type").is_some() {
                text(&v["type"])
            } else if v.get("process_name").is_some() {
                "Process rule".into()
            } else {
                "Domain rule".into()
            };
            (label, v)
        })
        .collect()
}
