//! Offline demo backend: fictional nodes and traffic in memory. It never
//! starts a core, opens sockets or touches system settings.
use super::App;
use crate::{
    api, config, model, native,
    runtime::{Action, ConnectionReport, Reply, Snapshot},
};
use anyhow::Result;
use serde_json::json;

pub struct Demo {
    store: model::Store,
    applied_native: Option<serde_json::Value>,
    connected: bool,
    started_at: u64,
    clock: u64,
    next_id: u64,
    connections: Vec<api::Connection>,
    groups: api::Groups,
    status: api::Status,
}

fn reply(ok: bool, message: impl Into<String>) -> Reply {
    let mut r: Reply =
        serde_json::from_value(json!({"ok":ok,"message":"","needs_auth":false})).unwrap();
    r.message = message.into();
    r
}

const TRAFFIC: [(&str, &str, &str); 14] = [
    ("Safari", "www.apple.com", "proxy"),
    ("Safari", "news.ycombinator.com", "proxy"),
    ("Google Chrome", "www.youtube.com", "media"),
    ("Google Chrome", "i.ytimg.com", "media"),
    ("Google Chrome", "github.com", "proxy"),
    ("Telegram", "149.154.167.51", "proxy"),
    ("Spotify", "spclient.wg.spotify.com", "media"),
    ("curl", "example.com", "direct"),
    ("Mail", "imap.example.invalid", "direct"),
    ("Slack", "wss-primary.slack.com", "proxy"),
    ("Code", "marketplace.visualstudio.com", "proxy"),
    ("Code", "update.code.visualstudio.com", "direct"),
    ("WeChat", "weixin.qq.com", "direct"),
    ("Music", "aod.itunes.apple.com", "direct"),
];

