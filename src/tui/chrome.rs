//! Header tabs, hint line and the bottom control bar with the always-needed
//! switches: Start, Mode and TUN. Config and help remain utilities on the right.
use super::{
    modal::{self, any, Choice, Confirm, Modal, Outcome, Picker},
    notify, text, theme, App, Tab,
};
use crate::runtime::Action;
use crossterm::event::{KeyCode as K, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

pub fn layout(area: Rect) -> [Rect; 4] {
    Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area)
}

fn spinner() -> &'static str {
    const FRAMES: [&str; 4] = ["◐", "◓", "◑", "◒"];
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    FRAMES[(ms / 150) as usize % 4]
}

pub fn header(f: &mut Frame, area: Rect, app: &App) {
    let [top, rule] = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(area);
    let mut spans = vec![
        Span::styled(" ◆ ", theme::s(theme::accent())),
        Span::styled("sing", theme::bold(theme::text())),
        Span::raw("   "),
    ];
    let mut underline = vec![Span::styled("─".repeat(10), theme::s(theme::faint()))];
    let tabs: Vec<(Tab, String)> = Tab::MAIN
        .iter()
        .enumerate()
        .map(|(i, t)| (*t, format!("{} {}", i + 1, t.name())))
        .collect();
    for (tab, name) in tabs {
        let on = tab == app.tab;
        let w = text::width(&name);
        spans.push(Span::styled(
            name.clone(),
            if on {
                theme::bold(theme::accent())
            } else {
                theme::s(theme::dim())
            },
        ));
        spans.push(Span::raw("   "));
        underline.push(Span::styled(
            if on { "━".repeat(w) } else { "─".repeat(w) },
            theme::s(if on { theme::accent() } else { theme::faint() }),
        ));
        underline.push(Span::styled("───", theme::s(theme::faint())));
    }
    let used: usize = underline.iter().map(|s| text::width(&s.content)).sum();
    underline.push(Span::styled(
        "─".repeat((area.width as usize).saturating_sub(used)),
        theme::s(theme::faint()),
    ));
    f.render_widget(Paragraph::new(Line::from(spans.clone())), top);
    f.render_widget(Paragraph::new(Line::from(underline)), rule);

    // Right side: status and traffic.
    let s = &app.snap;
    let mut status = vec![];
    if let Some((label, _)) = &app.busy {
        status.push(Span::styled(
            format!("{} {label}… ", spinner()),
            theme::s(theme::warn()),
        ));
    } else if s.connected {
        status.push(Span::styled("● ", theme::s(theme::good())));
        status.push(Span::styled("Running", theme::s(theme::text())));
        if s.started_at > 0 {
            status.push(Span::styled(
                format!(
                    " {}",
                    text::duration(crate::model::now().saturating_sub(s.started_at))
                ),
                theme::s(theme::dim()),
            ));
        }
        if app.tab != Tab::Overview
            && s.api_ready
            && s.status.traffic_available
            && app.snapshot_at.elapsed().as_secs() < 6
        {
            status.push(Span::styled(
                format!(
                    "   ↓ {}  ↑ {} ",
                    text::rate(s.status.downlink),
                    text::rate(s.status.uplink)
                ),
                theme::s(theme::dim()),
            ));
        } else {
            status.push(Span::raw(" "));
        }
    } else {
        status.push(Span::styled("○ ", theme::s(theme::dim())));
        status.push(Span::styled("Stopped ", theme::s(theme::dim())));
    }
    let status_w: usize = status.iter().map(|s| text::width(&s.content)).sum();
    let tabs_w: usize = spans.iter().map(|s| text::width(&s.content)).sum();
    if (area.width as usize) > tabs_w + status_w + 1 {
        f.render_widget(Paragraph::new(Line::from(status)).right_aligned(), top);
    }
}

