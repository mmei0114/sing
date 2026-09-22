//! Advanced configuration follows sing-box's own top-level document order.
use super::{
    editor, labels,
    modal::{self, Choice, Picker, TextArea},
    notify, text, theme, App, Tab,
};
use crate::{config as redact, native, runtime::Action};
use crossterm::event::{KeyCode as K, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};
use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Section {
    Core,
    Log,
    DnsServers,
    DnsRules,
    DnsOptions,
    Ntp,
    Certificate,
    Endpoints,
    Inbounds,
    Outbounds,
    RouteRules,
    RouteOptions,
    RuleSets,
    Services,
    Experimental,
    Json,
}
impl Section {
    const ALL: [Self; 16] = [
        Self::Core,
        Self::Log,
        Self::DnsServers,
        Self::DnsRules,
        Self::DnsOptions,
        Self::Ntp,
        Self::Certificate,
        Self::Endpoints,
        Self::Inbounds,
        Self::Outbounds,
        Self::RouteRules,
        Self::RouteOptions,
        Self::RuleSets,
        Self::Services,
        Self::Experimental,
        Self::Json,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Core => "Core",
            Self::Log => "log",
            Self::DnsServers => "dns › servers",
            Self::DnsRules => "dns › rules",
            Self::DnsOptions => "dns › options",
            Self::Ntp => "ntp",
            Self::Certificate => "certificate",
            Self::Endpoints => "endpoints",
            Self::Inbounds => "inbounds",
            Self::Outbounds => "outbounds",
            Self::RouteRules => "route › rules",
            Self::RouteOptions => "route › options",
            Self::RuleSets => "route › rule_set",
            Self::Services => "services",
            Self::Experimental => "experimental",
            Self::Json => "Full JSON",
        }
    }
    fn pointer(self) -> Option<&'static str> {
        match self {
            Self::Log => Some("/log"),
            Self::DnsServers => Some("/dns/servers"),
            Self::DnsRules => Some("/dns/rules"),
            Self::DnsOptions => Some("/dns"),
            Self::Ntp => Some("/ntp"),
            Self::Certificate => Some("/certificate"),
            Self::Endpoints => Some("/endpoints"),
            Self::Inbounds => Some("/inbounds"),
            Self::Outbounds => Some("/outbounds"),
            Self::RouteRules => Some("/route/rules"),
            Self::RouteOptions => Some("/route"),
            Self::RuleSets => Some("/route/rule_set"),
            Self::Services => Some("/services"),
            Self::Experimental => Some("/experimental"),
            _ => None,
        }
    }
    fn list(self) -> bool {
        matches!(
            self,
            Self::DnsServers
                | Self::DnsRules
                | Self::Endpoints
                | Self::Inbounds
                | Self::Outbounds
                | Self::RouteRules
                | Self::RuleSets
                | Self::Services
        )
    }
}

#[derive(Default)]
pub struct State {
    section: usize,
    selected: [usize; 16],
}

const MODULES: [(&str, &str, &str, usize); 13] = [
    (
        "core",
        "Core",
        "Version, location and official downloads",
        0,
    ),
    ("log", "log", "Logging level and output", 1),
    ("dns", "dns", "Servers, rules and resolver options", 2),
    ("ntp", "ntp", "Time synchronization", 5),
    (
        "certificate",
        "certificate",
        "Trust stores and certificates",
        6,
    ),
    ("endpoints", "endpoints", "Bidirectional protocols", 7),
    ("inbounds", "inbounds", "Proxy listeners and TUN", 8),
    (
        "outbounds",
        "outbounds",
        "Nodes, groups and direct connections",
        9,
    ),
    ("route", "route", "Rules, rule sets and routing options", 10),
    (
        "services",
        "services",
        "Core services and management API",
        13,
    ),
    (
        "experimental",
        "experimental",
        "Cache and experimental APIs",
        14,
    ),
    (
        "json",
        "Full JSON",
        "Every native field; nothing discarded",
        15,
    ),
    (
        "capture",
        "System proxy",
        "Application integration, outside native config",
        0,
    ),
];

pub fn open_menu(app: &mut App) {
    let choices = MODULES
        .iter()
        .map(|(id, name, detail, _)| Choice::new(*id, *name, *detail))
        .collect();
    app.push(Picker::single("Config · choose a module", choices, "", Box::new(|app, picked| {
        let Some(id) = picked.first() else { return };
                if id == "capture" {
                    if app.snap.system_proxy.pending_restore && (!app.snap.system_proxy.configured || !app.snap.system_proxy.helper_ready || !app.snap.connected) {
                        return app.push(modal::Confirm::new("Restore system proxy", "Restore the original system proxy settings recorded by sing? The core will not be stopped. Administrator authorization may be requested.", "Restore", Box::new(|app| app.request_busy(Action::RestoreProxy, "Restoring", Box::new(notify)))));
                    }
            return app.push(modal::Confirm::new("System proxy", "Change macOS system proxy integration? Applications that ignore system settings are unaffected. This does not edit the native sing-box configuration.", if app.system_proxy_on() { "Turn off" } else { "Turn on" }, Box::new(super::chrome::toggle_system_proxy)));
        }
        if let Some((_, _, _, index)) = MODULES.iter().find(|m| m.0 == id) {
            app.config.section = *index;
            app.go(Tab::Config);
        }
    })));
}