impl Demo {
    pub fn new() -> Result<Self> {
        let mut store = model::Store::new()?;
        store.nodes = crate::subscription::parse(
            "trojan://fictional@jp1.example.invalid:443#Tokyo 01\ntrojan://fictional@jp2.example.invalid:443#Tokyo 02\ntrojan://fictional@sg.example.invalid:443#Singapore\ntrojan://fictional@us.example.invalid:443#San Jose\ntrojan://fictional@hk.example.invalid:443#Hong Kong",
            "demo",
        )?
        .nodes;
        store.subscriptions.push(model::Subscription {
            id: "demo".into(),
            name: "Example Cloud".into(),
            source: "https://example.invalid/sub".into(),
            format: "uri".into(),
            updated_at: model::now() - 3600 * 5,
            warnings: vec![],
            user_agent: String::new(),
        });
        let doc = native::migration(&store)?;
        native::adopt(&mut store, doc)?;
        let tags: Vec<String> = store.nodes.iter().map(|n| n.tag()).collect();
        let d = store.native.as_mut().unwrap();
        d["outbounds"].as_array_mut().unwrap().push(json!({
            "type":"urltest","tag":"media","outbounds":[tags[2],tags[3],tags[4]],
            "url":"https://www.gstatic.com/generate_204","interval":"3m"
        }));
        store
            .display_names
            .insert("media".into(), "Streaming".into());
        store.display_names.insert("proxy".into(), "Proxy".into());
        d["route"]["rule_set"] = json!([
            {"type":"inline","tag":"streaming-sites","rules":[{"domain_suffix":["youtube.com","ytimg.com","googlevideo.com","spotify.com","netflix.com"]}]}
        ]);
        store
            .display_names
            .insert("streaming-sites".into(), "Streaming sites".into());
        let rules = d["route"]["rules"].as_array_mut().unwrap();
        rules.push(json!({"rule_set":["streaming-sites"],"action":"route","outbound":"media"}));
        rules.push(json!({"domain_suffix":["cn","qq.com"],"action":"route","outbound":"direct"}));
        rules.push(json!({"process_name":["Mail"],"action":"route","outbound":"direct"}));
        Ok(Self {
            store,
            applied_native: None,
            connected: false,
            started_at: 0,
            clock: model::now(),
            next_id: 0,
            connections: vec![],
            groups: api::Groups::default(),
            status: api::Status::default(),
        })
    }
    pub fn running() -> Result<Self> {
        let mut d = Self::new()?;
        d.start();
        d.started_at = model::now() - 4520;
        Ok(d)
    }
    fn start(&mut self) {
        self.connected = true;
        self.applied_native = self.store.native.clone();
        self.started_at = model::now();
        self.groups = api::Groups {
            group: native::array(self.store.native.as_ref().unwrap(), "/outbounds")
                .iter()
                .filter(|v| super::labels::is_group(v))
                .map(|g| api::Group {
                    tag: native::tag(g).into(),
                    kind: g["type"].as_str().unwrap_or("").into(),
                    selectable: g["type"] == "selector",
                    selected: g["default"]
                        .as_str()
                        .or(g["outbounds"][0].as_str())
                        .unwrap_or("")
                        .into(),
                    items: native::array(g, "/outbounds")
                        .iter()
                        .enumerate()
                        .map(|(i, m)| api::GroupItem {
                            tag: m.as_str().unwrap_or("").into(),
                            kind: String::new(),
                            time: model::now() as i64,
                            delay: 60 + (i as i32 * 47) % 180,
                        })
                        .collect(),
                })
                .collect(),
        };
    }
    pub fn snapshot(&self) -> Snapshot {
        let mut store = self.store.clone();
        store.native = store.native.as_ref().map(config::redacted);
        Snapshot {
            started_at: if self.connected { self.started_at } else { 0 },
            selection_recovery: String::new(),
            manager_protocol: crate::runtime::PROTOCOL,
            system_proxy: Default::default(),
            connectivity: Default::default(),
            running_tun: false,
            dirty: self.applied_native != self.store.native,
            store,
            connected: self.connected,
            api_ready: self.connected,
            core: "/demo/sing-box".into(),
            version: "sing-box 1.14.0".into(),
            running_settings: self.connected.then(|| self.store.settings.clone()),
            status: self.status.clone(),
            groups: self.groups.clone(),
            activity: vec!["Demo · fictional data".into()],
            host: "demo".into(),
            ssh: false,
        }
    }
    /// Advance fictional traffic by one second.
    pub fn tick(&mut self, app: &mut App) {
        if !self.connected {
            return;
        }
        self.clock += 1;
        let wave = ((self.clock % 17) as i64 - 8).abs();
        self.status = api::Status {
            traffic_available: true,
            downlink: 180_000 + wave * 95_000,
            uplink: 12_000 + wave * 4_000,
            downlink_total: self.status.downlink_total + 180_000 + wave * 95_000,
            uplink_total: self.status.uplink_total + 12_000 + wave * 4_000,
            connections_out: self.connections.iter().filter(|c| c.closed_at == 0).count() as i32,
            memory: 42 << 20,
            ..Default::default()
        };
        for c in &mut self.connections {
            if c.closed_at == 0 {
                c.downlink_total += 20_000 + (c.id.len() as i64 * 3000);
                c.uplink_total += 1_500;
            }
        }
        let n = self.clock as usize;
        for c in self.connections.iter_mut().filter(|c| c.closed_at == 0) {
            if (c.created_at as u64 / 1000 + n as u64).is_multiple_of(7) {
                c.closed_at = (self.clock * 1000) as i64;
            }
        }
        for k in 0..2 {
            let (app_name, host, _) = TRAFFIC[(n * 5 + k * 3) % TRAFFIC.len()];
            self.next_id += 1;
            let target = self.route(app_name, host);
            let chain = match self.groups.group.iter().find(|g| g.tag == target) {
                Some(g) => vec![g.selected.clone(), target.clone()],
                None => vec![target.clone()],
            };
            let ip = host.parse::<std::net::IpAddr>().is_ok();
            self.connections.push(api::Connection {
                id: format!("00000000-0000-4000-8000-{:012}", self.next_id),
                inbound: "mixed-in".into(),
                inbound_type: "mixed".into(),
                network: "tcp".into(),
                source: "127.0.0.1:52000".into(),
                destination: format!("{host}:443"),
                domain: if ip { String::new() } else { host.into() },
                protocol: if ip { String::new() } else { "tls".into() },
                created_at: (self.clock * 1000) as i64,
                closed_at: 0,
                uplink_total: 800,
                downlink_total: 4_000,
                rule: format!("demo => route({target})"),
                outbound: chain.first().cloned().unwrap_or_default(),
                outbound_type: "trojan".into(),
                chain,
                process: Some(api::ProcessInfo {
                    path: if app_name == "curl" {
                        "/usr/bin/curl".into()
                    } else {
                        format!("/Applications/{app_name}.app/Contents/MacOS/{app_name}")
                    },
                    ..Default::default()
                }),
            });
        }
        if self.connections.len() > 300 {
            self.connections.drain(..100);
        }
        app.observe(self.snapshot());
        app.observe_connections(self.report());
    }
    /// Tiny evaluator for the demo's own simple rules, enough to make fixes visible.
    fn route(&self, app: &str, host: &str) -> String {
        let s = &self.store.settings;
        match s.route_mode.as_str() {
            "direct" => return "direct".into(),
            "global" => return s.global_target.clone(),
            _ => {}
        }
        let doc = self.store.native.as_ref().unwrap();
        let sets = native::array(doc, "/route/rule_set");
        let hit = |rule: &serde_json::Value| -> bool {
            let list = |k: &str| {
                native::array(rule, &format!("/{k}"))
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            };
            let suffix = |sfx: &str| host == sfx || host.ends_with(&format!(".{sfx}"));
            list("domain").iter().any(|d| d == host)
                || list("domain_suffix").iter().any(|d| suffix(d))
                || list("domain_keyword")
                    .iter()
                    .any(|d| host.contains(d.as_str()))
                || list("process_name").iter().any(|p| p == app)
                || list("rule_set").iter().any(|t| {
                    sets.iter().filter(|s| native::tag(s) == t).any(|s| {
                        native::array(s, "/rules").iter().any(|r| {
                            native::array(r, "/domain_suffix")
                                .iter()
                                .filter_map(|v| v.as_str())
                                .any(suffix)
                        })
                    })
                })
        };
        for rule in native::array(doc, "/route/rules") {
            if rule["action"] == "route" && hit(rule) {
                return rule["outbound"].as_str().unwrap_or("direct").into();
            }
        }
        doc.pointer("/route/final")
            .and_then(|v| v.as_str())
            .unwrap_or("direct")
            .into()
    }
    fn report(&self) -> ConnectionReport {
        let mut items = self.connections.clone();
        items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        ConnectionReport {
            observed_at: model::now(),
            total: items.len(),
            items,
        }
    }