pub fn hint_line(f: &mut Frame, area: Rect, app: &App, hints: &[(&str, &str)]) {
    let mut spans = vec![Span::raw(" ")];
    let mut used = 1;
    for (k, label) in hints {
        let gap = if area.width < 80 { 1 } else { 3 };
        let needed = text::width(k) + text::width(label) + 1 + gap;
        if used + needed > area.width as usize {
            break;
        }
        used += needed;
        spans.push(Span::styled(k.to_string(), theme::key()));
        spans.push(Span::styled(
            format!(" {label}{}", " ".repeat(gap)),
            theme::s(theme::dim()),
        ));
    }
    let hint_width: usize = spans.iter().map(|s| text::width(&s.content)).sum();
    if let Some(t) = &app.toast {
        let icon = if t.error { "✕ " } else { "✓ " };
        let color = if t.error { theme::bad() } else { theme::good() };
        let room = (area.width as usize).saturating_sub(4);
        let toast = text::fit(&t.text, room.saturating_sub(2));
        let toast_w = text::width(&toast) + 3;
        if hint_width + toast_w > area.width as usize {
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::raw(" "),
                    Span::styled(icon, theme::s(color)),
                    Span::styled(toast, theme::s(color)),
                ])),
                area,
            );
            return;
        }
        f.render_widget(Paragraph::new(Line::from(spans)), area);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(icon, theme::s(color)),
                Span::styled(format!("{toast} "), theme::s(color)),
            ]))
            .right_aligned(),
            area,
        );
        return;
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

struct Segment {
    key: &'static str,
    label: &'static str,
    value: String,
    color: ratatui::style::Color,
}

pub fn controls(f: &mut Frame, area: Rect, app: &App) {
    let s = &app.snap;
    let settings = &s.store.settings;
    let bar = Style::default().bg(theme::panel());
    f.render_widget(Paragraph::new("").style(bar), area);
    if !app.modals.is_empty()
        || (app.tab == Tab::Activity && super::activity::capturing(app))
        || (app.tab == Tab::Proxies && super::proxies::capturing(app))
    {
        f.render_widget(
            Paragraph::new(" Global shortcuts paused · finish or cancel to return")
                .style(bar.fg(theme::dim())),
            area,
        );
        return;
    }
    let mode = match settings.route_mode.as_str() {
        "global" => "Global",
        "direct" => "Direct",
        _ => "Rule",
    };
    let tun_on = app.tun_configured();
    let tun_live = s.running_tun;
    let segments = [
        Segment {
            key: "s",
            label: if s.connected { "Stop" } else { "Start" },
            value: String::new(),
            color: if s.connected {
                theme::good()
            } else {
                theme::accent()
            },
        },
        Segment {
            key: "m",
            label: if area.width < 72 { "" } else { "Mode" },
            value: mode.into(),
            color: theme::accent(),
        },
        Segment {
            key: "t",
            label: "TUN",
            value: match (tun_on, s.connected, tun_live) {
                (true, true, false) => "On*".into(),
                (false, true, true) => "Off*".into(),
                (true, _, _) => "On".into(),
                _ => "Off".into(),
            },
            color: if tun_on { theme::good() } else { theme::dim() },
        },
    ];
    let compact = area.width < 96;
    let mut spans = vec![Span::styled(
        if s.connected { " ● " } else { " ○ " },
        Style::default()
            .fg(if s.connected {
                theme::good()
            } else {
                theme::dim()
            })
            .bg(theme::panel()),
    )];
    for (i, seg) in segments.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(
                " │ ",
                Style::default().fg(theme::faint()).bg(theme::panel()),
            ));
        }
        spans.push(Span::styled(
            seg.key,
            Style::default()
                .fg(theme::accent())
                .bg(theme::panel())
                .add_modifier(Modifier::BOLD),
        ));
        let label = if compact && seg.label == "System Proxy" {
            "Proxy"
        } else {
            seg.label
        };
        let label_color = if seg.value.is_empty() {
            seg.color
        } else {
            theme::dim()
        };
        spans.push(Span::styled(
            if label.is_empty() {
                String::new()
            } else {
                format!(" {label}")
            },
            Style::default().fg(label_color).bg(theme::panel()),
        ));
        if !seg.value.is_empty() {
            spans.push(Span::styled(
                format!(" {}", seg.value),
                Style::default()
                    .fg(seg.color)
                    .bg(theme::panel())
                    .add_modifier(Modifier::BOLD),
            ));
        }
    }
    f.render_widget(Paragraph::new(Line::from(spans)).style(bar), area);
    let mut right = vec![];
    if s.dirty {
        right.push(Span::styled(
            "A",
            Style::default()
                .fg(theme::warn())
                .bg(theme::panel())
                .add_modifier(Modifier::BOLD),
        ));
        right.push(Span::styled(
            if compact {
                " Apply "
            } else {
                " Apply changes "
            },
            Style::default().fg(theme::warn()).bg(theme::panel()),
        ));
        right.push(Span::styled(
            " │ ",
            Style::default().fg(theme::faint()).bg(theme::panel()),
        ));
    }
    let config_on = app.tab == Tab::Config;
    right.push(Span::styled(
        ":",
        Style::default()
            .fg(theme::accent())
            .bg(theme::panel())
            .add_modifier(Modifier::BOLD),
    ));
    right.push(Span::styled(
        " Config ",
        Style::default()
            .fg(if config_on {
                theme::accent()
            } else {
                theme::dim()
            })
            .bg(theme::panel())
            .add_modifier(if config_on {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }),
    ));
    right.push(Span::styled(
        "│ ",
        Style::default().fg(theme::faint()).bg(theme::panel()),
    ));
    right.push(Span::styled(
        "?",
        Style::default()
            .fg(theme::accent())
            .bg(theme::panel())
            .add_modifier(Modifier::BOLD),
    ));
    right.push(Span::styled(
        " Help ",
        Style::default().fg(theme::dim()).bg(theme::panel()),
    ));
    let left_w: usize = {
        let mut w = 3;
        for (i, seg) in segments.iter().enumerate() {
            w += if i > 0 { 3 } else { 0 }
                + 1
                + 1
                + seg.label.len()
                + if seg.value.is_empty() {
                    0
                } else {
                    1 + seg.value.len()
                };
        }
        w
    };
    let right_w: usize = right.iter().map(|s| text::width(&s.content)).sum();
    if left_w + right_w < area.width as usize {
        f.render_widget(Paragraph::new(Line::from(right)).right_aligned(), area);
    }
}