fn siblings(s: Section) -> &'static [usize] {
    match s {
        Section::DnsServers | Section::DnsRules | Section::DnsOptions => &[2, 3, 4],
        Section::RouteRules | Section::RouteOptions | Section::RuleSets => &[10, 12, 11],
        _ => &[],
    }
}

pub fn entered(app: &mut App) {
    if app.cores.is_none() {
        app.request(
            Action::CoreReport { releases: false },
            Box::new(|app, r| {
                if let Some(c) = r.cores {
                    app.cores = Some(c);
                } else if !r.ok {
                    app.error(r.message);
                }
            }),
        );
    }
}
pub fn capturing(_: &App) -> bool {
    false
}
pub fn paste(_: &mut App, _: &str) {}

fn section(app: &App) -> Section {
    Section::ALL[app.config.section]
}
fn items(app: &App, s: Section) -> &'_ [Value] {
    s.pointer()
        .filter(|_| s.list())
        .map(|p| native::array(app.doc(), p))
        .unwrap_or(&[])
}
fn item_label(app: &App, s: Section, v: &Value, i: usize) -> (String, String) {
    match s {
        Section::DnsRules | Section::RouteRules => {
            let (a, t) = if s == Section::DnsRules {
                labels::dns_action(v)
            } else {
                labels::action(v)
            };
            (
                if s == Section::RouteRules {
                    labels::route_matcher(&app.snap.store, v)
                } else {
                    let name = labels::matcher(&app.snap.store, v);
                    if name == "everything" {
                        "All queries".into()
                    } else {
                        name
                    }
                },
                if t.is_empty() {
                    if s == Section::RouteRules {
                        labels::route_action_label(v).into()
                    } else {
                        a
                    }
                } else {
                    format!("{a} → {}", app.label(&t))
                },
            )
        }
        _ => {
            let tag = native::tag(v);
            let name = if tag.is_empty() {
                format!("{} {}", s.label().trim_end_matches('s'), i + 1)
            } else {
                app.label(tag)
            };
            let kind = v["type"].as_str().unwrap_or("object");
            let extra = v["server"]
                .as_str()
                .or_else(|| v["path"].as_str())
                .or_else(|| v["url"].as_str())
                .unwrap_or("");
            (
                name,
                format!(
                    "{}{}",
                    labels::protocol(kind),
                    if extra.is_empty() {
                        String::new()
                    } else {
                        format!(" · {extra}")
                    }
                ),
            )
        }
    }
}

pub fn key(app: &mut App, k: KeyEvent) {
    let s = section(app);
    let n = if s == Section::Core {
        app.cores.as_ref().map_or(0, |c| c.installed.len())
    } else {
        items(app, s).len()
    };
    let selected = app.config.selected[app.config.section];
    if s == Section::RouteRules {
        if let Some(down) = editor::reorder_direction(k) {
            let section_i = app.config.section;
            editor::shift(
                app,
                "/route/rules",
                selected,
                down,
                Box::new(move |app, moved| {
                    app.config.selected[section_i] = moved;
                }),
            );
            return;
        }
    }
    match k.code {
        K::Esc => app.go(app.last_tab),
        K::Char(']') | K::Right if !siblings(s).is_empty() => {
            let siblings = siblings(s);
            let at = siblings
                .iter()
                .position(|i| *i == app.config.section)
                .unwrap_or(0);
            app.config.section = siblings[(at + 1) % siblings.len()];
        }
        K::Char('[') | K::Left if !siblings(s).is_empty() => {
            let siblings = siblings(s);
            let at = siblings
                .iter()
                .position(|i| *i == app.config.section)
                .unwrap_or(0);
            app.config.section = siblings[(at + siblings.len() - 1) % siblings.len()];
        }
        K::Down | K::Char('j') => {
            app.config.selected[app.config.section] = (selected + 1).min(n.saturating_sub(1))
        }
        K::Up | K::Char('k') => {
            app.config.selected[app.config.section] = selected.saturating_sub(1)
        }
        K::Enter if s == Section::Core => {
            if let Some(core) = app.cores.as_ref().and_then(|c| c.installed.get(selected)) {
                app.request_busy(
                    Action::SelectCore(core.path.clone()),
                    "Selecting core",
                    Box::new(notify),
                );
            }
        }
        K::Enter | K::Char('e') if s == Section::Json => open_json(app),
        K::Enter | K::Char('e') if s.list() => {
            if selected < items(app, s).len() {
                editor::open(app, format!("{}/{}", s.pointer().unwrap(), selected));
            }
        }
        K::Enter | K::Char('e') if s.pointer().is_some() => {
            editor::open(app, s.pointer().unwrap().into())
        }
        K::Char('n') if s.list() => editor::create_from_template(app, s.pointer().unwrap()),
        K::Char('x') if s.list() && selected < items(app, s).len() => {
            let (name, _) = item_label(app, s, &items(app, s)[selected], selected);
            editor::remove(app, s.pointer().unwrap(), selected, name);
        }
        K::Char('r') if s == Section::Core => refresh_cores(app, false),
        K::Char('d') if s == Section::Core => refresh_cores(app, true),
        K::Char('i') if s == Section::Core => install_core(app),
        _ => {}
    }
}

