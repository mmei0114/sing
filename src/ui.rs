use crate::{
    config,
    model::{self, ProxyGroup, Rule, RuleBinding, Store},
    runtime::{self, Action, ConnectionReport, ImportPreview, Reply, RulesPreview, Snapshot},
    subscription,
};
use anyhow::{Context, Result};
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEvent, KeyModifiers, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{CrosstermBackend, TestBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

const BG: Color = Color::Rgb(17, 22, 29);
const PANEL: Color = Color::Rgb(24, 31, 40);
const FG: Color = Color::Rgb(222, 228, 235);
const MUTED: Color = Color::Rgb(132, 149, 167);
const ACCENT: Color = Color::Rgb(115, 218, 202);
const WARN: Color = Color::Rgb(236, 192, 115);
const ERROR: Color = Color::Rgb(243, 137, 142);
fn tr<'a>(zh: bool, en: &'a str, cn: &'a str) -> &'a str {
    if zh {
        cn
    } else {
        en
    }
}
fn block(title: impl Into<String>) -> Block<'static> {
    Block::default()
        .title(format!(" {} ", title.into()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(51, 66, 81)))
        .style(Style::default().bg(PANEL).fg(FG))
}
fn option_label(zh: bool, value: &str) -> &str {
    match value {
        "rule" => tr(zh, "Rule-based", "规则分流"),
        "global" => tr(zh, "Global proxy", "全局代理"),
        "direct" => tr(zh, "Direct", "直连"),
        "port" => tr(zh, "Local proxy port", "本机代理端口"),
        "system" => tr(zh, "System proxy", "系统代理"),
        "tun" => tr(zh, "TUN (experimental)", "TUN（实验性）"),
        "on" => tr(zh, "On", "开启"),
        "off" => tr(zh, "Off", "关闭"),
        "legacy" => tr(zh, "Legacy (single resolver)", "兼容策略（单一解析器）"),
        "paired" => tr(zh, "Paired with domain rules", "配套策略（跟随域名规则）"),
        "ipv4_only" => tr(zh, "IPv4 only", "仅 IPv4"),
        "ipv6_only" => tr(zh, "IPv6 only", "仅 IPv6"),
        "prefer_ipv4" => tr(zh, "Prefer IPv4", "优先 IPv4"),
        "prefer_ipv6" => tr(zh, "Prefer IPv6", "优先 IPv6"),
        "en" => tr(zh, "English", "英语"),
        "zh" => tr(zh, "Chinese", "中文"),
        _ => value,
    }
}
fn builtin_target(zh: bool, value: &str) -> &str {
    match value {
        "proxy" => tr(zh, "Default proxy (Nodes)", "默认代理组（节点页）"),
        "direct" => tr(zh, "Direct", "直连"),
        "reject" => tr(zh, "Reject", "阻断"),
        _ => value,
    }
}

fn home_route(snapshot: &Snapshot) -> (String, String) {
    let zh = snapshot.store.settings.language == "zh";
    let settings = snapshot
        .running_settings
        .as_ref()
        .filter(|_| snapshot.connected)
        .unwrap_or(&snapshot.store.settings);
    if settings.route_mode == "direct" {
        return (
            tr(zh, "Route", "路由").into(),
            tr(zh, "Direct · no proxy node used", "直连 · 不使用代理节点").into(),
        );
    }
    let (label, target) = if settings.route_mode == "global" {
        (tr(zh, "Global target", "全局目标"), &settings.global_target)
    } else {
        (tr(zh, "Unmatched traffic", "未匹配流量"), &settings.routing)
    };
    let group_name = builtin_target(zh, &config::target_label(&snapshot.store, target)).to_string();
    if target == "direct" {
        return (label.into(), group_name);
    }
    // Never present a saved selection as the active node after an unapplied edit.
    let node = if snapshot.connected {
        snapshot
            .groups
            .group
            .iter()
            .find(|g| &g.tag == target)
            .filter(|_| snapshot.api_ready)
            .map(|g| config::target_label(&snapshot.store, &g.selected))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| tr(zh, "live node unavailable", "当前节点无法确认").into())
    } else if target == "proxy" {
        snapshot
            .store
            .selected
            .as_ref()
            .and_then(|id| snapshot.store.nodes.iter().find(|n| &n.id == id))
            .or_else(|| snapshot.store.nodes.first())
            .map(|n| n.name.clone())
            .unwrap_or_else(|| tr(zh, "no nodes", "无节点").into())
    } else {
        snapshot
            .store
            .proxy_groups
            .iter()
            .find(|g| g.tag() == *target)
            .and_then(|g| {
                if g.kind == "urltest" {
                    Some(tr(zh, "automatic after startup", "启动后自动选择").into())
                } else {
                    g.selected
                        .as_ref()
                        .map(|id| config::target_label(&snapshot.store, &format!("n-{id}")))
                }
            })
            .unwrap_or_else(|| tr(zh, "no selection", "未选择").into())
    };
    (label.into(), format!("{group_name} → {node}"))
}
fn bytes(n: i64) -> String {
    if n >= 1024 * 1024 * 1024 {
        format!("{:.1} GiB", n as f64 / 1073741824.)
    } else if n >= 1024 * 1024 {
        format!("{:.1} MiB", n as f64 / 1048576.)
    } else if n >= 1024 {
        format!("{:.1} KiB", n as f64 / 1024.)
    } else {
        format!("{n} B")
    }
}

