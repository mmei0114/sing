//! Capture health, traffic, quick groups and live evidence.
use super::{
    activity, config, flows, history, labels, modal, proxies, quick_rule, text, theme, App,
};
use crate::{native, runtime::Action};
use crossterm::event::{KeyCode as K, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

#[derive(Default)]
pub struct State {
    pub group: usize,
    pub connection: usize,
    pub connections_focus: bool,
}
pub fn groups(app: &App) -> Vec<String> {
    native::array(app.doc(), "/outbounds")
        .iter()
        .filter(|v| labels::is_group(v))
        .map(|v| native::tag(v).to_string())
        .collect()
}
fn ready(app: &App) -> bool {
    !app.snap.core.is_empty()
        && app.snap.store.native.is_some()
        && (!native::array(app.doc(), "/outbounds").is_empty()
            || !native::array(app.doc(), "/endpoints").is_empty())
        && (!native::array(app.doc(), "/inbounds").is_empty()
            || !native::array(app.doc(), "/endpoints").is_empty())
}
fn connections(app: &App) -> Vec<&history::Entry> {
    app.observation_view()
        .entries
        .iter()
        .filter(|e| e.open)
        .collect()
}
fn needs_recovery(app: &App) -> bool {
    let p = &app.snap.system_proxy;
    p.pending_restore && (!app.snap.connected || !p.configured || !p.helper_ready)
}
pub fn key(app: &mut App, k: KeyEvent) {
    if k.code == K::Tab || k.code == K::BackTab {
        app.overview.connections_focus = !app.overview.connections_focus;
        app.resume_activity();
        app.overview.connection = 0;
        return;
    }
    if k.code == K::Char('c') {
        activity::open_all(app);
        return;
    }
    if app.overview.connections_focus {
        let n = connections(app).len();
        match k.code {
            K::Down | K::Char('j') => {
                app.pause_activity();
                app.overview.connection = (app.overview.connection + 1).min(n.saturating_sub(1));
            }
            K::Up | K::Char('k') => {
                app.pause_activity();
                app.overview.connection = app.overview.connection.saturating_sub(1);
            }
            K::PageDown => {
                app.pause_activity();
                app.overview.connection = (app.overview.connection + 8).min(n.saturating_sub(1));
            }
            K::PageUp => {
                app.pause_activity();
                app.overview.connection = app.overview.connection.saturating_sub(8);
            }
            K::Enter | K::Char('r') => {
                app.pause_activity();
                if let Some(c) = connections(app)
                    .get(app.overview.connection)
                    .map(|e| e.c.clone())
                {
                    if k.code == K::Enter {
                        app.push(flows::connection_details(app, &c));
                    } else {
                        quick_rule::from_connection(app, c, false);
                    }
                }
            }
            K::Char(' ') => {
                app.resume_activity();
                app.overview.connection = 0;
            }
            K::Esc => {
                app.overview.connections_focus = false;
                app.resume_activity();
            }
            _ => {}
        }
        return;
    }
    let n = groups(app).len();
    match k.code {
        K::Down | K::Char('j') => {
            app.overview.group = (app.overview.group + 1).min(n.saturating_sub(1))
        }
        K::Up | K::Char('k') => app.overview.group = app.overview.group.saturating_sub(1),
        K::Enter => {
            if let Some(tag) = groups(app).get(app.overview.group) {
                proxies::choose_member(app, tag.clone());
            } else if app.snap.core.is_empty() {
                config::open_menu(app);
            } else if !ready(app) {
                flows::import_subscription(app);
            }
        }
        K::Char('l') => {
            if let Some(tag) = groups(app).get(app.overview.group) {
                proxies::test_latency(app, tag.clone());
            }
        }
        K::Char('i') => flows::import_subscription(app),
        K::Char('v') => {
            if !app.snap.connected {
                return app.error("Start the core before checking the connection");
            }
            app.push(modal::Confirm::new("Check connection", "Send an HTTPS request to www.gstatic.com through the local proxy? This checks reachability, not speed or every application's capture settings.", "Check", Box::new(|app| app.request_busy(Action::Probe, "Checking", Box::new(super::notify)))));
        }
        _ => {}
    }
}
pub fn hints(app: &App) -> Vec<(&'static str, &'static str)> {
    if app.overview.connections_focus {
        return vec![
            ("↑↓", "browse"),
            ("enter", "details"),
            ("r", "rule"),
            ("space", "live"),
            ("tab", "groups"),
        ];
    }
    if !ready(app) {
        return vec![
            ("i", "import"),
            (":", "config"),
            ("s", "start"),
            ("?", "help"),
        ];
    }
    vec![
        ("↑↓", "group"),
        ("enter", "select"),
        ("tab", "connections"),
        ("l", "latency"),
        ("v", "check"),
    ]
}
pub fn title(label: &str, right: &str, width: u16, focused: bool) -> Line<'static> {
    let right = text::fit(
        right,
        (width as usize).saturating_sub(text::width(label) + 4),
    );
    let used = text::width(label) + text::width(&right) + 2;
    Line::from(vec![
        Span::styled(
            label.to_string(),
            theme::bold(if focused {
                theme::accent()
            } else {
                theme::text()
            }),
        ),
        Span::styled(
            " ".repeat((width as usize).saturating_sub(used) + 2),
            theme::s(theme::faint()),
        ),
        Span::styled(right, theme::s(theme::dim())),
    ])
}
pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let area = area.inner(ratatui::layout::Margin {
        horizontal: 2,
        vertical: 0,
    });
    if area.height < 15 && app.overview.connections_focus {
        return draw_connections(f, area, app);
    }
    let status_height = if ready(app) {
        2 + u16::from(!app.snap.selection_recovery.is_empty()) + u16::from(needs_recovery(app))
    } else {
        4
    };
    let [status, _, body] = Layout::vertical([
        Constraint::Length(status_height),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(area);
    draw_status(f, status, app);
    if body.width >= 100 {
        let [left, _, right] = Layout::horizontal([
            Constraint::Percentage(44),
            Constraint::Length(3),
            Constraint::Min(30),
        ])
        .areas(body);
        draw_groups(f, left, app);
        draw_connections(f, right, app);
    } else {
        let g_height = (groups(app).len() as u16 + 1)
            .clamp(3, 6)
            .min(body.height / 2);
        let [g, _, c] = Layout::vertical([
            Constraint::Length(g_height),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .areas(body);
        draw_groups(f, g, app);
        draw_connections(f, c, app);
    }
}
fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    if !ready(app) {
        f.render_widget(
            Paragraph::new(vec![
                title("Welcome to sing", "", area.width, false),
                Line::styled(
                    "Import a subscription, choose a group, then start.",
                    theme::s(theme::text()),
                ),
                Line::styled(
                    "i Import subscription    : Core & configuration",
                    theme::s(theme::accent()),
                ),
                Line::styled(
                    "Configuration stays in a draft until you apply it.",
                    theme::s(theme::dim()),
                ),
            ]),
            area,
        );
        return;
    }
    let s = &app.snap;
    let capture = if !s.connected {
        "Stopped · no active capture"
    } else if needs_recovery(app) {
        "System proxy needs recovery"
    } else if s.running_tun {
        "TUN active"
    } else if s.system_proxy.effective {
        "System proxy verified · proxy-aware apps"
    } else if app.system_proxy_on() {
        "System proxy not verified"
    } else {
        "Proxy ports · apps must opt in"
    };
    let check = &s.connectivity;
    let checked = check.checked_at > 0;
    let result = if checked {
        format!(
            "{} · {} · www.gstatic.com",
            if check.state == "passed" {
                "Passed"
            } else {
                "Failed"
            },
            text::age(crate::model::now().saturating_sub(check.checked_at))
        )
    } else {
        "Not checked · v to test".into()
    };
    let kv = |label: &str, value: String, color| {
        Line::from(vec![
            Span::styled(text::cell(label, 11), theme::s(theme::dim())),
            Span::styled(value, theme::s(color)),
        ])
    };
    let mut lines = vec![
        kv(
            "Capture",
            capture.into(),
            if needs_recovery(app) {
                theme::warn()
            } else {
                theme::text()
            },
        ),
        kv(
            "Last check",
            result,
            if checked && check.state != "passed" {
                theme::warn()
            } else {
                theme::dim()
            },
        ),
    ];
    if !s.selection_recovery.is_empty() {
        lines.push(kv("Attention", s.selection_recovery.clone(), theme::warn()));
    }
    if needs_recovery(app) {
        lines.push(kv(
            "Recovery",
            ": System proxy · restore before stopping".into(),
            theme::warn(),
        ));
    }
    f.render_widget(Paragraph::new(lines), area);
}
fn draw_groups(f: &mut Frame, area: Rect, app: &App) {
    let tags = groups(app);
    let mut lines = vec![title(
        "Proxy groups",
        if app.snap.connected && app.snap.api_ready {
            "live"
        } else {
            "saved defaults"
        },
        area.width,
        !app.overview.connections_focus,
    )];
    if tags.is_empty() {
        lines.push(Line::styled(
            "No groups · 2 Policies → n New",
            theme::s(theme::dim()),
        ));
    }
    let height = area.height.saturating_sub(1) as usize;
    let start = modal::scroll(app.overview.group, height, tags.len());
    let name_w = (area.width as usize / 4).clamp(8, 20);
    for (i, tag) in tags.iter().enumerate().skip(start).take(height) {
        let v = native::array(app.doc(), "/outbounds")
            .iter()
            .find(|v| native::tag(v) == tag)
            .cloned()
            .unwrap_or_default();
        let (member, delay) = proxies::current_member(app, tag, &v);
        lines.push(modal::row(
            i == app.overview.group && !app.overview.connections_focus,
            vec![
                Span::styled(text::cell(&app.label(tag), name_w), theme::s(theme::text())),
                Span::styled(" → ", theme::s(theme::dim())),
                Span::styled(
                    text::cell(
                        &app.label(&member),
                        (area.width as usize).saturating_sub(name_w + 12),
                    ),
                    theme::s(theme::target(&member)),
                ),
                Span::styled(
                    text::right(&proxies::delay_text(delay), 8),
                    theme::s(proxies::delay_color(delay)),
                ),
            ],
        ));
    }
    f.render_widget(Paragraph::new(lines), area);
}
fn draw_connections(f: &mut Frame, area: Rect, app: &App) {
    let rows = connections(app);
    let mut lines = vec![title(
        "Connections",
        if app.activity.paused {
            "Browsing · Space live"
        } else {
            "c Activity"
        },
        area.width,
        app.overview.connections_focus,
    )];
    if !app.snap.connected || rows.is_empty() {
        lines.push(Line::styled(
            if !app.snap.connected {
                "Start the core to observe connections."
            } else if !app.poll_error.is_empty() {
                "Refresh failed · see Activity"
            } else {
                "No open connections in the latest sample."
            },
            theme::s(theme::dim()),
        ));
    } else {
        let width = area.width as usize;
        let host_w = width * 45 / 100;
        if area.height > 3 {
            lines.push(Line::from(vec![
                Span::styled(
                    text::cell("DESTINATION", host_w + 1),
                    theme::s(theme::dim()),
                ),
                Span::styled("POLICY → NODE", theme::s(theme::dim())),
            ]));
        }
        let h = (area.height as usize).saturating_sub(lines.len() + 1);
        let start = modal::scroll(app.overview.connection, h, rows.len());
        for (i, e) in rows.iter().enumerate().skip(start).take(h) {
            lines.push(modal::row(
                app.overview.connections_focus && i == app.overview.connection,
                vec![
                    Span::styled(
                        format!("{}  ", text::cell(&e.host(), host_w.saturating_sub(2))),
                        theme::s(theme::text()),
                    ),
                    Span::styled(
                        text::fit(
                            &history::route_path(&e.c, |t| app.label(t)),
                            width.saturating_sub(host_w + 1),
                        ),
                        theme::s(theme::target(&history::route_target(&e.c))),
                    ),
                ],
            ));
        }
        lines.push(Line::styled(
            format!(
                "{} observed · sample {}",
                rows.len(),
                text::age(crate::model::now().saturating_sub(app.observation_view().observed_at))
            ),
            theme::s(theme::dim()),
        ));
    }
    f.render_widget(Paragraph::new(lines), area);
}