fn refresh_cores(app: &mut App, releases: bool) {
    app.request_busy(
        Action::CoreReport { releases },
        "Refreshing cores",
        Box::new(|app, r| {
            if let Some(c) = r.cores {
                app.cores = Some(c);
                app.toast("Core information refreshed");
            } else {
                notify(app, r);
            }
        }),
    );
}
fn install_core(app: &mut App) {
    if app.cores.as_ref().is_none_or(|c| c.releases.is_empty()) {
        return refresh_cores(app, true);
    }
    let choices: Vec<Choice> = app
        .cores
        .as_ref()
        .unwrap()
        .releases
        .iter()
        .map(|r| {
            Choice::new(
                &r.version,
                format!("sing-box {}", r.version),
                format!(
                    "{}{}{}",
                    r.published,
                    if r.prerelease { " · prerelease" } else { "" },
                    if r.installed { " · installed" } else { "" }
                ),
            )
        })
        .collect();
    app.push(Picker::single(
        "Download official sing-box core",
        choices,
        "",
        Box::new(|app, picked| {
            if let Some(version) = picked.into_iter().next() {
                app.request_busy(
                    Action::InstallCoreVersion(version),
                    "Downloading core",
                    Box::new(notify),
                );
            }
        }),
    ));
}
fn open_json(app: &mut App) {
    app.request(
        Action::ReadNative(String::new()),
        Box::new(|app, r| {
            let Some(edit) = r.edit.filter(|_| r.ok) else {
                return app.error(r.message);
            };
            let revision = edit.revision.clone();
            let pretty = serde_json::to_string_pretty(&edit.value).unwrap_or_default();
            app.push(TextArea::new(
                "Full native configuration",
                pretty,
                Box::new(move |app, s| {
                    let value: Value =
                        serde_json::from_str(&s).map_err(|e| format!("Invalid JSON: {e}"))?;
                    app.request_busy(
                        Action::WriteNative(crate::native::Edit {
                            revision: revision.clone(),
                            pointer: String::new(),
                            value,
                        }),
                        "Saving",
                        Box::new(notify),
                    );
                    Ok(())
                }),
            ));
        }),
    );
}

pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let area = area.inner(ratatui::layout::Margin {
        horizontal: 2,
        vertical: 0,
    });
    let [nav, body] = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(area);
    let s = section(app);
    let mut spans = vec![Span::styled(
        match s {
            Section::DnsServers | Section::DnsRules | Section::DnsOptions => "Config / dns   ",
            Section::RouteRules | Section::RuleSets | Section::RouteOptions => "Config / route   ",
            _ => "Config / ",
        },
        theme::s(theme::dim()),
    )];
    if siblings(s).is_empty() {
        spans.push(Span::styled(s.label(), theme::bold(theme::accent())));
    } else {
        for i in siblings(s) {
            spans.push(Span::styled(
                format!(
                    "{}   ",
                    match Section::ALL[*i] {
                        Section::DnsServers => "Servers",
                        Section::DnsRules | Section::RouteRules => "Rules",
                        Section::DnsOptions | Section::RouteOptions => "Options",
                        Section::RuleSets => "Rule sets",
                        other => other.label(),
                    }
                ),
                if *i == app.config.section {
                    theme::bold(theme::accent())
                } else {
                    theme::s(theme::dim())
                },
            ));
        }
        spans.push(Span::styled("← →", theme::s(theme::dim())));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), nav);
    draw_body(f, body, app);
}
fn draw_body(f: &mut Frame, area: Rect, app: &App) {
    let s = section(app);
    if s == Section::Core {
        return draw_core(f, area, app);
    }
    if s == Section::Json {
        f.render_widget(Paragraph::new("Full native JSON\n\nEvery field is preserved. Use this when a newer or uncommon sing-box option is not yet described by the shared editor. Secrets are loaded only after you open it.\n\nEnter opens the editor · Ctrl+S saves to draft · A applies.").wrap(Wrap { trim: false }), area);
        return;
    }
    if !s.list() {
        let value = s
            .pointer()
            .and_then(|p| app.doc().pointer(p))
            .cloned()
            .unwrap_or(Value::Null);
        let pretty =
            serde_json::to_string_pretty(&redact::redacted(&value)).unwrap_or_else(|_| "{}".into());
        f.render_widget(
            Paragraph::new(pretty)
                .style(theme::s(theme::dim()))
                .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    let rows = items(app, s);
    if rows.is_empty() {
        f.render_widget(
            Paragraph::new(format!("{}\n\n  Empty · n creates an item.", s.label())),
            area,
        );
        return;
    }
    let selected = app.config.selected[app.config.section].min(rows.len() - 1);
    let height = area.height.saturating_sub(2) as usize;
    let start = modal::scroll(selected, height, rows.len());
    let w = area.width as usize;
    let name_w = (w / 2).clamp(16, 40);
    let mut lines = vec![Line::from(vec![
        Span::styled(text::cell(s.label(), name_w), theme::s(theme::faint())),
        Span::styled("DETAIL", theme::s(theme::faint())),
    ])];
    for (i, value) in rows.iter().enumerate().skip(start).take(height) {
        let (name, detail) = item_label(app, s, value, i);
        lines.push(modal::row(
            i == selected,
            vec![
                Span::styled(text::cell(&name, name_w), theme::s(theme::text())),
                Span::styled(
                    text::fit(&detail, w.saturating_sub(name_w + 2)),
                    theme::s(theme::dim()),
                ),
            ],
        ));
    }
    f.render_widget(Paragraph::new(lines), area);
}
fn draw_core(f: &mut Frame, area: Rect, app: &App) {
    let Some(report) = &app.cores else {
        f.render_widget(Paragraph::new("Loading core information…"), area);
        return;
    };
    let selected =
        app.config.selected[app.config.section].min(report.installed.len().saturating_sub(1));
    let mut lines = vec![
        Line::from(vec![
            Span::styled("sing-box core", theme::bold(theme::text())),
            Span::styled(
                format!("  tested with {}", report.tested),
                theme::s(theme::faint()),
            ),
        ]),
        Line::raw(""),
    ];
    for (i, c) in report.installed.iter().enumerate() {
        lines.push(modal::row(
            i == selected,
            vec![
                Span::styled(
                    if c.selected { "● " } else { "○ " },
                    theme::s(if c.selected {
                        theme::good()
                    } else {
                        theme::dim()
                    }),
                ),
                Span::styled(text::cell(&c.version, 24), theme::s(theme::text())),
                Span::styled(text::cell(&c.source, 10), theme::s(theme::dim())),
                Span::styled(
                    text::fit(&c.path, area.width.saturating_sub(42) as usize),
                    theme::s(theme::faint()),
                ),
            ],
        ));
    }
    if report.installed.is_empty() {
        lines.push(Line::styled(
            "  No sing-box core found · i downloads an official release",
            theme::s(theme::warn()),
        ));
    }
    if !report.releases_error.is_empty() {
        lines.push(Line::styled(
            format!("\nRelease list: {}", report.releases_error),
            theme::s(theme::bad()),
        ));
    }
    lines.push(Line::styled(
        "\nEnter selects · r rescans · d loads releases · i installs",
        theme::s(theme::faint()),
    ));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}
pub fn hints(app: &App) -> Vec<(&'static str, &'static str)> {
    match section(app) {
        Section::Core => vec![
            ("↑↓", "core"),
            ("enter", "select"),
            ("r", "rescan"),
            ("d", "releases"),
            ("i", "install"),
            ("esc", "back"),
        ],
        Section::RouteRules => vec![
            ("↑↓", "rule"),
            ("Alt+↑↓", "reorder"),
            ("enter", "edit"),
            ("n", "new"),
            ("x", "remove"),
            ("esc", "back"),
        ],
        s if s.list() => vec![
            ("↑↓", "item"),
            ("enter", "edit"),
            ("n", "new"),
            ("x", "remove"),
            ("esc", "back"),
        ],
        _ => vec![("enter", "edit"), (":", "modules"), ("esc", "back")],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_sections_follow_document_order() {
        let labels = Section::ALL.map(Section::label);
        assert_eq!(
            &labels[1..7],
            &[
                "log",
                "dns › servers",
                "dns › rules",
                "dns › options",
                "ntp",
                "certificate"
            ]
        );
        assert!(
            labels.iter().position(|x| *x == "inbounds")
                < labels.iter().position(|x| *x == "route › rules")
        );
    }
}