    pub fn handle(&mut self, _app: &App, action: Action) -> Reply {
        let result = self.handle_inner(action);
        let mut r = result.unwrap_or_else(|e| reply(false, format!("{e:#}")));
        r.snapshot = Some(self.snapshot());
        r
    }
    fn handle_inner(&mut self, action: Action) -> Result<Reply> {
        Ok(match action {
            Action::Snapshot => reply(true, ""),
            Action::Connections => {
                let mut r = reply(true, "");
                r.connections = Some(self.report());
                r
            }
            Action::ReadNative(p) => {
                let mut r = reply(true, "");
                r.edit = Some(native::read(&self.store, p)?);
                r
            }
            Action::WriteNative(e) => {
                native::write(&mut self.store, e)?;
                reply(true, "Saved to draft (demo)")
            }
            Action::Delete(id) => {
                let mut next = self.store.clone();
                next.subscriptions.retain(|s| s.id != id);
                next.nodes.retain(|n| n.provider != id);
                if !next
                    .nodes
                    .iter()
                    .any(|n| Some(&n.id) == next.selected.as_ref())
                {
                    next.selected = next.nodes.first().map(|n| n.id.clone());
                }
                native::reconcile(&self.store, &mut next)?;
                self.store = next;
                reply(true, "Source removed (demo)")
            }
            Action::WriteGroup(change) => {
                native::write_group(&mut self.store, change)?;
                reply(true, "Group saved (demo)")
            }
            Action::SetMode(mode) => {
                self.store.settings.route_mode = mode.clone();
                reply(true, format!("Mode: {mode} (demo, live)"))
            }
            Action::SetGlobalTarget(t) => {
                self.store.settings.global_target = t;
                reply(true, "Global target switched (demo)")
            }
            Action::SetSystemProxy(on) => {
                self.store.settings.mode = if on { "system" } else { "port" }.into();
                reply(true, "Demo only: macOS settings are not changed")
            }
            Action::SetTun { enabled, .. } => {
                let d = self.store.native.as_mut().unwrap();
                let list = d["inbounds"].as_array_mut().unwrap();
                if enabled {
                    list.push(native::tun_template());
                } else {
                    list.retain(|i| i["type"] != "tun");
                }
                reply(true, "Demo only: TUN is not started")
            }
            Action::ReviewApply => {
                let mut r = reply(true, "");
                r.config = Some(native::review(&self.store, None)?);
                r.diff = Some(native::review::detailed(
                    &json!({}),
                    &config::generate(&self.store)?,
                ));
                r.confirm = Some(Action::ApplyNative {
                    revision: native::revision(&self.store),
                });
                r
            }
            Action::ApplyNative { .. } | Action::Connect => {
                self.start();
                reply(true, "Demo core “started” with fictional traffic")
            }
            Action::Disconnect => {
                self.connected = false;
                self.connections.clear();
                self.groups = api::Groups::default();
                self.status = api::Status::default();
                reply(true, "Stopped (demo)")
            }
            Action::SelectNative { group, member } => {
                if let Some(g) = self.groups.group.iter_mut().find(|g| g.tag == group) {
                    g.selected = member;
                }
                reply(true, "Selected (demo)")
            }
            Action::Test(tag) => {
                for g in &mut self.groups.group {
                    if g.tag == tag || tag.is_empty() {
                        for (i, item) in g.items.iter_mut().enumerate() {
                            item.delay = 40 + ((self.clock as i32 + i as i32 * 37) % 220);
                        }
                    }
                }
                reply(true, "Latency tested (demo)")
            }
            Action::CloseConnection(id) => {
                if let Some(c) = self.connections.iter_mut().find(|c| c.id == id) {
                    c.closed_at = (self.clock * 1000) as i64;
                }
                reply(true, "Connection closed (demo)")
            }
            Action::Logs => {
                let mut r = reply(true, "");
                r.config = Some("+0000 INFO router: demo core started\n+0000 WARN dns: fictional warning example\n+0000 INFO outbound/trojan[Tokyo 01]: fictional connection".into());
                r
            }
            Action::CoreReport { .. } => {
                let mut r = reply(true, "");
                r.cores = Some(crate::runtime::CoreReport {
                    installed: vec![crate::runtime::CoreInfo {
                        path: "/demo/sing-box".into(),
                        version: "sing-box version 1.14.0".into(),
                        source: "sing".into(),
                        selected: true,
                        supported: true,
                    }],
                    releases: vec![],
                    releases_error: "Demo: release list is not fetched".into(),
                    running: self.connected,
                    tested: crate::runtime::CORE_VERSION.into(),
                });
                r
            }
            Action::Diagnostics => {
                let mut r = reply(true, "");
                r.config = Some(config::diagnostics(&self.store, None));
                r
            }
            _ => reply(true, "Demo: this action needs a real sing manager"),
        })
    }
}