// ---- actions --------------------------------------------------------------------------
pub fn start_stop(app: &mut App) {
    if app.snap.connected {
        app.request_busy(Action::Disconnect, "Stopping", Box::new(notify));
        return;
    }
    if app.snap.store.native.is_none() {
        app.request(
            Action::ReviewMigration,
            Box::new(|app, r| {
                if !r.ok {
                    return notify(app, r);
                }
                let Some(confirm) = r.confirm.clone() else {
                    return notify(app, r);
                };
                app.push(
                    Confirm::new(
                        "Upgrade saved configuration",
                        "sing now keeps a native sing-box configuration. Your current state is backed up privately first; nothing starts yet.",
                        "Upgrade",
                        Box::new(move |app| app.send(confirm)),
                    ),
                );
            }),
        );
        return;
    }
    if app.snap.core.is_empty() {
        app.error("No sing-box core found. Open Config (,) → Core to download one.");
        return;
    }
    // Starting is not destructive; skip the review and apply the current draft.
    app.request_busy(
        Action::ReviewApply,
        "Checking",
        Box::new(|app, r| {
            if !r.ok {
                return notify(app, r);
            }
            if let Some(action) = r.confirm {
                app.request_busy(action, "Starting", Box::new(notify));
            }
        }),
    );
}

pub fn toggle_tun(app: &mut App) {
    if app.snap.store.native.is_none() {
        app.error("Start once to upgrade the configuration before using TUN");
        return;
    }
    let enable = !app.tun_configured();
    let go = move |app: &mut App| {
        app.request(
            Action::ReadNative("/inbounds".into()),
            Box::new(move |app, r| {
                let Some(edit) = r.edit.filter(|_| r.ok) else {
                    return app.error(r.message);
                };
                app.request_busy(
                    Action::SetTun {
                        enabled: enable,
                        revision: edit.revision,
                    },
                    "Saving",
                    Box::new(|app, r| {
                        if !r.ok {
                            return notify(app, r);
                        }
                        match r.confirm.clone() {
                            Some(apply) => app.push(Confirm::new(
                                "Restart core?",
                                format!(
                                    "{}\n\nRestarting briefly interrupts open connections.",
                                    r.message
                                ),
                                "Restart now",
                                Box::new(move |app| {
                                    app.request_busy(apply, "Restarting", Box::new(notify))
                                }),
                            )),
                            None => notify(app, r),
                        }
                    }),
                );
            }),
        );
    };
    if enable {
        let mut body = String::from("TUN captures traffic from every app, including those that ignore proxy settings. It needs administrator access when the core starts.");
        if app.snap.ssh {
            body.push_str("\n\nYou are connected over SSH: TUN changes routing on the remote host and can cut this session.");
        }
        app.push(Confirm::new("Turn on TUN?", body, "Turn on", Box::new(go)));
    } else {
        go(app);
    }
}