struct Field {
    label: String,
    value: String,
    secret: bool,
    choices: Vec<String>,
    cursor: usize,
    localize_choices: bool,
}
impl Field {
    fn text(label: &str, value: &str) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            secret: false,
            choices: vec![],
            cursor: value.chars().count(),
            localize_choices: false,
        }
    }
    fn choice(label: &str, value: &str, choices: &[&str]) -> Self {
        let mut f = Self::text(label, value);
        f.choices = choices.iter().map(|s| s.to_string()).collect();
        f.localize_choices = true;
        f
    }
    fn display_value(&self, zh: bool) -> &str {
        if self.localize_choices {
            option_label(zh, &self.value)
        } else if !self.choices.is_empty() {
            builtin_target(zh, &self.value)
        } else {
            &self.value
        }
    }
    fn insert(&mut self, text: &str) {
        if !self.choices.is_empty() {
            return;
        }
        let text: String = text
            .chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .take(8 * 1024 * 1024)
            .collect();
        if self.value.len() + text.len() > 8 * 1024 * 1024 {
            return;
        }
        let pos = self
            .value
            .char_indices()
            .nth(self.cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.value.len());
        self.value.insert_str(pos, &text);
        self.cursor += text.chars().count();
    }
    fn key(&mut self, k: KeyEvent) {
        if !self.choices.is_empty() {
            if matches!(
                k.code,
                KeyCode::Char(' ') | KeyCode::Left | KeyCode::Right | KeyCode::Enter
            ) {
                let index = self
                    .choices
                    .iter()
                    .position(|s| s == &self.value)
                    .unwrap_or(0);
                self.value = self.choices[(index + 1) % self.choices.len()].clone();
            }
            return;
        }
        match k.code {
            KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                self.value.clear();
                self.cursor = 0;
            }
            KeyCode::Char(c)
                if !k
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.insert(&c.to_string())
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.value.chars().count()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.value.chars().count(),
            KeyCode::Backspace if self.cursor > 0 => {
                let start = self.value.char_indices().nth(self.cursor - 1).unwrap().0;
                let end = self
                    .value
                    .char_indices()
                    .nth(self.cursor)
                    .map(|(i, _)| i)
                    .unwrap_or(self.value.len());
                self.value.replace_range(start..end, "");
                self.cursor -= 1;
            }
            KeyCode::Delete => {
                if let Some((start, c)) = self.value.char_indices().nth(self.cursor) {
                    self.value.replace_range(start..start + c.len_utf8(), "");
                }
            }
            _ => {}
        }
    }
}
enum FormKind {
    Capture,
    Routing,
    Dns,
    Import,
    Settings,
    Rule,
    Group(ProxyGroup),
    RulesImport,
    Binding(RuleBinding),
}
enum Dialog {
    SettingsMenu(usize),
    Form {
        title: String,
        kind: FormKind,
        fields: Vec<Field>,
        selected: usize,
    },
    Import(ImportPreview, u16),
    RulesReview(RulesPreview, u16),
    Members {
        group: ProxyGroup,
        cursor: usize,
        filter: String,
        searching: bool,
        select_only: bool,
    },
    Text {
        title: String,
        text: String,
        scroll: u16,
    },
    Confirm {
        title: String,
        text: String,
        action: Action,
    },
    Authorize {
        kind: String,
        after: Action,
    },
}
struct App {
    snapshot: Snapshot,
    page: usize,
    selected: [usize; 8],
    connections: ConnectionReport,
    connection_filter: String,
    show_closed: bool,
    connections_error: String,
    filter: String,
    searching: bool,
    dialog: Option<Dialog>,
    notice: String,
    error: bool,
    busy: bool,
    polling: bool,
    demo: bool,
    tick: usize,
    quit: bool,
    last_poll: Instant,
    scroll: u16,
}
impl App {
    fn new(snapshot: Snapshot, demo: bool) -> Self {
        Self {
            snapshot,
            page: 0,
            selected: [0; 8],
            connections: ConnectionReport::default(),
            connection_filter: String::new(),
            show_closed: false,
            connections_error: String::new(),
            filter: String::new(),
            searching: false,
            dialog: None,
            notice: if demo {
                "DEMO · fictional nodes, no network actions".into()
            } else {
                "Welcome. Add a subscription to begin.".into()
            },
            error: false,
            busy: false,
            polling: false,
            demo,
            tick: 0,
            quit: false,
            last_poll: Instant::now(),
            scroll: 0,
        }
    }
    fn zh(&self) -> bool {
        self.snapshot.store.settings.language == "zh"
    }
    fn nodes(&self) -> Vec<&model::Node> {
        let q = self.filter.to_lowercase();
        self.snapshot
            .store
            .nodes
            .iter()
            .filter(|n| {
                format!("{} {} {}", n.name, n.kind(), n.server())
                    .to_lowercase()
                    .contains(&q)
            })
            .collect()
    }
    fn selected_node(&self) -> Option<String> {
        self.nodes().get(self.selected[1]).map(|n| n.id.clone())
    }
    fn count(&self) -> usize {
        match self.page {
            1 => self.nodes().len(),
            2 => self.snapshot.store.subscriptions.len(),
            3 => 13 + self.snapshot.store.settings.rules.len(),
            5 => self.snapshot.store.proxy_groups.len(),
            6 => self.snapshot.store.rule_bindings.len(),
            7 => self.connection_rows().len(),
            _ => 0,
        }
    }
    fn change_selection(&mut self, delta: isize) {
        let count = self.count();
        self.selected[self.page] = self.selected[self.page]
            .saturating_add_signed(delta)
            .min(count.saturating_sub(1));
    }
    fn import(&mut self) {
        let z = self.zh();
        let mut source = Field::text(
            tr(
                z,
                "Subscription URL / node URI / local file",
                "订阅链接 / 节点 URI / 本地文件",
            ),
            "",
        );
        source.secret = true;
        self.dialog = Some(Dialog::Form {
            title: tr(z, "Add subscription", "添加订阅").into(),
            kind: FormKind::Import,
            fields: vec![
                Field::text(tr(z, "Name (optional)", "名称（可选）"), ""),
                source,
                Field::text("User-Agent (optional)", ""),
            ],
            selected: 1,
        });
    }
    fn settings(&mut self) {
        self.dialog = Some(Dialog::SettingsMenu(0));
    }
    fn connection_rows(&self) -> Vec<&crate::api::Connection> {
        let q = self.connection_filter.to_lowercase();
        self.connections
            .items
            .iter()
            .filter(|c| self.show_closed || c.closed_at == 0)
            .filter(|c| {
                format!(
                    "{} {} {} {} {}",
                    c.domain,
                    c.destination,
                    c.rule,
                    config::target_label(&self.snapshot.store, &c.outbound),
                    c.process.as_ref().map(|p| p.path.as_str()).unwrap_or("")
                )
                .to_lowercase()
                .contains(&q)
            })
            .collect()
    }
    fn section(&mut self, section: usize) {
        let s = &self.snapshot.store.settings;
        let (title, kind, fields) = match section {
            0 => (
                tr(
                    self.zh(),
                    "Capture · how traffic enters",
                    "接管 · 流量如何进入",
                ),
                FormKind::Capture,
                vec![
                    Field::choice(
                        tr(self.zh(), "Capture", "接管方式"),
                        &s.mode,
                        if cfg!(target_os = "macos") {
                            &["port", "system", "tun"]
                        } else {
                            &["port", "tun"]
                        },
                    ),
                    Field::text(
                        tr(self.zh(), "Local HTTP/SOCKS port", "本机 HTTP/SOCKS 端口"),
                        &s.port.to_string(),
                    ),
                ],
            ),
            1 => (
                tr(
                    self.zh(),
                    "Routing mode · saved until c applies",
                    "路由模式 · 按 c 后生效",
                ),
                FormKind::Routing,
                vec![
                    Field::choice(
                        tr(self.zh(), "Routing mode", "路由模式"),
                        &s.route_mode,
                        &["rule", "global", "direct"],
                    ),
                    target_field(
                        &self.snapshot.store,
                        tr(
                            self.zh(),
                            "Global target group (global mode only)",
                            "全局目标组（仅全局模式使用）",
                        ),
                        &s.global_target,
                        false,
                    ),
                    target_field(
                        &self.snapshot.store,
                        tr(
                            self.zh(),
                            "Unmatched traffic target (rule mode only)",
                            "未匹配流量目标（仅规则模式使用）",
                        ),
                        &s.routing,
                        false,
                    ),
                    Field::choice(
                        tr(self.zh(), "Private IPs direct", "内网 IP 直连例外"),
                        if s.bypass_lan { "on" } else { "off" },
                        &["on", "off"],
                    ),
                ],
            ),
            2 => (
                tr(
                    self.zh(),
                    "DNS · only queries handled by sing-box",
                    "DNS · 仅处理交给核心的查询",
                ),
                FormKind::Dns,
                vec![
                    Field::choice(
                        tr(self.zh(), "Rule-mode DNS policy", "规则模式 DNS 策略"),
                        &s.dns_policy,
                        &["legacy", "paired"],
                    ),
                    Field::text(tr(self.zh(), "HTTPS resolver IP", "DoH 解析器 IP"), &s.dns),
                    Field::choice(
                        tr(self.zh(), "Address preference", "地址偏好"),
                        &s.dns_strategy,
                        &["ipv4_only", "prefer_ipv4", "prefer_ipv6", "ipv6_only"],
                    ),
                ],
            ),
            _ => {
                self.advanced_settings();
                return;
            }
        };
        self.dialog = Some(Dialog::Form {
            title: title.into(),
            kind,
            fields,
            selected: 0,
        });
        if section == 1 {
            if let Some(Dialog::Form { fields, .. }) = &mut self.dialog {
                fields[1].choices.retain(|s| s != "direct");
            }
        }
    }
    fn advanced_settings(&mut self) {
        let s = &self.snapshot.store.settings;
        self.dialog = Some(Dialog::Form {
            title: tr(self.zh(), "Configuration assistant", "配置辅助").into(),
            kind: FormKind::Settings,
            fields: vec![
                Field::choice(
                    tr(self.zh(), "Capture mode", "接管方式"),
                    "port",
                    if cfg!(target_os = "macos") {
                        &["port", "system", "tun"]
                    } else {
                        &["port", "tun"]
                    },
                ),
                target_field(
                    &self.snapshot.store,
                    tr(self.zh(), "Unmatched traffic target", "未匹配流量目标"),
                    &s.routing,
                    false,
                ),
                Field::text(
                    tr(self.zh(), "HTTPS DNS resolver IP", "DoH 解析器 IP"),
                    &s.dns,
                ),
                Field::text(
                    tr(self.zh(), "Local HTTP / SOCKS port", "本机 HTTP/SOCKS 端口"),
                    &s.port.to_string(),
                ),
                Field::text(
                    tr(self.zh(), "Native gRPC port", "原生管理接口端口"),
                    &s.api_port.to_string(),
                ),
                Field::text(
                    tr(
                        self.zh(),
                        "Core path (blank = auto)",
                        "核心路径（留空自动查找）",
                    ),
                    &s.core,
                ),
                Field::choice(
                    tr(self.zh(), "Language", "界面语言"),
                    &s.language,
                    &["en", "zh"],
                ),
                Field::choice(
                    tr(
                        self.zh(),
                        "DNS policy (paired follows domain rules)",
                        "DNS 策略（配套策略跟随域名规则）",
                    ),
                    &s.dns_policy,
                    &["legacy", "paired"],
                ),
                Field::choice(
                    tr(self.zh(), "DNS address preference", "DNS 地址偏好"),
                    &s.dns_strategy,
                    &["ipv4_only", "prefer_ipv4", "prefer_ipv6", "ipv6_only"],
                ),
            ],
            selected: 0,
        });
        if let Some(Dialog::Form { fields, .. }) = &mut self.dialog {
            fields[0].value = s.mode.clone();
        }
    }
    fn rule(&mut self) {
        self.dialog = Some(Dialog::Form {
            title: tr(self.zh(), "Add routing rule", "添加分流规则").into(),
            kind: FormKind::Rule,
            fields: vec![
                Field::choice(
                    "Match",
                    "domain_suffix",
                    &[
                        "domain_suffix",
                        "domain",
                        "domain_keyword",
                        "domain_regex",
                        "ip_cidr",
                        "process_name",
                        "process_path",
                    ],
                ),
                Field::text("Value (e.g. example.com)", ""),
                target_field(&self.snapshot.store, "Target", "proxy", true),
            ],
            selected: 1,
        });
    }
    fn group_form(&mut self, existing: Option<ProxyGroup>) -> Result<()> {
        let group = match existing {
            Some(g) => g,
            None => ProxyGroup {
                id: model::token()?,
                name: String::new(),
                kind: "selector".into(),
                members: vec![],
                selected: None,
            },
        };
        self.dialog = Some(Dialog::Form {
            title: tr(
                self.zh(),
                "Group · next: choose members",
                "分组 · 下一步选择节点",
            )
            .into(),
            fields: vec![
                Field::text("Group name", &group.name),
                Field::choice(
                    "Type (urltest: gstatic HTTPS every 3m)",
                    &group.kind,
                    &["selector", "urltest"],
                ),
            ],
            kind: FormKind::Group(group),
            selected: 0,
        });
        Ok(())
    }
    fn rules_import(&mut self) {
        let mut source = Field::text("Rule subscription URL / local file", "");
        source.secret = true;
        self.dialog = Some(Dialog::Form {
            title: tr(
                self.zh(),
                "Import classification rules · preview first",
                "导入分类规则 · 先预览",
            )
            .into(),
            kind: FormKind::RulesImport,
            fields: vec![
                Field::text("Name (e.g. YouTube)", ""),
                source,
                Field::choice(
                    "Format",
                    "auto",
                    &["auto", "qx", "clash", "domain", "ipcidr", "native"],
                ),
                target_field(
                    &self.snapshot.store,
                    "Send matching traffic to",
                    "proxy",
                    true,
                ),
            ],
            selected: 1,
        });
    }
    fn binding_form(&mut self, binding: RuleBinding) {
        self.dialog = Some(Dialog::Form {
            title: tr(self.zh(), "Rule destination", "规则分流目标").into(),
            fields: vec![
                target_field(&self.snapshot.store, "Target", &binding.target, true),
                Field::choice(
                    "Enabled",
                    if binding.enabled { "on" } else { "off" },
                    &["on", "off"],
                ),
            ],
            kind: FormKind::Binding(binding),
            selected: 0,
        });
    }
    fn submit_form(&mut self) -> Result<Option<Action>> {
        let Some(Dialog::Form { kind, fields, .. }) = &self.dialog else {
            return Ok(None);
        };
        let action = match kind {
            FormKind::Capture => {
                let mut s = self.snapshot.store.settings.clone();
                s.mode = fields[0].value.clone();
                s.port = fields[1]
                    .value
                    .parse()
                    .context("Proxy port must be a number")?;
                config::validate(&s)?;
                Action::SaveSettings(s)
            }
            FormKind::Routing => {
                let mut s = self.snapshot.store.settings.clone();
                s.route_mode = fields[0].value.clone();
                s.global_target = target_tag(&self.snapshot.store, &fields[1].value);
                s.routing = target_tag(&self.snapshot.store, &fields[2].value);
                s.bypass_lan = fields[3].value == "on";
                config::validate(&s)?;
                Action::SaveSettings(s)
            }
            FormKind::Dns => {
                let mut s = self.snapshot.store.settings.clone();
                s.dns_policy = fields[0].value.clone();
                s.dns = fields[1].value.trim().into();
                s.dns_strategy = fields[2].value.clone();
                config::validate(&s)?;
                Action::SaveSettings(s)
            }
            FormKind::Import => Action::Import {
                source: fields[1].value.trim().into(),
                name: fields[0].value.trim().into(),
                user_agent: fields[2].value.trim().into(),
            },
            FormKind::Settings => {
                let mut s = self.snapshot.store.settings.clone();
                s.mode = fields[0].value.clone();
                s.routing = target_tag(&self.snapshot.store, &fields[1].value);
                s.dns = fields[2].value.trim().into();
                s.port = fields[3]
                    .value
                    .parse()
                    .context("Proxy port must be a number")?;
                s.api_port = fields[4]
                    .value
                    .parse()
                    .context("API port must be a number")?;
                s.core = fields[5].value.trim().into();
                s.language = fields[6].value.clone();
                s.dns_policy = fields[7].value.clone();
                s.dns_strategy = fields[8].value.clone();
                config::validate(&s)?;
                Action::SaveSettings(s)
            }
            FormKind::Rule => {
                let mut s = self.snapshot.store.settings.clone();
                s.rules.push(Rule {
                    kind: fields[0].value.clone(),
                    value: fields[1].value.trim().into(),
                    target: target_tag(&self.snapshot.store, &fields[2].value),
                });
                config::validate(&s)?;
                Action::SaveSettings(s)
            }
            FormKind::Group(group) => {
                let mut group = group.clone();
                group.name = fields[0].value.trim().into();
                group.kind = fields[1].value.clone();
                self.dialog = Some(Dialog::Members {
                    group,
                    cursor: 0,
                    filter: String::new(),
                    searching: false,
                    select_only: false,
                });
                return Ok(None);
            }
            FormKind::RulesImport => Action::ImportRules {
                source: fields[1].value.trim().into(),
                name: fields[0].value.trim().into(),
                format: fields[2].value.clone(),
                target: target_tag(&self.snapshot.store, &fields[3].value),
            },
            FormKind::Binding(binding) => {
                let mut b = binding.clone();
                b.target = target_tag(&self.snapshot.store, &fields[0].value);
                b.enabled = fields[1].value == "on";
                Action::SaveBinding(b)
            }
        };
        self.dialog = None;
        Ok(Some(action))
    }
    fn key(&mut self, k: KeyEvent) -> Result<Option<Action>> {
        if k.modifiers.contains(KeyModifiers::CONTROL) && matches!(k.code, KeyCode::Char('c' | 'q'))
        {
            self.quit = true;
            return Ok(None);
        }
        if self.busy {
            if k.code == KeyCode::Char('q') && k.modifiers.contains(KeyModifiers::CONTROL) {
                self.quit = true;
            }
            return Ok(None);
        }
        if self.dialog.is_some() {
            if k.code == KeyCode::Esc {
                let import = matches!(self.dialog, Some(Dialog::Import(..)));
                let rules = matches!(self.dialog, Some(Dialog::RulesReview(..)));
                self.dialog = None;
                return Ok(if rules {
                    Some(Action::CancelRules)
                } else {
                    import.then_some(Action::CancelImport)
                });
            }
            if matches!(self.dialog, Some(Dialog::Form { .. }))
                && ((k.code == KeyCode::Char('s') && k.modifiers.contains(KeyModifiers::CONTROL))
                    || k.code == KeyCode::F(2))
            {
                return self.submit_form();
            }
            match self.dialog.as_mut().unwrap() {
                Dialog::SettingsMenu(selected) => match k.code {
                    KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                        *selected = (*selected + 1) % 4
                    }
                    KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                        *selected = (*selected + 3) % 4
                    }
                    KeyCode::Char('1'..='4') => {
                        if let KeyCode::Char(c) = k.code {
                            self.section(c.to_digit(10).unwrap() as usize - 1);
                        }
                    }
                    KeyCode::Enter => {
                        let index = *selected;
                        self.section(index);
                    }
                    _ => {}
                },
                Dialog::Members {
                    group,
                    cursor,
                    filter,
                    searching,
                    select_only,
                } => {
                    if *searching {
                        match k.code {
                            KeyCode::Enter => *searching = false,
                            KeyCode::Backspace => {
                                filter.pop();
                                *cursor = 0;
                            }
                            KeyCode::Char(c) => {
                                filter.push(c);
                                *cursor = 0;
                            }
                            _ => {}
                        }
                        return Ok(None);
                    }
                    let rows = member_rows(&self.snapshot.store, group, filter, *select_only);
                    let id = rows.get(*cursor).map(|(id, _)| id.clone());
                    if !*select_only
                        && (k.code == KeyCode::F(2)
                            || (k.code == KeyCode::Char('s')
                                && k.modifiers.contains(KeyModifiers::CONTROL)))
                    {
                        let mut g = group.clone();
                        if !g.selected.as_ref().is_some_and(|id| g.members.contains(id)) {
                            g.selected = g.members.first().cloned();
                        }
                        config::validate_group(&self.snapshot.store, &g)?;
                        self.dialog = None;
                        return Ok(Some(Action::SaveGroup(g)));
                    }
                    match k.code {
                        KeyCode::Down | KeyCode::Char('j') => {
                            *cursor = (*cursor + 1).min(rows.len().saturating_sub(1))
                        }
                        KeyCode::Up | KeyCode::Char('k') => *cursor = cursor.saturating_sub(1),
                        KeyCode::Char('/') => *searching = true,
                        KeyCode::Char(' ') if !*select_only => {
                            if let Some(id) = id {
                                if group.members.contains(&id) {
                                    group.members.retain(|m| m != &id);
                                } else {
                                    group.members.push(id);
                                }
                            }
                        }
                        KeyCode::Enter if *select_only => {
                            if let Some(node) = id {
                                let a = Action::SelectGroup {
                                    group: group.id.clone(),
                                    node,
                                };
                                self.dialog = None;
                                return Ok(Some(a));
                            }
                        }
                        _ => {}
                    }
                }
                Dialog::Form {
                    fields, selected, ..
                } => match k.code {
                    KeyCode::Tab | KeyCode::Down => *selected = (*selected + 1) % fields.len(),
                    KeyCode::BackTab | KeyCode::Up => {
                        *selected = (*selected + fields.len() - 1) % fields.len()
                    }
                    KeyCode::Enter if fields[*selected].choices.is_empty() => {
                        if *selected + 1 < fields.len() {
                            *selected += 1;
                        } else {
                            return self.submit_form();
                        }
                    }
                    _ => fields[*selected].key(k),
                },
                Dialog::Import(_, scroll) => {
                    if k.code == KeyCode::Enter {
                        self.dialog = None;
                        return Ok(Some(Action::CommitImport));
                    }
                    match k.code {
                        KeyCode::Down | KeyCode::Char('j') => *scroll = scroll.saturating_add(1),
                        KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                        KeyCode::PageDown => *scroll = scroll.saturating_add(10),
                        KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
                        _ => {}
                    }
                }
                Dialog::RulesReview(_, scroll) => {
                    if k.code == KeyCode::Enter {
                        self.dialog = None;
                        return Ok(Some(Action::CommitRules));
                    }
                    match k.code {
                        KeyCode::Down | KeyCode::Char('j') => *scroll = scroll.saturating_add(1),
                        KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                        KeyCode::PageDown => *scroll = scroll.saturating_add(10),
                        KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
                        _ => {}
                    }
                }
                Dialog::Confirm { action, .. } => {
                    if k.code == KeyCode::Enter {
                        let a = action.clone();
                        self.dialog = None;
                        return Ok(Some(a));
                    }
                }
                Dialog::Text { scroll, .. } => match k.code {
                    KeyCode::Down | KeyCode::Char('j') => *scroll = scroll.saturating_add(1),
                    KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                    KeyCode::PageDown => *scroll = scroll.saturating_add(15),
                    KeyCode::PageUp => *scroll = scroll.saturating_sub(15),
                    _ => {}
                },
                Dialog::Authorize { .. } => {}
            }
            return Ok(None);
        }
        if self.searching {
            let filter = if self.page == 7 {
                &mut self.connection_filter
            } else {
                &mut self.filter
            };
            match k.code {
                KeyCode::Esc | KeyCode::Enter => self.searching = false,
                KeyCode::Backspace => {
                    filter.pop();
                    self.selected[self.page] = 0;
                }
                KeyCode::Char(c) => {
                    filter.push(c);
                    self.selected[self.page] = 0;
                }
                _ => {}
            }
            return Ok(None);
        }
        if k.modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return Ok(None);
        }
        let action = match k.code {
            KeyCode::Char('q') => {
                self.quit = true;
                None
            }
            KeyCode::Char('1'..='8') => {
                if let KeyCode::Char(c) = k.code {
                    self.page = c.to_digit(10).unwrap() as usize - 1;
                }
                (self.page == 7).then_some(Action::Connections)
            }
            KeyCode::Tab => {
                self.page = (self.page + 1) % 8;
                (self.page == 7).then_some(Action::Connections)
            }
            KeyCode::BackTab => {
                self.page = (self.page + 7) % 8;
                (self.page == 7).then_some(Action::Connections)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.page == 4 {
                    self.scroll = self.scroll.saturating_add(1);
                } else {
                    self.change_selection(1);
                }
                None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.page == 4 {
                    self.scroll = self.scroll.saturating_sub(1);
                } else {
                    self.change_selection(-1);
                }
                None
            }
            KeyCode::Char('a') => {
                if self.page == 5 {
                    self.group_form(None)?;
                } else if self.page == 6 {
                    self.rules_import();
                } else {
                    self.import();
                }
                None
            }
            KeyCode::Char('c') => {
                if self.snapshot.store.settings.mode == "system" {
                    self.dialog=Some(Dialog::Confirm{title:tr(self.zh(),"Enable macOS system proxy?","启用 macOS 系统代理？").into(),text:tr(self.zh(),"After the core is ready, sing sets HTTP / HTTPS / SOCKS proxies on enabled Wi-Fi / Ethernet services in the current location. PAC / auto-discovery are temporarily disabled; bypass lists are preserved.\n\nOriginal settings are backed up. d restores them; q keeps the proxy running. Some apps ignore system settings.\n\nEnter: continue    Esc: cancel","核心就绪后，为当前网络位置中已启用的 Wi-Fi / 以太网服务设置 HTTP、HTTPS、SOCKS 代理，暂时关闭 PAC / 自动发现，保留绕过列表。\n\n先备份原设置；d 关闭并恢复，q 退出界面但保持代理。部分应用可能不遵循系统设置。\n\nEnter 继续    Esc 取消").into(),action:Action::Connect});
                    None
                } else if self.snapshot.store.settings.mode == "tun" {
                    self.dialog=Some(Dialog::Confirm{title:tr(self.zh(),"Enable TUN on this host?","在此主机启用 TUN？").into(),text:format!("Host: {}\n\nTUN changes this machine's routing and DNS interception. It may interrupt SSH. Administrator authorization will be requested.\n\nPort mode is safer for an initial test.\n\nEnter: continue    Esc: cancel",self.snapshot.host),action:Action::Connect});
                    None
                } else {
                    Some(Action::Connect)
                }
            }
            KeyCode::Char('d') => {
                self.dialog=Some(Dialog::Confirm{title:tr(self.zh(),"Stop proxy and restore settings?","关闭代理并恢复设置？").into(),text:tr(self.zh(),"Restore sing-owned system proxy settings BEFORE stopping the core. External changes are preserved. If safe restoration fails, the core is kept running.\n\nEnter: stop proxy    Esc: cancel","先恢复 sing 托管的系统代理设置，再停止核心。保留其他软件的修改；无法安全恢复时，不主动停止核心。\n\nEnter 关闭代理    Esc 取消").into(),action:Action::Disconnect});
                None
            }
            KeyCode::Char('i') if self.page == 0 => {
                self.dialog=Some(Dialog::Confirm{title:"Install official core".into(),text:format!("Download sing-box {} from SagerNet GitHub releases.\nVerify SHA-256, then save to the private app directory.\nNo system installation or network settings change.\n\nEnter: install    Esc: cancel",runtime::CORE_VERSION),action:Action::InstallCore});
                None
            }
            KeyCode::Char('s') if self.page == 0 => {
                self.settings();
                None
            }
            KeyCode::Enter if self.page == 1 => self.selected_node().map(Action::Select),
            KeyCode::Char('f') if self.page == 1 => self.selected_node().map(Action::Favorite),
            KeyCode::Char('t') if self.page == 1 => self.selected_node().map(Action::Test),
            KeyCode::Char('T') if self.page == 1 => Some(Action::Test(String::new())),
            KeyCode::Char('/') if self.page == 1 || self.page == 7 => {
                self.searching = true;
                None
            }
            KeyCode::Esc if self.page == 1 => {
                self.filter.clear();
                self.selected[1] = 0;
                None
            }
            KeyCode::Esc if self.page == 7 => {
                self.connection_filter.clear();
                self.selected[7] = 0;
                None
            }
            KeyCode::Char('M') => {
                self.section(1);
                None
            }
            KeyCode::Char('D') => Some(Action::Diagnostics),
            KeyCode::Char('r') if self.page == 7 => Some(Action::Connections),
            KeyCode::Char('h') if self.page == 7 => {
                self.show_closed = !self.show_closed;
                self.selected[7] = 0;
                None
            }
            KeyCode::Enter if self.page == 7 => {
                if let Some(c) = self.connection_rows().get(self.selected[7]) {
                    self.dialog = Some(Dialog::Text {
                        title: tr(self.zh(), "Observed connection", "实际连接").into(),
                        text: connection_details(&self.snapshot.store, c),
                        scroll: 0,
                    });
                }
                None
            }
            KeyCode::Char('x') if self.page == 7 => {
                if let Some(c) = self
                    .connection_rows()
                    .get(self.selected[7])
                    .filter(|c| c.closed_at == 0)
                {
                    self.dialog = Some(Dialog::Confirm {
                        title: tr(self.zh(), "Close this connection?", "关闭这条连接？").into(),
                        text: format!(
                            "{} → {}\n{}\n\n{}\n\n{}",
                            c.source,
                            c.destination,
                            c.domain,
                            tr(
                                self.zh(),
                                "Only this connection is closed. The app may reconnect.",
                                "仅关闭此连接，不停止代理；应用可能自动重连。"
                            ),
                            tr(
                                self.zh(),
                                "Enter: close    Esc: cancel",
                                "Enter 关闭    Esc 取消"
                            )
                        ),
                        action: Action::CloseConnection(c.id.clone()),
                    });
                }
                None
            }
            KeyCode::Char('r') if self.page == 2 => self
                .snapshot
                .store
                .subscriptions
                .get(self.selected[2])
                .map(|s| Action::Refresh(s.id.clone())),
            KeyCode::Char('x') if self.page == 2 => {
                if let Some(s) = self.snapshot.store.subscriptions.get(self.selected[2]) {
                    self.dialog=Some(Dialog::Confirm{title:"Remove subscription".into(),text:format!("Remove {} and its saved nodes?\nYour provider account is not changed.\nThe running config is unchanged until Apply.\n\nEnter: remove    Esc: cancel",s.name),action:Action::Delete(s.id.clone())});
                }
                None
            }
            KeyCode::Enter if self.page == 3 => {
                self.settings();
                None
            }
            KeyCode::Char('r') if self.page == 3 => {
                self.rule();
                None
            }
            KeyCode::Char('x') if self.page == 3 => {
                let index = self.selected[3];
                if index >= 13 && index - 13 < self.snapshot.store.settings.rules.len() {
                    let mut s = self.snapshot.store.settings.clone();
                    s.rules.remove(index - 13);
                    Some(Action::SaveSettings(s))
                } else {
                    None
                }
            }
            KeyCode::Char('s') if self.page == 3 => Some(Action::Check),
            KeyCode::Char('p') if self.page == 3 => Some(Action::Preview),
            KeyCode::Char('b') if self.page == 3 => {
                self.dialog=Some(Dialog::Confirm{title:"Restore previous applied configuration".into(),text:"This restores previous subscriptions, nodes and network settings, then restarts the core.\n\nEnter: restore    Esc: cancel".into(),action:Action::Rollback});
                None
            }
            KeyCode::Char('L') => {
                let mut s = self.snapshot.store.settings.clone();
                s.language = if s.language == "en" { "zh" } else { "en" }.into();
                Some(Action::SaveSettings(s))
            }
            KeyCode::Char('l') if self.page == 4 => Some(Action::Logs),
            KeyCode::Char('e' | 'm') | KeyCode::Enter if self.page == 5 => {
                if let Some(g) = self
                    .snapshot
                    .store
                    .proxy_groups
                    .get(self.selected[5])
                    .cloned()
                {
                    if k.code == KeyCode::Char('e') {
                        self.group_form(Some(g))?;
                    } else if k.code == KeyCode::Enter && g.kind == "urltest" {
                        self.dialog = Some(Dialog::Text { title:g.name, text:"This group automatically selects by HTTPS latency, not download speed. Tests use https://www.gstatic.com/generate_204 every 3 minutes while active.\n\nPress m to edit members, or e to change its type to selector for manual choice.".into(), scroll:0 });
                    } else {
                        self.dialog = Some(Dialog::Members {
                            group: g,
                            cursor: 0,
                            filter: String::new(),
                            searching: false,
                            select_only: k.code == KeyCode::Enter,
                        });
                    }
                }
                None
            }
            KeyCode::Char('x') if self.page == 5 => {
                if let Some(g) = self.snapshot.store.proxy_groups.get(self.selected[5]) {
                    self.dialog = Some(Dialog::Confirm { title:"Remove group".into(), text:format!("Remove {}? Referenced groups cannot be removed. Nodes are kept.\n\nEnter: remove    Esc: cancel",g.name), action:Action::DeleteGroup(g.id.clone()) });
                }
                None
            }
            KeyCode::Enter | KeyCode::Char('e') if self.page == 6 => {
                if let Some(b) = self
                    .snapshot
                    .store
                    .rule_bindings
                    .get(self.selected[6])
                    .cloned()
                {
                    self.binding_form(b);
                }
                None
            }
            KeyCode::Char('r') if self.page == 6 => self
                .snapshot
                .store
                .rule_bindings
                .get(self.selected[6])
                .map(|b| Action::RefreshRules(b.resource.clone())),
            KeyCode::Char(' ') if self.page == 6 => self
                .snapshot
                .store
                .rule_bindings
                .get(self.selected[6])
                .cloned()
                .map(|mut b| {
                    b.enabled = !b.enabled;
                    Action::SaveBinding(b)
                }),
            KeyCode::Char('[' | ']') if self.page == 6 => self
                .snapshot
                .store
                .rule_bindings
                .get(self.selected[6])
                .map(|b| Action::MoveBinding {
                    id: b.id.clone(),
                    delta: if k.code == KeyCode::Char('[') { -1 } else { 1 },
                }),
            KeyCode::Char('p') if self.page == 6 => {
                if let Some(r) = self
                    .snapshot
                    .store
                    .rule_bindings
                    .get(self.selected[6])
                    .and_then(|b| {
                        self.snapshot
                            .store
                            .rule_resources
                            .iter()
                            .find(|r| r.id == b.resource)
                    })
                {
                    self.dialog = Some(Dialog::Text {
                        title: format!("{} · {}", r.name, r.format),
                        text: format!(
                            "{}\n{} saved matches from {} entries · {} warnings\n\n{}\n\n{}",
                            r.source,
                            r.rules.len(),
                            r.input_count,
                            r.warnings.len(),
                            r.warnings.join("\n"),
                            r.rules
                                .iter()
                                .map(|r| format!("{}  {}", r.kind, r.value))
                                .collect::<Vec<_>>()
                                .join("\n")
                        ),
                        scroll: 0,
                    });
                }
                None
            }
            KeyCode::Char('x') if self.page == 6 => {
                if let Some(b) = self.snapshot.store.rule_bindings.get(self.selected[6]) {
                    self.dialog = Some(Dialog::Confirm { title:"Remove rule subscription".into(), text:"Remove this routing binding and its unused saved rule resource? The remote file is not changed.\n\nEnter: remove    Esc: cancel".into(), action:Action::DeleteBinding(b.id.clone()) });
                }
                None
            }
            KeyCode::Char('v') => {
                self.dialog=Some(Dialog::Confirm{title:tr(self.zh(),"Check HTTPS through proxy","检查代理 HTTPS 访问").into(),text:tr(self.zh(),"Send a small HTTPS request through the local proxy to https://www.gstatic.com/generate_204. This checks this proxy path, NOT whether every app uses it or every site is reachable.\n\nEnter: check    Esc: cancel","通过本地代理向 https://www.gstatic.com/generate_204 发送一个小型 HTTPS 请求。它只检查这条代理路径，不代表每个应用都已使用代理，也不代表所有网站可达。\n\nEnter 检查    Esc 取消").into(),action:Action::Probe});
                None
            }
            KeyCode::Char('R') => {
                self.dialog=Some(Dialog::Confirm{title:tr(self.zh(),"Restore original system proxy","恢复原系统代理").into(),text:tr(self.zh(),"Restore settings still owned by sing, preserving external changes. This disables system takeover without stopping the core; the local proxy port remains available. Authorization may be required.\n\nEnter: restore    Esc: cancel","恢复仍由 sing 托管的设置，保留外部修改。关闭系统接管，但不停止正在运行的核心，本地代理端口仍可用。可能需要管理员授权。\n\nEnter 恢复    Esc 取消").into(),action:Action::RestoreProxy});
                None
            }
            KeyCode::Char('?') => {
                self.dialog = Some(Dialog::Text {
                    title: tr(self.zh(), "Keyboard guide", "键盘帮助").into(),
                    text: help(self.zh()),
                    scroll: 0,
                });
                None
            }
            _ => None,
        };
        Ok(action)
    }
    fn reply(&mut self, r: Reply) {
        if self.page == 7 && !r.ok {
            self.connections_error = r.message.clone();
        }
        if let Some(report) = r.connections {
            let selected_id = self
                .connection_rows()
                .get(self.selected[7])
                .map(|c| c.id.clone());
            self.connections = report;
            self.connections_error.clear();
            self.polling = false;
            self.selected[7] = selected_id
                .and_then(|id| self.connection_rows().iter().position(|c| c.id == id))
                .unwrap_or(0);
        }
        if r.snapshot.is_some() {
            self.polling = false;
        } else {
            self.busy = false;
            self.error = !r.ok;
        }
        if !r.message.is_empty() {
            self.notice = r.message;
        }
        if let Some(s) = r.snapshot {
            self.snapshot = s;
            self.selected[self.page] = self.selected[self.page].min(self.count().saturating_sub(1));
        }
        if let Some(p) = r.preview {
            self.dialog = Some(Dialog::Import(p, 0));
        }
        if let Some(p) = r.rules_preview {
            self.dialog = Some(Dialog::RulesReview(p, 0));
        }
        if let Some(text) = r.config {
            self.dialog = Some(Dialog::Text {
                title: tr(self.zh(), "Details · redacted", "详情 · 已脱敏").into(),
                text,
                scroll: 0,
            });
        }
        if r.needs_auth {
            self.dialog = Some(Dialog::Authorize {
                kind: if r.auth_kind.is_empty() {
                    "tun".into()
                } else {
                    r.auth_kind
                },
                after: r.after_auth.unwrap_or(Action::Connect),
            });
        }
        self.last_poll = Instant::now();
    }
}