pub fn toggle_system_proxy(app: &mut App) {
    if !cfg!(target_os = "macos") || app.snap.ssh {
        app.error("System proxy switching is available for local macOS sessions. Use TUN or configure apps to use the proxy port.");
        return;
    }
    let on = !app.system_proxy_on();
    app.request_busy(
        Action::SetSystemProxy(on),
        if on { "Enabling" } else { "Restoring" },
        Box::new(notify),
    );
}

// ---- Mode menu ----------------------------------------------------------------------------
pub struct ModeMenu {
    selected: usize,
}
const MODES: [(&str, &str, &str); 3] = [
    (
        "rule",
        "Rule",
        "Your rules decide; unmatched traffic uses the final target",
    ),
    ("global", "Global", "Everything goes through one proxy"),
    ("direct", "Direct", "Everything connects directly"),
];
impl ModeMenu {
    pub fn new(app: &App) -> Self {
        Self {
            selected: MODES
                .iter()
                .position(|m| m.0 == app.snap.store.settings.route_mode)
                .unwrap_or(0),
        }
    }
}
fn global_picker(app: &App) -> Picker {
    let choices: Vec<Choice> = super::labels::targets(&app.snap.store, app.doc(), false);
    Picker::single(
        "Global target",
        choices,
        &app.snap.store.settings.global_target,
        Box::new(|app, v| {
            if let Some(target) = v.into_iter().next() {
                app.request(Action::SetGlobalTarget(target), Box::new(notify));
            }
        }),
    )
}
impl Modal for ModeMenu {
    any!();
    fn key(&mut self, app: &mut App, k: KeyEvent) -> Outcome {
        match k.code {
            K::Esc | K::Char('m') => return Outcome::Close,
            K::Down | K::Char('j') => self.selected = (self.selected + 1) % 3,
            K::Up | K::Char('k') => self.selected = (self.selected + 2) % 3,
            K::Char('r') => self.selected = 0,
            K::Char('g') => return Outcome::Replace(Box::new(global_picker(app))),
            K::Right if self.selected == 1 => {
                return Outcome::Replace(Box::new(global_picker(app)))
            }
            K::Enter => {
                let mode = MODES[self.selected].0.to_string();
                if mode != app.snap.store.settings.route_mode {
                    app.request(Action::SetMode(mode), Box::new(notify));
                }
                return Outcome::Close;
            }
            _ => {}
        }
        Outcome::Stay
    }
    fn draw(&self, f: &mut Frame, area: Rect, app: &App) {
        let r = modal::popup(area, 66, 10);
        let r = Rect {
            y: area.bottom().saturating_sub(r.height),
            ..r
        };
        let inner = modal::frame(
            f,
            r,
            "Mode",
            if app.snap.connected {
                "switches live"
            } else {
                "used on start"
            },
        );
        let current = &app.snap.store.settings.route_mode;
        let mut lines = vec![];
        for (i, (id, name, help)) in MODES.iter().enumerate() {
            let on = current == id;
            let mut spans = vec![
                Span::styled(if on { "● " } else { "○ " }, theme::s(theme::accent())),
                Span::styled(text::cell(name, 8), theme::bold(theme::text())),
                Span::styled(help.to_string(), theme::s(theme::dim())),
            ];
            if *id == "global" {
                spans = vec![
                    Span::styled(if on { "● " } else { "○ " }, theme::s(theme::accent())),
                    Span::styled(text::cell(name, 8), theme::bold(theme::text())),
                    Span::styled("Everything → ", theme::s(theme::dim())),
                    Span::styled(
                        app.label(&app.snap.store.settings.global_target),
                        theme::s(theme::target(&app.snap.store.settings.global_target)),
                    ),
                    Span::styled("   g change", theme::s(theme::faint())),
                ];
            }
            lines.push(modal::row(i == self.selected, spans));
            lines.push(Line::raw(""));
        }
        lines.pop();
        f.render_widget(Paragraph::new(lines), inner);
    }
    fn hints(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("↑↓", "move"),
            ("enter", "switch"),
            ("g", "global target"),
            ("esc", "close"),
        ]
    }
}