fn target_name(store: &Store, tag: &str) -> String {
    store
        .proxy_groups
        .iter()
        .find(|g| g.tag() == tag)
        .map(|g| g.name.clone())
        .unwrap_or_else(|| tag.into())
}
fn target_tag(store: &Store, name: &str) -> String {
    store
        .proxy_groups
        .iter()
        .find(|g| g.name == name)
        .map(|g| g.tag())
        .unwrap_or_else(|| name.into())
}
fn target_field(store: &Store, label: &str, target: &str, reject: bool) -> Field {
    let mut f = Field::text(label, &target_name(store, target));
    f.choices = vec!["proxy".into(), "direct".into()];
    if reject {
        f.choices.push("reject".into());
    }
    f.choices
        .extend(store.proxy_groups.iter().map(|g| g.name.clone()));
    f
}
fn member_rows(
    store: &Store,
    group: &ProxyGroup,
    filter: &str,
    select_only: bool,
) -> Vec<(String, String)> {
    let mut rows: Vec<_> = store
        .nodes
        .iter()
        .filter(|n| !select_only || group.members.contains(&n.id))
        .map(|n| (n.id.clone(), n.name.clone()))
        .collect();
    if !select_only {
        for id in &group.members {
            if !store.nodes.iter().any(|n| &n.id == id) {
                rows.push((
                    id.clone(),
                    format!(
                        "Unavailable node {} · uncheck to repair",
                        id.chars().take(8).collect::<String>()
                    ),
                ));
            }
        }
    }
    rows.retain(|(_, name)| name.to_lowercase().contains(&filter.to_lowercase()));
    rows
}
fn tab_labels(zh: bool, width: u16) -> [&'static str; 8] {
    if zh {
        [
            "概览", "节点", "订阅", "配置", "活动", "分组", "规则", "连接",
        ]
    } else if width < 116 {
        [
            "Home", "Nodes", "Subs", "Cfg", "Log", "Groups", "Rules", "Conns",
        ]
    } else {
        [
            "Overview",
            "Nodes",
            "Subscriptions",
            "Config",
            "Activity",
            "Groups",
            "Rules",
            "Connections",
        ]
    }
}
fn help(zh: bool) -> String {
    let mut guide: String = if zh {
        "1–5 / Tab       切换页面\n↑↓ / j k        移动选择\na               添加订阅\nc               连接 / 应用已保存配置\nd               断开连接（需确认）\nq               退出界面；后台保持运行\nL               切换语言\n\n节点页：Enter 选择，/ 搜索，f 收藏，t 测当前，T 测全部\n订阅页：r 更新并预览，x 删除\n配置页：Enter 编辑，r 新增规则，s 校验，p 预览，b 恢复\n表单：Tab 下一项，空格切换选项，Ctrl+S / F2 保存，Esc 取消\n\n端口模式：在应用中设置 HTTP/SOCKS 代理 127.0.0.1:2080。\nTUN 模式：需要管理员权限，会修改当前主机路由。\nSSH：始终操作远端主机，不是你自己的电脑。\n订阅和凭据仅保存到当前主机的私有应用目录。".into()
    } else {
        "1–5 / Tab       Switch page\nArrows / j k    Move selection\na               Add subscription\nc               Connect / apply saved configuration\nd               Disconnect (confirmation required)\nq               Exit interface; core keeps running\nL               Switch language\n\nNodes: Enter select · / search · f favorite · t test · T test all\nSubscriptions: r refresh with preview · x remove\nConfig: Enter edit · r add rule · s check · p preview · b restore\nForms: Tab next · Space cycle · Ctrl+S / F2 save · Esc cancel\n\nPort mode: set HTTP/SOCKS proxy in your app to 127.0.0.1:2080.\nTUN: administrator permission; changes routes on THIS host.\nSSH: controls the remote host, not the computer in your lap.\nSubscriptions and credentials stay in this host's private app directory.\n\nPort mode does not change macOS/Linux system proxy settings.\nManager survives closing this TUI, but does not autostart at login yet.".into()
    };
    guide = guide.replace("1–5", "1–8");
    guide.push_str(tr(zh,"\n\nM: routing mode · D: local capture / routing / DNS diagnostics\n8 Connections: actual core routes · Enter details · / search\nh includes recent closed · x close one (confirmation) · r refresh\nMode changes stay saved until c applies, restarting the core.\nGlobal mode keeps the private-IP exception unless explicitly disabled.","\n\nM 路由模式 · D 本地接管 / 路由 / DNS 诊断\n8 连接：实际核心路由 · Enter 详情 · / 搜索\nh 含最近关闭 · x 关闭单条（需确认）· r 刷新\n模式保存后须按 c 应用，会重启核心。\n全局代理默认保留内网直连例外，可明确关闭。"));
    guide.push_str(tr(zh, "\n\n6 Groups: a create · e name/type · m members · Enter select node\nMembers: Space toggle · / search · Ctrl+S / F2 save\n7 Rules: a import · Enter target · Space enable · r refresh\n[ / ] reorder · p details/warnings · x remove · c apply\nRules run after private-IP handling and manual rules.\nDNS: Config → Enter → DNS policy: paired follows domain targets.\nOnly DNS handled by sing-box is affected; no system DNS takeover.\nProcess rules require traffic capture and OS process visibility.", "\n\n6 分组：a 创建 · e 名称/类型 · m 成员 · Enter 选择节点\n成员：空格勾选 · / 搜索 · Ctrl+S / F2 保存\n7 规则：a 导入 · Enter 目标 · 空格启停 · r 更新\n[ / ] 调序 · p 详情/警告 · x 删除 · c 应用\n规则集位于内网直连和手工规则之后。\n配置 → Enter → DNS policy：paired 跟随域名分流目标。\n只影响交给 sing-box 的 DNS，不接管系统 DNS。\n进程规则要求流量进入核心且系统能识别进程。"));
    guide.push_str(tr(zh,
        "\n\nSystem mode (macOS): s → Capture mode → system → save → c.\nBacks up HTTP/HTTPS/SOCKS and PAC settings before takeover.\nd restores original settings BEFORE stopping; q keeps proxy running.\nR restores system proxy without stopping the local port.\nv checks one HTTPS endpoint through the local proxy (confirmation required).\nSome applications ignore system proxy settings; this is not TUN.\nRecovery problems: R or sing --restore-system-proxy.\nNo automatic Linux desktop proxy setup; SSH uses Port mode.",
        "\n\n系统模式（macOS）：s → Capture mode → system → 保存 → c。\n接管前备份 HTTP/HTTPS/SOCKS 与 PAC 设置。\nd 先恢复原设置再停止；q 退出界面但保持代理。\nR 恢复系统代理但不停止本地端口。\nv 确认后通过本地代理检查一个 HTTPS 测试地址。\n部分应用忽略系统代理设置；它不是 TUN。\n恢复异常：R 或 sing --restore-system-proxy。\nLinux 暂不自动设置桌面代理；SSH 使用端口模式。"));
    guide
        .replace(
            "Config → Enter → DNS policy",
            "Config → Enter → 3 DNS → policy",
        )
        .replace("配置 → Enter → DNS policy", "配置 → Enter → 3 DNS → policy")
        .replace("s → Capture mode", "s → 1 Capture → mode")
}

fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    f.render_widget(Block::default().style(Style::default().bg(BG).fg(FG)), area);
    if area.width < 54 || area.height < 18 {
        f.render_widget(
            Paragraph::new(
                "Terminal too small\nResize to at least 54 × 18.\nq: exit (core keeps running)",
            )
            .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    let z = app.zh();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Min(8),
            Constraint::Length(2),
            Constraint::Length(1),
        ])
        .split(area);
    let status =
        if app.snapshot.system_proxy.pending_restore && !app.snapshot.system_proxy.configured {
            tr(z, "PROXY RECOVERY / REVIEW NEEDED", "系统代理需恢复 / 检查")
        } else if app.snapshot.connected {
            if app.snapshot.api_ready {
                match app
                    .snapshot
                    .running_settings
                    .as_ref()
                    .map(|s| s.mode.as_str())
                    .unwrap_or("port")
                {
                    "tun" => tr(z, "TUN CORE READY", "TUN 核心就绪"),
                    "system"
                        if app.snapshot.system_proxy.configured
                            && app.snapshot.system_proxy.effective =>
                    {
                        tr(z, "SYSTEM PROXY ON", "系统代理已启用")
                    }
                    "system" if app.snapshot.system_proxy.configured => tr(
                        z,
                        "PROXY SET · ROUTING UNVERIFIED",
                        "系统代理已配置 · 生效待确认",
                    ),
                    _ => tr(
                        z,
                        "PORT READY · NO SYSTEM TAKEOVER",
                        "端口就绪 · 未接管系统",
                    ),
                }
            } else {
                tr(z, "CORE RUNNING · API UNAVAILABLE", "核心运行 · API 不可用")
            }
        } else {
            tr(z, "CORE STOPPED", "核心未运行")
        };
    let head = Line::from(vec![
        Span::styled(
            "  sing ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" / ", Style::default().fg(MUTED)),
        Span::raw(format!(
            "{}{}",
            app.snapshot.host,
            if app.snapshot.ssh { " · SSH" } else { "" }
        )),
    ]);
    f.render_widget(
        Paragraph::new(vec![
            head,
            Line::styled(
                format!("  {}{}", if app.demo { "DEMO · " } else { "" }, status),
                Style::default().fg(if app.snapshot.connected { ACCENT } else { WARN }),
            ),
        ])
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(MUTED)),
        ),
        rows[0],
    );
    let labels = tab_labels(z, f.area().width);
    let mut tabs = vec![];
    for (i, label) in labels.iter().enumerate() {
        if f.area().width < 88 && i != app.page {
            continue;
        }
        tabs.push(Span::styled(
            format!(" {} {} ", i + 1, label),
            if i == app.page {
                Style::default()
                    .fg(BG)
                    .bg(ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(MUTED)
            },
        ));
        tabs.push(Span::raw(" "));
    }
    if f.area().width < 88 {
        tabs.push(Span::styled(
            " 1–8 / Tab · pages",
            Style::default().fg(MUTED),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(tabs)), rows[1]);
    match app.page {
        0 => overview(f, app, rows[2]),
        1 => nodes(f, app, rows[2]),
        2 => subscriptions(f, app, rows[2]),
        3 => settings(f, app, rows[2]),
        5 => groups(f, app, rows[2]),
        6 => rule_bindings(f, app, rows[2]),
        7 => connections(f, app, rows[2]),
        _ => activity(f, app, rows[2]),
    }
    let notice = if app.busy {
        format!(
            "{} {}",
            ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"][app.tick % 8],
            tr(
                z,
                "Working… interface remains responsive; Ctrl+q exits",
                "处理中… Ctrl+q 可退出界面"
            )
        )
    } else {
        app.notice.clone()
    };
    f.render_widget(
        Paragraph::new(notice)
            .style(Style::default().fg(if app.error { ERROR } else { ACCENT }))
            .wrap(Wrap { trim: true }),
        rows[3],
    );
    let footer = match app.page {
        0 => tr(
            z,
            "a add  c start  d stop  s settings  v check  R restore  ? help",
            "a 添加  c 连接  d 断开  s 设置  v 检查  R 恢复  ? 帮助",
        ),
        1 => "Enter select  / search  f favorite  t test  T test all  c apply  ? help",
        2 => "a add  r refresh  x remove  c apply  ? help  q exit",
        3 => "Enter edit  r rule  x delete rule  s check  p preview  c apply  b undo",
        5 => "a create  e edit  m members  Enter select  x remove  c apply  ? help",
        6 => "a import  Enter target  Space on/off  [ ] order  r refresh  p info  c apply",
        7 => tr(
            z,
            "Enter details  / search  h recent  x close  r refresh  D diagnose  ? help",
            "Enter 详情  / 搜索  h 最近关闭  x 关闭  r 刷新  D 诊断  ? 帮助",
        ),
        _ => "l core logs  v HTTPS check  R restore proxy  d stop  q exit",
    };
    let footer = if app.page == 0 {
        tr(
            z,
            "c apply  d stop  s settings  M mode  D diagnose  v check  ? help  q exit",
            "c 应用  d 断开  s 设置  M 模式  D 诊断  v 检查  ? 帮助",
        )
    } else {
        footer
    };
    f.render_widget(
        Paragraph::new(footer).style(Style::default().fg(MUTED)),
        rows[4],
    );
    if let Some(dialog) = &app.dialog {
        draw_dialog(f, app, dialog);
    }
}

fn overview(f: &mut Frame, app: &App, area: Rect) {
    let z = app.zh();
    let s = &app.snapshot;
    let saved = &s.store.settings;
    let effective = s
        .running_settings
        .as_ref()
        .filter(|_| s.connected)
        .unwrap_or(saved);
    let chunks = Layout::default()
        .constraints([
            Constraint::Length(if area.height > 18 { 9 } else { 8 }),
            Constraint::Min(5),
        ])
        .split(area);
    if s.store.nodes.is_empty() && effective.route_mode != "direct" {
        let text = vec![
            Line::from(""),
            Line::styled(
                tr(z, "  Your connection starts here.", "  从导入订阅开始。"),
                Style::default().fg(FG).add_modifier(Modifier::BOLD),
            ),
            Line::from(tr(
                z,
                "  Paste a subscription link. We will recognize its format.",
                "  粘贴订阅链接，由客户端自动识别格式。",
            )),
            Line::from(tr(
                z,
                "  No configuration file to write. No external converter.",
                "  不必手写配置，不经过外部转换服务。",
            )),
            Line::styled(
                tr(
                    z,
                    "  [a] Add subscription     [i] Install sing-box",
                    "  [a] 添加订阅     [i] 安装 sing-box",
                ),
                Style::default().fg(ACCENT),
            ),
        ];
        f.render_widget(
            Paragraph::new(text)
                .block(block(tr(z, "Welcome", "欢迎")))
                .wrap(Wrap { trim: false }),
            chunks[0],
        );
    } else {
        let (target_label, choice) = home_route(s);
        let text = vec![
            Line::styled(
                format!(
                    "  {}: {} · {}",
                    if s.connected {
                        tr(z, "LIVE", "当前生效")
                    } else {
                        tr(z, "SAVED", "已保存")
                    },
                    option_label(z, &effective.route_mode),
                    if effective.bypass_lan {
                        tr(z, "private IPs direct", "内网 IP 直连")
                    } else {
                        tr(z, "no private-IP exception", "内网直连例外关闭")
                    }
                ),
                Style::default().fg(ACCENT),
            ),
            Line::from(vec![
                Span::styled(format!("  {target_label}  "), Style::default().fg(MUTED)),
                Span::styled(
                    choice,
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(format!(
                "  {} {} · {} {}",
                s.store.nodes.len(),
                tr(z, "nodes", "个节点"),
                s.store.subscriptions.len(),
                tr(z, "subscriptions", "个订阅")
            )),
            Line::from(format!(
                "  ↓ {}/s     ↑ {}/s     {} connections",
                bytes(s.status.downlink),
                bytes(s.status.uplink),
                s.status.connections_out
            )),
            Line::styled(
                format!(
                    "  {}: {}  [v]",
                    tr(z, "HTTPS check", "HTTPS 检查"),
                    match s.connectivity.state.as_str() {
                        "passed" => tr(z, "passed via local proxy", "本地代理路径通过"),
                        "failed" => tr(z, "failed · retry with v", "失败 · 按 v 重试"),
                        _ => tr(z, "not checked", "尚未检查"),
                    }
                ),
                Style::default().fg(if s.connectivity.state == "failed" {
                    ERROR
                } else {
                    MUTED
                }),
            ),
            Line::styled(
                if s.dirty {
                    tr(
                        z,
                        "  Saved changes waiting · press c to apply",
                        "  有未应用的更改 · 按 c 应用",
                    )
                } else {
                    tr(
                        z,
                        "  M routing · 6 group nodes · 7 rule targets · c apply",
                        "  M 路由 · 6 组内节点 · 7 规则目标 · c 应用",
                    )
                },
                Style::default().fg(if s.dirty { WARN } else { MUTED }),
            ),
        ];
        f.render_widget(
            Paragraph::new(text)
                .block(block(tr(z, "Connection", "连接")))
                .wrap(Wrap { trim: false }),
            chunks[0],
        );
    }
    let mode = s
        .running_settings
        .as_ref()
        .filter(|_| s.connected)
        .unwrap_or(saved);
    let mut text = vec![
        Line::from(format!(
            "  {}: {}",
            tr(z, "Core", "核心"),
            if s.core.is_empty() {
                tr(z, "Not installed · press i", "未安装 · 按 i 安装")
            } else {
                &s.core
            }
        )),
        Line::from(format!(
            "  {}: {}",
            tr(z, "Native API", "原生管理接口"),
            if s.api_ready {
                tr(z, "gRPC · ready", "gRPC · 就绪")
            } else {
                tr(z, "offline", "离线")
            }
        )),
        Line::styled(format!("  {}", s.version), Style::default().fg(MUTED)),
        Line::from(""),
        Line::styled(
            format!(
                "  {} · 127.0.0.1:{}",
                if mode.mode == "tun" {
                    "TUN + HTTP / SOCKS"
                } else {
                    "HTTP / SOCKS proxy"
                },
                mode.port
            ),
            Style::default().fg(ACCENT),
        ),
    ];
    if mode.mode == "port" {
        text.push(Line::from(tr(
            z,
            "  Set this proxy in your app; system traffic is not captured.",
            "  在应用中设置此代理；不会自动接管系统流量。",
        )));
        text.push(Line::from(format!(
            "  export https_proxy=http://127.0.0.1:{}",
            mode.port
        )));
    }
    if mode.mode == "system" {
        text.push(Line::styled(
            if s.system_proxy.configured && s.system_proxy.effective {
                tr(
                    z,
                    "  System proxy ON · apps must honor system settings",
                    "  系统代理已启用 · 仅覆盖遵循系统设置的应用",
                )
            } else if !s.connected && !s.system_proxy.pending_restore {
                tr(
                    z,
                    "  System proxy OFF · c enables saved mode",
                    "  系统代理已关闭 · c 启用已保存模式",
                )
            } else {
                tr(
                    z,
                    "  System takeover NOT verified · R restores original settings",
                    "  系统接管尚未确认 · R 恢复原设置",
                )
            },
            Style::default().fg(if s.system_proxy.effective {
                ACCENT
            } else {
                WARN
            }),
        ));
        if !s.system_proxy.services.is_empty() {
            text.push(Line::from(format!(
                "  Services: {}",
                s.system_proxy.services.join(", ")
            )));
        }
    }
    if s.system_proxy.pending_restore && !s.system_proxy.configured {
        text.push(Line::styled(
            format!("  {}", s.system_proxy.detail),
            Style::default().fg(WARN),
        ));
    }
    text.extend([
        Line::from(""),
        Line::styled(
            tr(
                z,
                "  q closes the UI. The connection stays running.",
                "  q 仅关闭界面，连接继续在后台运行。",
            ),
            Style::default().fg(MUTED),
        ),
    ]);
    if s.ssh {
        text.push(Line::styled(
            tr(
                z,
                "  SSH: all operations affect THIS REMOTE HOST.",
                "  SSH：所有操作都作用于当前远程主机。",
            ),
            Style::default().fg(WARN),
        ));
    }
    f.render_widget(
        Paragraph::new(text)
            .block(block(tr(z, "On this host", "当前主机")))
            .wrap(Wrap { trim: false }),
        chunks[1],
    );
}
fn nodes(f: &mut Frame, app: &App, area: Rect) {
    let wide = area.width >= 100;
    let parts = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(if wide {
            vec![Constraint::Percentage(68), Constraint::Percentage(32)]
        } else {
            vec![Constraint::Percentage(100)]
        })
        .split(area);
    let nodes = app.nodes();
    let lines: Vec<ListItem> = nodes
        .iter()
        .map(|n| {
            let chosen = app.snapshot.store.selected.as_ref() == Some(&n.id);
            let delay = app
                .snapshot
                .groups
                .group
                .iter()
                .flat_map(|g| g.items.iter())
                .find(|i| i.tag == n.tag())
                .filter(|i| i.delay > 0)
                .map(|i| format!("{} ms", i.delay))
                .unwrap_or_else(|| "—".into());
            ListItem::new(Line::from(vec![
                Span::styled(
                    if chosen { " ● " } else { "   " },
                    Style::default().fg(ACCENT),
                ),
                Span::styled(
                    if n.favorite { "★ " } else { "  " },
                    Style::default().fg(WARN),
                ),
                Span::raw(format!("{}  ", n.name)),
                Span::styled(format!("{}  {delay}", n.kind()), Style::default().fg(MUTED)),
            ]))
        })
        .collect();
    let title = if app.searching {
        format!("Search: {}▏", app.filter)
    } else {
        format!(
            "{} · {}{}",
            tr(app.zh(), "Nodes", "节点"),
            nodes.len(),
            if app.filter.is_empty() {
                String::new()
            } else {
                format!(" / {}", app.filter)
            }
        )
    };
    let mut state =
        ListState::default().with_selected((!nodes.is_empty()).then_some(app.selected[1]));
    f.render_stateful_widget(
        List::new(lines)
            .block(block(title))
            .highlight_style(Style::default().bg(Color::Rgb(38, 62, 68)))
            .highlight_symbol("▎"),
        parts[0],
        &mut state,
    );
    if nodes.is_empty() {
        f.render_widget(
            Paragraph::new(tr(
                app.zh(),
                "\n  No matching nodes. Press a to import, Esc to clear search.",
                "\n  暂无匹配节点。按 a 导入，Esc 清除搜索。",
            ))
            .wrap(Wrap { trim: false }),
            block("").inner(parts[0]),
        );
    }
    if wide {
        let text=nodes.get(app.selected[1]).map(|n|{let provider=app.snapshot.store.subscriptions.iter().find(|s|s.id==n.provider).map(|s|s.name.as_str()).unwrap_or("—");format!("\n{}\n\nProtocol  {}\nServer    {}\nProvider  {}\n\nEnter  Select node\nt      Test latency\nf      Toggle favorite\n\nLatency is not a download speed test.\nUnmeasured / failed: —",n.name,n.kind(),n.server(),provider)}).unwrap_or_default();
        f.render_widget(
            Paragraph::new(text)
                .block(block(tr(app.zh(), "Details", "详情")))
                .wrap(Wrap { trim: false }),
            parts[1],
        );
    }
}
fn subscriptions(f: &mut Frame, app: &App, area: Rect) {
    let z = app.zh();
    let lines: Vec<ListItem> = app
        .snapshot
        .store
        .subscriptions
        .iter()
        .map(|s| {
            let count = app
                .snapshot
                .store
                .nodes
                .iter()
                .filter(|n| n.provider == s.id)
                .count();
            ListItem::new(vec![
                Line::styled(
                    format!(" {} · {count} nodes · {}", s.name, s.format),
                    Style::default().fg(FG),
                ),
                Line::styled(
                    format!(
                        " {} · updated {}s ago · {} warnings",
                        s.source,
                        model::now().saturating_sub(s.updated_at),
                        s.warnings.len()
                    ),
                    Style::default().fg(MUTED),
                ),
                Line::from(""),
            ])
        })
        .collect();
    let mut state =
        ListState::default().with_selected((!lines.is_empty()).then_some(app.selected[2]));
    f.render_stateful_widget(
        List::new(lines)
            .block(block(tr(
                z,
                "Subscriptions · refresh is previewed before saving",
                "订阅 · 更新先预览后保存",
            )))
            .highlight_style(Style::default().bg(Color::Rgb(38, 62, 68)))
            .highlight_symbol("▎"),
        area,
        &mut state,
    );
    if app.snapshot.store.subscriptions.is_empty() {
        f.render_widget(Paragraph::new(tr(z,"\n  Add your first subscription with a.\n\n  Supported input: URI list, Base64, Clash YAML, sing-box JSON.\n  URLs stay private. No third-party conversion service.","\n  按 a 添加第一份订阅。\n\n  支持 URI 列表、Base64、Clash YAML、sing-box JSON。\n  链接私有保存，不发送到第三方转换服务。")).wrap(Wrap{trim:false}),block("").inner(area));
    }
}
fn settings(f: &mut Frame, app: &App, area: Rect) {
    let s = &app.snapshot.store.settings;
    let z = app.zh();
    let mut lines = vec![
        format!(" Capture mode       {}", s.mode),
        format!(
            " Routing mode       {} (M to edit; c applies)",
            s.route_mode
        ),
        format!(
            " Global proxy       {}",
            target_name(&app.snapshot.store, &s.global_target)
        ),
        format!(
            " Private IP direct  {}",
            if s.bypass_lan { "on" } else { "off" }
        ),
        format!(
            " Rule fallback      {} (not a global mode switch)",
            target_name(&app.snapshot.store, &s.routing)
        ),
        format!(" HTTPS DNS          {}", s.dns),
        format!(" Proxy port         127.0.0.1:{}", s.port),
        format!(" gRPC port          127.0.0.1:{}", s.api_port),
        format!(
            " Core               {}",
            if s.core.is_empty() {
                "Auto-detect"
            } else {
                &s.core
            }
        ),
        format!(" Language           {}", s.language),
        format!(
            " DNS policy         {} (paired: domain rules only)",
            s.dns_policy
        ),
        format!(" DNS preference     {}", s.dns_strategy),
        " Saved manual rules · used in rule mode".into(),
    ];
    for r in &s.rules {
        lines.push(format!(
            " {}  {} → {}",
            r.kind,
            r.value,
            target_name(&app.snapshot.store, &r.target)
        ));
    }
    let items: Vec<_> = lines.into_iter().map(ListItem::new).collect();
    let parts = Layout::default()
        .constraints([Constraint::Min(8), Constraint::Length(4)])
        .split(area);
    let mut state = ListState::default().with_selected(Some(app.selected[3]));
    f.render_stateful_widget(
        List::new(items)
            .block(block(tr(
                z,
                "Configuration assistant · saved settings",
                "配置辅助 · 已保存设置",
            )))
            .highlight_style(Style::default().bg(Color::Rgb(38, 62, 68)))
            .highlight_symbol("▎"),
        parts[0],
        &mut state,
    );
    f.render_widget(Paragraph::new(tr(z,"Enter: edit fields   r: add a rule   p: view generated JSON\nChanges are saved separately from the running core.\ns checks syntax; c applies with startup verification and rollback.","Enter 编辑设置   r 添加规则   p 查看生成的 JSON\n保存设置不会立刻改变正在运行的核心。\ns 校验配置；c 应用，启动失败尝试恢复原配置。")).style(Style::default().fg(MUTED)).wrap(Wrap{trim:false}),parts[1]);
}
fn groups(f: &mut Frame, app: &App, area: Rect) {
    let store = &app.snapshot.store;
    let parts = Layout::default()
        .constraints([Constraint::Min(3), Constraint::Length(4)])
        .split(area);
    let items: Vec<_> = store
        .proxy_groups
        .iter()
        .map(|g| {
            let missing = g
                .members
                .iter()
                .filter(|id| !store.nodes.iter().any(|n| &n.id == *id))
                .count();
            let live = app
                .snapshot
                .groups
                .group
                .iter()
                .find(|live| live.tag == g.tag());
            let chosen = live
                .map(|g| g.selected.trim_start_matches("n-"))
                .or(g.selected.as_deref());
            let name = chosen
                .and_then(|id| store.nodes.iter().find(|n| n.id == id))
                .map(|n| n.name.as_str())
                .unwrap_or("—");
            ListItem::new(vec![
                Line::from(format!(
                    " {} · {} · {} members",
                    g.name,
                    if g.kind == "urltest" {
                        "Auto latency"
                    } else {
                        "Manual"
                    },
                    g.members.len()
                )),
                Line::styled(
                    format!(
                        " {}: {}{}",
                        if live.is_some() { "Live" } else { "Saved" },
                        name,
                        if missing > 0 {
                            format!(" · {missing} unavailable! Press m")
                        } else {
                            String::new()
                        }
                    ),
                    Style::default().fg(if missing > 0 { WARN } else { MUTED }),
                ),
                Line::from(""),
            ])
        })
        .collect();
    let mut state =
        ListState::default().with_selected((!items.is_empty()).then_some(app.selected[5]));
    f.render_stateful_widget(
        List::new(items)
            .block(block(tr(
                app.zh(),
                "Groups · destinations for your rules",
                "分组 · 规则的分流目标",
            )))
            .highlight_style(Style::default().bg(Color::Rgb(38, 62, 68)))
            .highlight_symbol("▎"),
        parts[0],
        &mut state,
    );
    if store.proxy_groups.is_empty() {
        f.render_widget(Paragraph::new(tr(app.zh(),"\n  Press a to create a group, then check its nodes.\n  Example: YouTube → a few nodes you trust.\n  The built-in proxy target uses the Nodes page selection.","\n  按 a 创建分组，再勾选节点。\n  例如：YouTube → 你希望用于视频的几个节点。\n  内置 proxy 目标使用节点页当前选择。")).wrap(Wrap{trim:false}),block("").inner(parts[0]));
    }
    f.render_widget(Paragraph::new(tr(app.zh(),"Manual: Enter chooses a node. Auto: HTTPS latency, not speed.\nMembership is explicit; new subscription nodes are not auto-added.\nMissing members block Apply. Save first, then c applies.","手动组：Enter 选择节点；自动组：按 HTTPS 延迟，不是带宽。\n订阅新增节点不会自动加入；缺失成员需按 m 修复。\n保存分组后按 c 应用。")).style(Style::default().fg(MUTED)).wrap(Wrap{trim:false}),parts[1]);
}
fn rule_bindings(f: &mut Frame, app: &App, area: Rect) {
    let store = &app.snapshot.store;
    let parts = Layout::default()
        .constraints([Constraint::Min(3), Constraint::Length(4)])
        .split(area);
    let items: Vec<_> = store
        .rule_bindings
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let r = store.rule_resources.iter().find(|r| r.id == b.resource);
            ListItem::new(vec![
                Line::from(format!(
                    " {} {} {} → {}",
                    i + 1,
                    if b.enabled { "●" } else { "○" },
                    r.map(|r| r.name.as_str()).unwrap_or("Missing resource"),
                    target_name(store, &b.target)
                )),
                Line::styled(
                    r.map(|r| {
                        format!(
                            "   {} · {} matches · {} warnings · {}s ago",
                            r.format,
                            r.rules.len(),
                            r.warnings.len(),
                            model::now().saturating_sub(r.updated_at)
                        )
                    })
                    .unwrap_or_default(),
                    Style::default().fg(MUTED),
                ),
                Line::from(""),
            ])
        })
        .collect();
    let mut state =
        ListState::default().with_selected((!items.is_empty()).then_some(app.selected[6]));
    f.render_stateful_widget(
        List::new(items)
            .block(block(
                if app
                    .snapshot
                    .running_settings
                    .as_ref()
                    .filter(|_| app.snapshot.connected)
                    .unwrap_or(&store.settings)
                    .route_mode
                    == "rule"
                {
                    tr(
                        app.zh(),
                        "Saved rule subscriptions · first match wins",
                        "已保存规则订阅 · 从上往下首次命中",
                    )
                } else {
                    tr(
                        app.zh(),
                        "Saved rules · INACTIVE in current mode",
                        "已保存规则 · 当前模式不使用",
                    )
                },
            ))
            .highlight_style(Style::default().bg(Color::Rgb(38, 62, 68)))
            .highlight_symbol("▎"),
        parts[0],
        &mut state,
    );
    if store.rule_bindings.is_empty() {
        f.render_widget(Paragraph::new(tr(app.zh(),"\n  Press a, paste a classification-list URL, choose a target.\n  QX / Clash payload / native JSON / domain / CIDR lists.\n  Conversion happens locally. Review unsupported entries first.","\n  按 a 粘贴分类规则链接，选择分流目标。\n  支持 QX / Clash payload / 原生 JSON / 域名 / CIDR 子集。\n  本地转换，先预览不支持的条目。")).wrap(Wrap{trim:false}),block("").inner(parts[0]));
    }
    f.render_widget(
        Paragraph::new(format!(
            "{}\n{}: {}\n{}",
            if store.settings.bypass_lan {
                tr(
                    app.zh(),
                    "Rule mode: private IP → manual → this list → fallback",
                    "规则模式：内网直连 → 手工规则 → 此列表 → 默认",
                )
            } else {
                tr(
                    app.zh(),
                    "Rule mode: manual → this list → fallback (no LAN exception)",
                    "规则模式：手工规则 → 此列表 → 默认（内网例外关闭）",
                )
            },
            tr(app.zh(), "Default", "默认"),
            target_name(store, &store.settings.routing),
            tr(
                app.zh(),
                "r previews an update; old rules stay until you confirm. c applies.",
                "r 更新先预览，确认前保留旧规则；c 应用。"
            )
        ))
        .style(Style::default().fg(MUTED))
        .wrap(Wrap { trim: false }),
        parts[1],
    );
}
fn connection_details(store: &Store, c: &crate::api::Connection) -> String {
    let zh = store.settings.language == "zh";
    let unknown = tr(zh, "unavailable", "无法识别");
    let known = |value: &str| {
        if value.is_empty() {
            unknown.to_string()
        } else {
            value.to_string()
        }
    };
    let mut text = format!(
        "{}\n{}\n\n",
        tr(zh, "Observed by sing-box", "核心实际观察"),
        if c.closed_at == 0 {
            tr(zh, "Active at last sample", "上次采样时活跃")
        } else {
            tr(zh, "Recently closed", "最近已关闭")
        }
    );
    let fields = [
        (tr(zh, "Destination", "目标"), known(&c.destination)),
        (tr(zh, "Domain", "域名"), known(&c.domain)),
        (tr(zh, "Source", "来源"), known(&c.source)),
        (
            tr(zh, "Inbound", "入站"),
            format!("{} ({})", known(&c.inbound), known(&c.inbound_type)),
        ),
        (
            tr(zh, "Network", "网络"),
            format!("{} · {}", known(&c.network), known(&c.protocol)),
        ),
        (
            tr(zh, "Rule", "命中规则"),
            if c.rule.is_empty() {
                tr(
                    zh,
                    "not reported (may be default route)",
                    "未报告，可能为默认路由",
                )
                .into()
            } else {
                c.rule.clone()
            },
        ),
        (
            tr(zh, "Outbound", "出口"),
            format!(
                "{} ({})",
                config::target_label(store, &c.outbound),
                known(&c.outbound_type)
            ),
        ),
        (
            tr(zh, "Core-reported chain", "核心报告的链"),
            if c.chain.is_empty() {
                tr(zh, "not reported", "未报告").into()
            } else {
                c.chain
                    .iter()
                    .map(|t| config::target_label(store, t))
                    .collect::<Vec<_>>()
                    .join(" → ")
            },
        ),
        (
            tr(zh, "Process", "进程"),
            c.process
                .as_ref()
                .map(|p| format!("{} (pid {})", known(&p.path), p.pid))
                .unwrap_or_else(|| unknown.into()),
        ),
        (
            tr(zh, "Total", "累计"),
            format!("↓ {}  ↑ {}", bytes(c.downlink_total), bytes(c.uplink_total)),
        ),
    ];
    for (label, value) in fields {
        text.push_str(&format!("{label}: {value}\n"));
    }
    text.push_str(tr(zh,"\nMissing metadata means unavailable, not a guessed match.\nFull host/path details can be private. Avoid sharing screenshots.","\n缺失的信息标为未知，不猜测规则命中或 DNS 耗时。\n域名和进程路径可能敏感，请勿公开分享截图。"));
    text
}
fn connections(f: &mut Frame, app: &App, area: Rect) {
    let parts = Layout::default()
        .constraints([Constraint::Min(3), Constraint::Length(4)])
        .split(area);
    let rows = app.connection_rows();
    let items: Vec<_> = rows
        .iter()
        .map(|c| {
            ListItem::new(vec![
                Line::from(format!(
                    " {} {}  {} → {}",
                    if c.closed_at == 0 { "●" } else { "○" },
                    c.network,
                    if c.domain.is_empty() {
                        &c.destination
                    } else {
                        &c.domain
                    },
                    config::target_label(&app.snapshot.store, &c.outbound)
                )),
                Line::styled(
                    format!(
                        "   ↓ {} ↑ {} · {}",
                        bytes(c.downlink_total),
                        bytes(c.uplink_total),
                        if c.rule.is_empty() {
                            tr(app.zh(), "default / not reported", "默认路由 / 未报告")
                        } else {
                            &c.rule
                        }
                    ),
                    Style::default().fg(MUTED),
                ),
            ])
        })
        .collect();
    let title = if app.searching {
        format!(
            "{}: {}▏",
            tr(app.zh(), "Search connections", "搜索连接"),
            app.connection_filter
        )
    } else {
        format!(
            "{} · {}{}",
            tr(app.zh(), "Connections", "连接"),
            rows.len(),
            if app.show_closed {
                tr(app.zh(), " · includes recent closed", " · 含最近关闭")
            } else {
                tr(app.zh(), " · active sample", " · 活跃采样")
            }
        )
    };
    let mut state =
        ListState::default().with_selected((!items.is_empty()).then_some(app.selected[7]));
    f.render_stateful_widget(
        List::new(items)
            .block(block(title))
            .highlight_style(Style::default().bg(Color::Rgb(38, 62, 68)))
            .highlight_symbol("▎"),
        parts[0],
        &mut state,
    );
    if rows.is_empty() {
        f.render_widget(Paragraph::new(tr(app.zh(),"\n  No matching connections in this sample.\n  Open a website, or press h to include recent closed.\n  Traffic bypassing sing-box cannot appear here.","\n  本次采样没有匹配连接。\n  打开网站，或按 h 显示最近关闭的连接。\n  未经过 sing-box 的流量不会出现在这里。")).wrap(Wrap{trim:false}),block("").inner(parts[0]));
    }
    let age = model::now().saturating_sub(app.connections.observed_at);
    let status = if !app.connections_error.is_empty() {
        format!(
            "{}: {}",
            tr(app.zh(), "Refresh failed", "刷新失败"),
            app.connections_error
        )
    } else if app.connections.observed_at == 0 {
        tr(app.zh(), "Not sampled yet · r refresh", "尚未采样 · r 刷新").into()
    } else {
        format!(
            "{} · {} {} · {}: {} (≤500)",
            if age > 5 {
                tr(app.zh(), "STALE", "已过期")
            } else {
                tr(app.zh(), "Sample", "采样")
            },
            age,
            tr(app.zh(), "seconds ago", "秒前"),
            tr(app.zh(), "retained records", "保留记录"),
            app.connections.total
        )
    };
    f.render_widget(Paragraph::new(format!("{status}\n{}",tr(app.zh(),"Refreshes every 2s here; not complete history. Enter: actual metadata.\nClosing one connection does not prevent the app reconnecting.","此页每 2 秒采样，不是完整历史。Enter 查看实际元数据。\n关闭单条连接不阻止应用自动重连。"))).style(Style::default().fg(if age > 5 || !app.connections_error.is_empty() {WARN} else {MUTED})).wrap(Wrap{trim:false}),parts[1]);
}
fn activity(f: &mut Frame, app: &App, area: Rect) {
    let text = app
        .snapshot
        .activity
        .iter()
        .rev()
        .map(|s| Line::from(format!(" {s}")))
        .collect::<Vec<_>>();
    f.render_widget(
        Paragraph::new(text)
            .scroll((app.scroll, 0))
            .wrap(Wrap { trim: false })
            .block(block(tr(
                app.zh(),
                "Manager activity · newest first",
                "管理器活动 · 最新在前",
            ))),
        area,
    );
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width.saturating_sub(4));
    let h = height.min(area.height.saturating_sub(2));
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}
fn draw_dialog(f: &mut Frame, app: &App, dialog: &Dialog) {
    let area = centered(
        f.area(),
        86,
        match dialog {
            Dialog::Form { fields, .. } => fields.len() as u16 * 3 + 7,
            Dialog::Import(p, _) => p.names.len() as u16 + p.warnings.len().min(5) as u16 + 10,
            Dialog::Text { .. } => f.area().height.saturating_sub(4),
            Dialog::Members { .. } | Dialog::RulesReview(..) => f.area().height.saturating_sub(2),
            _ => 18,
        },
    );
    f.render_widget(Clear, area);
    let inner = Rect::new(
        area.x + 2,
        area.y + 2,
        area.width.saturating_sub(4),
        area.height.saturating_sub(4),
    );
    match dialog {
        Dialog::SettingsMenu(selected) => {
            f.render_widget(block(tr(app.zh(), "Configuration", "配置")), area);
            let items: Vec<_> = [
                (
                    tr(app.zh(), "1  Capture", "1  接管"),
                    tr(app.zh(), "Which apps reach sing?", "哪些流量进入 sing？"),
                ),
                (
                    tr(app.zh(), "2  Routing mode", "2  路由模式"),
                    tr(
                        app.zh(),
                        "Rules, global proxy, direct",
                        "规则分流、全局代理、直连",
                    ),
                ),
                (
                    tr(app.zh(), "3  DNS", "3  DNS 名称解析"),
                    tr(app.zh(), "Resolver and policy", "解析器与配套策略"),
                ),
                (
                    tr(app.zh(), "4  Advanced", "4  高级"),
                    tr(
                        app.zh(),
                        "Ports, core path, language",
                        "端口、核心路径、界面语言",
                    ),
                ),
            ]
            .into_iter()
            .map(|(a, b)| {
                ListItem::new(vec![
                    Line::from(a),
                    Line::styled(b, Style::default().fg(MUTED)),
                    Line::from(""),
                ])
            })
            .collect();
            let mut state = ListState::default().with_selected(Some(*selected));
            f.render_stateful_widget(
                List::new(items)
                    .highlight_style(Style::default().bg(Color::Rgb(38, 62, 68)))
                    .highlight_symbol("▎"),
                Rect::new(
                    inner.x,
                    inner.y,
                    inner.width,
                    inner.height.saturating_sub(1),
                ),
                &mut state,
            );
            f.render_widget(
                Paragraph::new(tr(
                    app.zh(),
                    "1–4 / ↑↓ Enter · Esc cancel",
                    "1–4 / ↑↓ Enter 选择 · Esc 取消",
                ))
                .style(Style::default().fg(ACCENT)),
                Rect::new(inner.x, area.bottom() - 2, inner.width, 1),
            );
        }
        Dialog::Members {
            group,
            cursor,
            filter,
            searching,
            select_only,
        } => {
            f.render_widget(
                block(format!("{} · {} checked", group.name, group.members.len())),
                area,
            );
            f.render_widget(
                Paragraph::new(format!(
                    "{}{}{}",
                    if *searching { "Search: " } else { "/ search  " },
                    filter,
                    if *searching { "▏ · Enter done" } else { "" }
                ))
                .style(Style::default().fg(ACCENT)),
                Rect::new(inner.x, inner.y, inner.width, 1),
            );
            let rows = member_rows(&app.snapshot.store, group, filter, *select_only);
            let items: Vec<_> = rows
                .iter()
                .map(|(id, name)| {
                    ListItem::new(format!(
                        "{} {}{}",
                        if group.members.contains(id) {
                            "[x]"
                        } else {
                            "[ ]"
                        },
                        name,
                        if group.selected.as_ref() == Some(id) {
                            " · selected"
                        } else {
                            ""
                        }
                    ))
                })
                .collect();
            let mut state =
                ListState::default().with_selected((!items.is_empty()).then_some(*cursor));
            f.render_stateful_widget(
                List::new(items)
                    .highlight_style(Style::default().bg(Color::Rgb(38, 62, 68)))
                    .highlight_symbol("▎"),
                Rect::new(
                    inner.x,
                    inner.y + 1,
                    inner.width,
                    inner.height.saturating_sub(2),
                ),
                &mut state,
            );
            f.render_widget(
                Paragraph::new(if *select_only {
                    "Enter select · Esc cancel"
                } else {
                    "Space toggle · F2/Ctrl+S save · Esc cancel"
                })
                .style(Style::default().fg(ACCENT)),
                Rect::new(inner.x, area.bottom() - 2, inner.width, 1),
            );
        }
        Dialog::RulesReview(p, scroll) => {
            f.render_widget(
                block(tr(app.zh(), "Review rule conversion", "规则转换预览")),
                area,
            );
            let summary = format!("{} warnings · unsupported entries excluded\n{} entries → {} matches · +{} / −{}\nTarget: {}\n{} · {}",p.warnings.len(),p.input_count,p.count,p.added,p.removed,target_name(&app.snapshot.store,&p.target),p.name,p.format);
            f.render_widget(
                Paragraph::new(summary).style(Style::default().fg(if p.warnings.is_empty() {
                    ACCENT
                } else {
                    WARN
                })),
                Rect::new(inner.x, inner.y, inner.width, 4.min(inner.height)),
            );
            let text = format!("Source policy names: {}\nThey are NOT executed. Your chosen target replaces them.\n\n{}\n\nSample converted matches:\n{}",if p.policies.is_empty() {"none".into()} else {p.policies.join(", ")},p.warnings.join("\n"),p.sample.join("\n"));
            f.render_widget(
                Paragraph::new(text)
                    .scroll((*scroll, 0))
                    .wrap(Wrap { trim: false }),
                Rect::new(
                    inner.x,
                    inner.y + 4,
                    inner.width,
                    inner.height.saturating_sub(5),
                ),
            );
            f.render_widget(
                Paragraph::new("↑↓ scroll · Enter accept subset · Esc cancel")
                    .style(Style::default().fg(ACCENT)),
                Rect::new(inner.x, area.bottom() - 2, inner.width, 1),
            );
        }
        Dialog::Form {
            title,
            fields,
            selected,
            ..
        } => {
            f.render_widget(
                block(title.clone()).border_style(Style::default().fg(ACCENT)),
                area,
            );
            let visible = ((inner.height.saturating_sub(3)) / 3).max(1) as usize;
            let start = selected.saturating_sub(visible - 1);
            for (i, field) in fields.iter().enumerate().skip(start).take(visible) {
                let row = inner.y + (i - start) as u16 * 3;
                let active = i == *selected;
                f.render_widget(
                    Paragraph::new(field.label.as_str()).style(Style::default().fg(if active {
                        ACCENT
                    } else {
                        MUTED
                    })),
                    Rect::new(inner.x, row, inner.width, 1),
                );
                let value = if !field.choices.is_empty() {
                    format!(
                        "‹ {} ›  {}",
                        field.display_value(app.zh()),
                        tr(app.zh(), "Space to change", "空格切换")
                    )
                } else if field.secret {
                    if field.value.is_empty() {
                        "Paste here — stored privately".into()
                    } else {
                        format!("{} chars · hidden", field.value.chars().count())
                    }
                } else {
                    field.value.replace('\n', " ↵ ")
                };
                let value = if active { format!("{value}▏") } else { value };
                f.render_widget(
                    Paragraph::new(value).style(Style::default().fg(FG).bg(if active {
                        Color::Rgb(38, 62, 68)
                    } else {
                        BG
                    })),
                    Rect::new(inner.x, row + 1, inner.width, 1),
                );
            }
            f.render_widget(
                Paragraph::new(tr(
                    app.zh(),
                    "Tab next · Ctrl+S / F2 save · Esc cancel",
                    "Tab 下一项 · Ctrl+S / F2 保存 · Esc 取消",
                ))
                .style(Style::default().fg(ACCENT)),
                Rect::new(inner.x, area.bottom() - 2, inner.width, 1),
            );
        }
        Dialog::Import(p, scroll) => {
            let mut text = format!(
                "{} · {}\n{} nodes   +{} added   −{} removed\n\n",
                p.name, p.format, p.count, p.added, p.removed
            );
            for n in &p.names {
                text.push_str(&format!("  {n}\n"));
            }
            if !p.warnings.is_empty() {
                text.push_str("\nWarnings (unsupported entries are skipped):\n");
                for w in &p.warnings {
                    text.push_str(&format!("• {w}\n"));
                }
            }
            f.render_widget(
                Paragraph::new(text)
                    .scroll((*scroll, 0))
                    .block(block(tr(app.zh(), "Review import", "导入预览")))
                    .wrap(Wrap { trim: false }),
                area,
            );
            f.render_widget(
                Paragraph::new(tr(
                    app.zh(),
                    "↑↓ scroll   Enter save   Esc cancel",
                    "↑↓ 滚动   Enter 保存   Esc 取消",
                ))
                .style(Style::default().fg(ACCENT).bg(PANEL)),
                Rect::new(inner.x, area.bottom() - 2, inner.width, 1),
            );
        }
        Dialog::Text {
            title,
            text,
            scroll,
        } => {
            f.render_widget(block(title.clone()), area);
            f.render_widget(
                Paragraph::new(text.as_str())
                    .scroll((*scroll, 0))
                    .wrap(Wrap { trim: false }),
                inner,
            );
            f.render_widget(
                Paragraph::new(tr(
                    app.zh(),
                    "↑↓ / PgUp PgDn scroll · Esc close",
                    "↑↓ / PgUp PgDn 滚动 · Esc 关闭",
                ))
                .style(Style::default().fg(ACCENT).bg(PANEL)),
                Rect::new(inner.x, area.bottom() - 2, inner.width, 1),
            );
        }
        Dialog::Confirm { title, text, .. } => {
            f.render_widget(block(title.clone()), area);
            let (body, footer) = text
                .rsplit_once("\n\n")
                .unwrap_or((text, "Enter: continue    Esc: cancel"));
            f.render_widget(
                Paragraph::new(body).wrap(Wrap { trim: false }),
                Rect::new(
                    inner.x,
                    inner.y,
                    inner.width,
                    inner.height.saturating_sub(1),
                ),
            );
            f.render_widget(
                Paragraph::new(footer).style(Style::default().fg(ACCENT)),
                Rect::new(inner.x, area.bottom() - 2, inner.width, 1),
            );
        }
        Dialog::Authorize { kind, .. } => {
            f.render_widget(
                block(tr(app.zh(), "Administrator permission", "管理员授权")),
                area,
            );
            let text = if kind == "system" {
                tr(app.zh(),"sudo will request your password in the real terminal. sing never reads it. A privileged helper will back up / manage macOS system proxy settings and watch for manager or core failure.\n\nRecovery records are root-only. This does not enable TUN or change DNS servers.\n\nEnter: authorize    Esc: cancel","sudo 将在真实终端请求密码，sing 不读取密码。管理员辅助进程负责备份和管理 macOS 系统代理，并监护管理器或核心异常。\n\n恢复记录仅管理员可读；不启用 TUN，不修改 DNS 服务器。\n\nEnter 授权    Esc 取消")
            } else {
                tr(app.zh(),"sudo will request your password. This authorizes the experimental TUN helper on this host.\n\nEnter: authorize    Esc: cancel","sudo 将请求密码，授权当前主机上的实验性 TUN 辅助进程。\n\nEnter 授权    Esc 取消")
            };
            let (body, footer) = text.rsplit_once("\n\n").unwrap();
            f.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), inner);
            f.render_widget(
                Paragraph::new(footer).style(Style::default().fg(ACCENT)),
                Rect::new(inner.x, area.bottom() - 2, inner.width, 1),
            );
        }
    }
}

struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            DisableBracketedPaste,
            crossterm::cursor::Show
        );
    }
}
fn enter() -> Result<()> {
    enable_raw_mode()?;
    execute!(
        io::stdout(),
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    Ok(())
}
fn leave() -> Result<()> {
    disable_raw_mode()?;
    execute!(
        io::stdout(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste,
        crossterm::cursor::Show
    )?;
    Ok(())
}

pub fn run(dir: PathBuf, demo: bool) -> Result<()> {
    anyhow::ensure!(
        io::stdout().is_terminal(),
        "Run sing in an interactive terminal (or use --preview)"
    );
    let dir = if dir.is_absolute() {
        dir
    } else {
        std::env::current_dir()?.join(dir)
    };
    let snapshot = if demo {
        sample()?
    } else {
        runtime::ensure_daemon(&dir)?;
        runtime::request(&dir, Action::Snapshot)?
            .snapshot
            .context("No manager snapshot")?
    };
    let mut app = App::new(snapshot, demo);
    anyhow::ensure!(demo||app.snapshot.manager_protocol>=5,"The background manager is an older version. Close old interfaces, run ./sing --shutdown, then reopen ./sing. Saved subscriptions are preserved; shutdown stops the old core.");
    let (tx, rx) = mpsc::channel::<Action>();
    let (out, results) = mpsc::channel::<Result<Reply>>();
    let worker_dir = dir.clone();
    std::thread::spawn(move || {
        for action in rx {
            let snapshot = matches!(action, Action::Snapshot);
            let mut result = runtime::request(&worker_dir, action);
            if !snapshot {
                if let Ok(reply) = &mut result {
                    if let Ok(fresh) = runtime::request(&worker_dir, Action::Snapshot) {
                        reply.snapshot = fresh.snapshot;
                    }
                }
            }
            if out.send(result).is_err() {
                break;
            }
        }
    });
    let old_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = leave();
        old_hook(info);
    }));
    enter()?;
    let _guard = TerminalGuard;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut redraw = true;
    while !app.quit {
        while let Ok(result) = results.try_recv() {
            redraw = true;
            match result {
                Ok(reply) => {
                    if !reply.message.is_empty() {
                        app.busy = false;
                        app.error = !reply.ok;
                    }
                    app.reply(reply);
                }
                Err(e) => {
                    app.busy = false;
                    app.polling = false;
                    app.error = true;
                    app.notice = format!("{e:#}");
                    if app.page == 7 {
                        app.connections_error = app.notice.clone();
                    }
                }
            }
        }
        if redraw || app.busy {
            terminal.draw(|f| draw(f, &app))?;
            redraw = false;
        }
        if event::poll(Duration::from_millis(100))? {
            redraw = true;
            let mut action = None;
            match event::read()? {
                Event::Key(k) if k.kind != event::KeyEventKind::Release => {
                    if matches!(app.dialog, Some(Dialog::Authorize { .. }))
                        && k.code == KeyCode::Enter
                    {
                        let Some(Dialog::Authorize { kind, after }) = app.dialog.take() else {
                            unreachable!()
                        };
                        leave()?;
                        let result = if kind == "system" {
                            crate::system_proxy::helper::authorize(&dir)
                        } else {
                            runtime::authorize_tun(&dir, &app.snapshot.core)
                        };
                        enter()?;
                        terminal.clear()?;
                        match result {
                            Ok(()) => action = Some(after),
                            Err(e) => {
                                app.notice = format!("{e:#}");
                                app.error = true;
                            }
                        }
                    } else {
                        match app.key(k) {
                            Ok(a) => action = a,
                            Err(e) => {
                                app.notice = format!("{e:#}");
                                app.error = true;
                            }
                        }
                    }
                }
                Event::Paste(text) => {
                    if let Some(Dialog::Form {
                        fields, selected, ..
                    }) = &mut app.dialog
                    {
                        fields[*selected].insert(&text);
                    }
                }
                Event::Mouse(m) if app.dialog.is_none() && !app.busy => match m.kind {
                    MouseEventKind::ScrollDown => app.change_selection(1),
                    MouseEventKind::ScrollUp => app.change_selection(-1),
                    MouseEventKind::Down(event::MouseButton::Left) => {
                        if m.row == 3 {
                            let width = terminal.size()?.width;
                            let labels = tab_labels(app.zh(), width);
                            let mut col = 0;
                            for (i, label) in labels.iter().enumerate() {
                                if width < 88 && i != app.page {
                                    continue;
                                }
                                let w = if app.zh() { 8 } else { label.len() as u16 + 4 };
                                if m.column >= col && m.column < col + w {
                                    app.page = i;
                                    break;
                                }
                                col += w + 1;
                            }
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
            if let Some(action) = action {
                if app.demo {
                    match action {
                        Action::Connections => {
                            app.connections = ConnectionReport {
                                observed_at: model::now(),
                                total: 1,
                                items: vec![crate::api::Connection {
                                    id: "00000000-0000-0000-0000-000000000001".into(),
                                    inbound: "mixed-in".into(),
                                    inbound_type: "mixed".into(),
                                    network: "tcp".into(),
                                    destination: "example.invalid:443".into(),
                                    domain: "example.invalid".into(),
                                    outbound: "g-deabcd".into(),
                                    rule: "rule_set=rs-de1234".into(),
                                    created_at: model::now() as i64 * 1000,
                                    ..Default::default()
                                }],
                            };
                            app.notice = "Fictional demo connections · no network requests".into();
                        }
                        Action::Diagnostics => {
                            app.dialog = Some(Dialog::Text {
                                title: "Demo diagnostics".into(),
                                text: config::diagnostics(&app.snapshot.store, None),
                                scroll: 0,
                            });
                        }
                        Action::SaveGroup(group) => {
                            if let Some(old) = app
                                .snapshot
                                .store
                                .proxy_groups
                                .iter_mut()
                                .find(|g| g.id == group.id)
                            {
                                *old = group;
                            } else {
                                app.snapshot.store.proxy_groups.push(group);
                            }
                            app.notice = "Demo group saved in memory only".into();
                        }
                        Action::SelectGroup { group, node } => {
                            if let Some(g) = app
                                .snapshot
                                .store
                                .proxy_groups
                                .iter_mut()
                                .find(|g| g.id == group)
                            {
                                g.selected = Some(node);
                            }
                        }
                        Action::SaveSettings(s) => app.snapshot.store.settings = s,
                        Action::Preview => {
                            app.dialog = Some(Dialog::Text {
                                title: "Demo configuration".into(),
                                text: serde_json::to_string_pretty(&config::redacted(
                                    &config::generate(&app.snapshot.store)?,
                                ))?,
                                scroll: 0,
                            })
                        }
                        Action::Select(id) => app.snapshot.store.selected = Some(id),
                        Action::Favorite(id) => {
                            if let Some(n) =
                                app.snapshot.store.nodes.iter_mut().find(|n| n.id == id)
                            {
                                n.favorite = !n.favorite;
                            }
                        }
                        _ => {
                            app.notice =
                                "Demo is offline. Run without --demo to use real subscriptions."
                                    .into()
                        }
                    }
                } else {
                    app.busy = true;
                    app.error = false;
                    tx.send(action)?;
                }
            }
        }
        if !app.demo
            && !app.busy
            && !app.polling
            && app.dialog.is_none()
            && app.last_poll.elapsed() > Duration::from_secs(2)
        {
            tx.send(if app.page == 7 {
                Action::Connections
            } else {
                Action::Snapshot
            })?;
            app.polling = true;
            app.last_poll = Instant::now();
        }
        app.tick += 1;
    }
    drop(terminal);
    drop(_guard);
    println!("{}",tr(app.zh(),"Interface closed. If connected, the core is still running. Reopen sing or use --disconnect.","界面已关闭。已建立的连接继续运行；重新打开 sing 或使用 --disconnect。"));
    Ok(())
}

fn sample() -> Result<Snapshot> {
    let mut store = Store::new()?;
    store.nodes=subscription::parse("trojan://fictional@jp.example.invalid:443#Tokyo%20%2F%20Edge\ntrojan://fictional@sg.example.invalid:443#Singapore%20%2F%20Cloud\ntrojan://fictional@de.example.invalid:443#Frankfurt%20%2F%20Transit","demo")?.nodes;
    store.nodes[0].favorite = true;
    store.selected = Some(store.nodes[0].id.clone());
    store.proxy_groups.push(ProxyGroup {
        id: "deabcd".into(),
        name: "Media".into(),
        kind: "selector".into(),
        members: store.nodes.iter().take(2).map(|n| n.id.clone()).collect(),
        selected: store.selected.clone(),
    });
    store.rule_resources.push(model::RuleResource {
        id: "de1234".into(),
        name: "Video example".into(),
        source: "Offline fixture".into(),
        format: "qx".into(),
        updated_at: model::now(),
        digest: "demo".into(),
        input_count: 1,
        rules: vec![model::MatchRule {
            kind: "domain_suffix".into(),
            value: "example.invalid".into(),
        }],
        warnings: vec![],
    });
    store.rule_bindings.push(RuleBinding {
        id: "de5678".into(),
        resource: "de1234".into(),
        target: "g-deabcd".into(),
        enabled: true,
    });
    store.subscriptions.push(model::Subscription {
        id: "demo".into(),
        name: "Sample subscription".into(),
        source: "https://example.invalid/••••".into(),
        format: "URI list".into(),
        updated_at: model::now(),
        warnings: vec![],
        user_agent: String::new(),
    });
    Ok(Snapshot {
        manager_protocol: 5,
        system_proxy: Default::default(),
        connectivity: Default::default(),
        store,
        connected: false,
        api_ready: false,
        core: String::new(),
        version: String::new(),
        running_settings: None,
        dirty: false,
        status: Default::default(),
        groups: Default::default(),
        activity: vec!["Sample data only · no network requests".into()],
        host: runtime::host(),
        ssh: false,
    })
}
pub fn preview() -> Result<()> {
    let app = App::new(sample()?, true);
    let mut terminal = Terminal::new(TestBackend::new(108, 32))?;
    terminal.draw(|f| draw(f, &app))?;
    for row in terminal.backend().buffer().content.chunks(108) {
        println!(
            "{}",
            row.iter()
                .map(|c| c.symbol())
                .collect::<String>()
                .trim_end()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_pages_render_wide_and_narrow() {
        for (w, h) in [(120, 36), (80, 24), (54, 18), (40, 12)] {
            let mut app = App::new(sample().unwrap(), true);
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            for page in 0..8 {
                app.page = page;
                terminal.draw(|f| draw(f, &app)).unwrap();
            }
            app.settings();
            terminal.draw(|f| draw(f, &app)).unwrap();
            app.import();
            terminal.draw(|f| draw(f, &app)).unwrap();
        }
    }
    #[test]
    fn unicode_editing() {
        let mut f = Field::text("name", "日本");
        f.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        f.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(f.value, "本");
        f.insert("新");
        assert_eq!(f.value, "新本");
    }
    fn press(app: &mut App, code: KeyCode) -> Option<Action> {
        app.key(KeyEvent::new(code, KeyModifiers::NONE)).unwrap()
    }
    #[test]
    fn routing_form_is_independent_and_never_implicitly_connects() {
        let mut app = App::new(sample().unwrap(), false);
        let before = app.snapshot.store.settings.clone();
        assert!(press(&mut app, KeyCode::Char('M')).is_none());
        press(&mut app, KeyCode::Char(' '));
        let Some(Action::SaveSettings(after)) = press(&mut app, KeyCode::F(2)) else {
            panic!("Expected save only")
        };
        assert_eq!(after.route_mode, "global");
        assert_eq!(after.mode, before.mode);
        assert_eq!(after.rules, before.rules);
        assert_eq!(after.dns_policy, before.dns_policy);
        assert_eq!(app.snapshot.store.settings, before);
        assert!(matches!(
            press(&mut app, KeyCode::Char('D')),
            Some(Action::Diagnostics)
        ));
    }
    #[test]
    fn settings_language_is_independent_of_stored_values() {
        for language in ["en", "zh"] {
            let mut app = App::new(sample().unwrap(), false);
            app.snapshot.store.settings.language = language.into();
            for section in 0..4 {
                app.section(section);
                let mut terminal = Terminal::new(TestBackend::new(110, 32)).unwrap();
                terminal
                    .draw(|f| draw_dialog(f, &app, app.dialog.as_ref().unwrap()))
                    .unwrap();
                let text: String = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|c| c.symbol())
                    .collect();
                if language == "en" {
                    assert!(
                        !text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
                        "{text}"
                    );
                } else {
                    assert!(
                        !text.contains("Space to change")
                            && !text.contains("Global target group")
                            && !text.contains("Capture mode"),
                        "{text}"
                    );
                }
            }
            app.section(1);
            press(&mut app, KeyCode::Char(' '));
            let Some(Dialog::Form { fields, .. }) = &app.dialog else {
                panic!()
            };
            assert_eq!(fields[0].value, "global");
            assert_eq!(
                fields[0].display_value(language == "zh"),
                if language == "zh" {
                    "全局代理"
                } else {
                    "Global proxy"
                }
            );
            let Some(Action::SaveSettings(s)) = press(&mut app, KeyCode::F(2)) else {
                panic!()
            };
            assert_eq!(s.route_mode, "global");
            assert_eq!(s.language, language);
        }
    }
    #[test]
    fn home_route_shows_actual_group_member_and_rule_fallback() {
        let mut s = sample().unwrap();
        let group = s.store.proxy_groups[0].tag();
        s.store.settings.route_mode = "global".into();
        s.store.settings.global_target = group.clone();
        s.running_settings = Some(s.store.settings.clone());
        s.connected = true;
        s.api_ready = true;
        s.groups.group = vec![crate::api::Group {
            tag: group,
            selected: s.store.nodes[1].tag(),
            ..Default::default()
        }];
        let (label, route) = home_route(&s);
        assert_eq!(label, "Global target");
        assert!(
            route.contains(&s.store.proxy_groups[0].name) && route.contains(&s.store.nodes[1].name)
        );
        assert!(!route.contains(&s.store.nodes[0].name));
        s.store.settings.global_target = "proxy".into();
        assert_eq!(home_route(&s).1, route); // Saved edit must not impersonate live state.
        s.api_ready = false;
        assert!(home_route(&s).1.contains("live node unavailable"));
        s.running_settings.as_mut().unwrap().route_mode = "rule".into();
        s.running_settings.as_mut().unwrap().routing = "direct".into();
        assert_eq!(
            home_route(&s),
            ("Unmatched traffic".into(), "Direct".into())
        );
    }
    #[test]
    fn long_diagnostics_do_not_overlap_the_fixed_footer() {
        let mut app = App::new(sample().unwrap(), false);
        app.dialog = Some(Dialog::Text {
            title: "Diagnostics".into(),
            text: "BODY".repeat(1000),
            scroll: 0,
        });
        for (w, h) in [(110, 32), (54, 18)] {
            let text = rendered(&app, w, h);
            let footer = text
                .lines()
                .find(|line| line.contains("Esc close"))
                .unwrap();
            assert!(!footer.contains("BODY"), "{footer}");
        }
    }
    #[test]
    fn settings_sections_are_usable_in_both_languages_and_small_terminals() {
        for language in ["en", "zh"] {
            let mut app = App::new(sample().unwrap(), false);
            app.snapshot.store.settings.language = language.into();
            for section in ['1', '2', '3', '4'] {
                app.settings();
                press(&mut app, KeyCode::Char(section));
                for (w, h) in [(110, 32), (54, 18)] {
                    let text = rendered(&app, w, h);
                    assert!(text.contains("Esc"), "{text}");
                    assert!(text.contains("F2"), "{text}");
                }
                assert!(press(&mut app, KeyCode::Esc).is_none());
            }
        }
    }
    #[test]
    fn connections_filter_history_details_and_confirm_exact_id() {
        let mut app = App::new(sample().unwrap(), false);
        assert!(matches!(
            press(&mut app, KeyCode::Char('8')),
            Some(Action::Connections)
        ));
        app.connections = ConnectionReport {
            observed_at: model::now().saturating_sub(10),
            total: 2,
            items: vec![
                crate::api::Connection {
                    id: "00000000-0000-0000-0000-000000000001".into(),
                    domain: "video.invalid".into(),
                    outbound: "direct".into(),
                    ..Default::default()
                },
                crate::api::Connection {
                    id: "00000000-0000-0000-0000-000000000002".into(),
                    domain: "closed.invalid".into(),
                    closed_at: 1,
                    ..Default::default()
                },
            ],
        };
        assert_eq!(app.connection_rows().len(), 1);
        assert!(rendered(&app, 110, 32).contains("STALE"));
        press(&mut app, KeyCode::Char('h'));
        assert_eq!(app.connection_rows().len(), 2);
        app.connection_filter = "video".into();
        assert_eq!(app.connection_rows().len(), 1);
        press(&mut app, KeyCode::Enter);
        let Some(Dialog::Text { text, .. }) = &app.dialog else {
            panic!("Details missing")
        };
        assert!(text.contains("unavailable") && text.contains("not reported"));
        press(&mut app, KeyCode::Esc);
        assert!(press(&mut app, KeyCode::Char('x')).is_none());
        for (w, h) in [(110, 32), (54, 18)] {
            let text = rendered(&app, w, h);
            assert!(text.contains("Enter") && text.contains("Esc"), "{text}");
        }
        // A new sample must not silently change the connection being confirmed.
        app.connections.items.swap(0, 1);
        let Some(Action::CloseConnection(id)) = press(&mut app, KeyCode::Enter) else {
            panic!("Expected close")
        };
        assert!(id.ends_with("001"));
        app.connection_filter = "closed".into();
        assert!(press(&mut app, KeyCode::Char('x')).is_none());
        assert!(app.dialog.is_none());
    }
    #[test]
    fn group_creation_members_and_named_targets() {
        let mut app = App::new(sample().unwrap(), true);
        press(&mut app, KeyCode::Char('6'));
        press(&mut app, KeyCode::Char('a'));
        if let Some(Dialog::Form { fields, .. }) = &mut app.dialog {
            fields[0].insert("Video");
        }
        press(&mut app, KeyCode::F(2));
        assert!(matches!(app.dialog, Some(Dialog::Members { .. })));
        press(&mut app, KeyCode::Char(' '));
        let Some(Action::SaveGroup(g)) = press(&mut app, KeyCode::F(2)) else {
            panic!("Expected group save")
        };
        assert_eq!(g.members.len(), 1);
        assert_eq!(g.selected, Some(g.members[0].clone()));
        app.snapshot.store.proxy_groups.push(g.clone());
        let f = target_field(&app.snapshot.store, "Target", &g.tag(), true);
        assert_eq!(f.value, "Video");
        assert_eq!(target_tag(&app.snapshot.store, &f.value), g.tag());
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            press(&mut app, KeyCode::Enter),
            Some(Action::SelectGroup { .. })
        ));
    }
    #[test]
    fn rule_review_keeps_warning_and_cancel_visible() {
        let mut app = App::new(sample().unwrap(), true);
        let p = RulesPreview {
            name: "Video".into(),
            format: "qx".into(),
            input_count: 3,
            count: 2,
            added: 2,
            removed: 0,
            target: "proxy".into(),
            warnings: vec!["USER-AGENT unsupported".into()],
            policies: vec!["original".into()],
            sample: vec![],
        };
        for (w, h) in [(120, 36), (80, 24), (54, 18)] {
            app.dialog = Some(Dialog::RulesReview(p.clone(), 0));
            let text = rendered(&app, w, h);
            assert!(text.contains("1 warnings"), "{text}");
            assert!(text.contains("Esc"), "{text}");
        }
        assert!(matches!(
            press(&mut app, KeyCode::Esc),
            Some(Action::CancelRules)
        ));
        app.dialog = Some(Dialog::RulesReview(p, 0));
        assert!(matches!(
            press(&mut app, KeyCode::Enter),
            Some(Action::CommitRules)
        ));
    }
    #[test]
    fn disconnected_system_mode_is_off_not_unverified() {
        let mut app = App::new(sample().unwrap(), true);
        app.snapshot.store.settings.mode = "system".into();
        let text = rendered(&app, 110, 36);
        assert!(text.contains("System proxy OFF"));
        assert!(!text.contains("takeover NOT verified"));
    }
    #[test]
    fn quitting_does_not_disconnect() {
        let mut app = App::new(sample().unwrap(), true);
        assert!(app
            .key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE))
            .unwrap()
            .is_none());
        assert!(app.quit);
    }
    #[test]
    fn ctrl_c_quits_instead_of_connecting() {
        let mut app = App::new(sample().unwrap(), true);
        assert!(app
            .key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
            .unwrap()
            .is_none());
        assert!(app.quit);
    }
    fn rendered(app: &App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .chunks(width as usize)
            .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
    #[test]
    fn system_proxy_status_is_not_confused_with_port_or_connectivity() {
        let mut app = App::new(sample().unwrap(), false);
        app.snapshot.connected = true;
        app.snapshot.api_ready = true;
        let mut settings = app.snapshot.store.settings.clone();
        settings.mode = "system".into();
        app.snapshot.running_settings = Some(settings);
        assert!(rendered(&app, 54, 18).contains("PORT READY · NO SYSTEM TAKEOVER"));
        app.snapshot.system_proxy.configured = true;
        assert!(rendered(&app, 54, 18).contains("PROXY SET · ROUTING UNVERIFIED"));
        app.snapshot.system_proxy.effective = true;
        assert!(rendered(&app, 54, 18).contains("SYSTEM PROXY ON"));
        assert_eq!(app.snapshot.connectivity.state, "not_checked");
        app.snapshot.system_proxy.configured = false;
        app.snapshot.system_proxy.pending_restore = true;
        assert!(rendered(&app, 54, 18).contains("PROXY RECOVERY / REVIEW NEEDED"));
    }
    #[test]
    fn proxy_confirmation_and_authorization_remain_operable_in_small_terminal() {
        for language in ["en", "zh"] {
            let mut app = App::new(sample().unwrap(), false);
            app.snapshot.store.settings.mode = "system".into();
            app.snapshot.store.settings.language = language.into();
            for key in ['c', 'd', 'R', 'v'] {
                app.dialog = None;
                assert!(app
                    .key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE))
                    .unwrap()
                    .is_none());
                let s = rendered(&app, 54, 18);
                assert!(s.contains("Enter"), "{s}");
                assert!(s.contains("Esc"), "{s}");
            }
            app.dialog = Some(Dialog::Authorize {
                kind: "system".into(),
                after: Action::Connect,
            });
            let s = rendered(&app, 54, 18);
            assert!(s.contains("sudo"));
            assert!(s.contains("Enter"));
            assert!(s.contains("Esc"));
        }
    }
}
