//! Overview: is it working, how much is flowing, which proxy is in use.
use super::{flows, labels, modal, notify, proxies, text, theme, App, Tab};
use crate::{native, runtime::Action};
use crossterm::event::{KeyCode as K, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Paragraph, Sparkline},
    Frame,
};

#[derive(Default)]
pub struct State {
    pub group: usize,
}

pub fn groups(app: &App) -> Vec<String> {
    native::array(app.doc(), "/outbounds")
        .iter()
        .filter(|v| labels::is_group(v))
        .map(|v| native::tag(v).to_string())
        .collect()
}

fn ready(app: &App) -> (bool, bool, bool) {
    let core = !app.snap.core.is_empty();
    let nodes = !app.snap.store.nodes.is_empty()
        || native::array(app.doc(), "/outbounds").iter().any(|v| {
            !["selector", "urltest", "direct", "block", "dns"]
                .contains(&v["type"].as_str().unwrap_or(""))
        });
    (core, nodes, app.snap.connected)
}

pub fn key(app: &mut App, k: KeyEvent) {
    let n = groups(app).len();
    match k.code {
        K::Down | K::Char('j') => {
            app.overview.group = (app.overview.group + 1).min(n.saturating_sub(1))
        }
        K::Up | K::Char('k') => app.overview.group = app.overview.group.saturating_sub(1),
        K::Enter => {
            if let Some(tag) = groups(app).get(app.overview.group) {
                proxies::choose_member(app, tag.clone());
            } else {
                let (core, nodes, _) = ready(app);
                if !core {
                    app.go(Tab::Config);
                } else if !nodes {
                    flows::import_subscription(app);
                }
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
            app.request_busy(Action::Probe, "Checking", Box::new(notify));
        }
        _ => {}
    }
}

pub fn hints(app: &App) -> Vec<(&'static str, &'static str)> {
    let (core, nodes, _) = ready(app);
    if !core || !nodes {
        return vec![
            ("enter", "next step"),
            ("i", "import subscription"),
            (",", "config"),
        ];
    }
    let mut hints = vec![
        ("↑↓", "group"),
        ("enter", "choose proxy"),
        ("l", "test latency"),
        ("v", "check connection"),
        ("i", "import"),
    ];
    if cfg!(target_os = "macos") && !app.snap.ssh {
        hints.push(("p", "system proxy"));
    }
    hints
}

fn title(label: &str, right: &str, width: u16) -> Line<'static> {
    let used = text::width(label) + text::width(right) + 4;
    Line::from(vec![
        Span::styled(format!("{label} "), theme::bold(theme::dim())),
        Span::styled(
            "─".repeat((width as usize).saturating_sub(used)),
            theme::s(theme::faint()),
        ),
        Span::styled(format!(" {right}"), theme::s(theme::faint())),
    ])
}

pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let area = Rect {
        x: area.x + 1,
        width: area.width.saturating_sub(2),
        y: area.y + 1,
        height: area.height.saturating_sub(1),
    };
    let wide = area.width >= 100;
    if wide {
        let [left, _, right] = Layout::horizontal([
            Constraint::Length(46),
            Constraint::Length(3),
            Constraint::Min(30),
        ])
        .areas(area);
        let [status, traffic, _] = Layout::vertical([
            Constraint::Length(8),
            Constraint::Length(11.min(left.height.saturating_sub(8))),
            Constraint::Min(0),
        ])
        .areas(left);
        draw_status(f, status, app);
        draw_traffic(f, traffic, app);
        let group_h = (groups(app).len() as u16 + 3).clamp(5, right.height / 2 + 2);
        let [g, _, apps] = Layout::vertical([
            Constraint::Length(group_h),
            Constraint::Length(1),
            Constraint::Min(3),
        ])
        .areas(right);
        draw_groups(f, g, app);
        draw_apps(f, apps, app);
    } else {
        let group_h = (groups(app).len() as u16 + 2).clamp(4, 9);
        let [status, _, g, _, traffic] = Layout::vertical([
            Constraint::Length(8),
            Constraint::Length(1),
            Constraint::Length(group_h),
            Constraint::Length(1),
            Constraint::Min(3),
        ])
        .areas(area);
        draw_status(f, status, app);
        draw_groups(f, g, app);
        if traffic.height >= 7 {
            let [t, a] =
                Layout::vertical([Constraint::Length(4), Constraint::Min(3)]).areas(traffic);
            draw_traffic(f, t, app);
            draw_apps(f, a, app);
        } else {
            draw_traffic(f, traffic, app);
        }
    }
}

fn kv(k: &str, v: Vec<Span<'static>>) -> Line<'static> {
    let mut spans = vec![Span::styled(text::cell(k, 10), theme::s(theme::dim()))];
    spans.extend(v);
    Line::from(spans)
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    let (core, nodes, running) = ready(app);
    let s = &app.snap;
    if !core || !nodes {
        let step = |done: bool, n: &str, label: &str, how: &str| {
            Line::from(vec![
                Span::styled(
                    if done { "  ✓ " } else { "  ○ " },
                    theme::s(if done { theme::good() } else { theme::accent() }),
                ),
                Span::styled(format!("{n}  "), theme::s(theme::dim())),
                Span::styled(
                    text::cell(label, 22),
                    theme::s(if done { theme::dim() } else { theme::text() }),
                ),
                Span::styled(how.to_string(), theme::s(theme::faint())),
            ])
        };
        let lines = vec![
            title("GET STARTED", "", area.width),
            Line::raw(""),
            step(
                core,
                "1",
                "Download sing-box core",
                if core { "" } else { ", → Core" },
            ),
            step(
                nodes,
                "2",
                "Import a subscription",
                if nodes { "" } else { "i" },
            ),
            step(running, "3", "Start", "s"),
            Line::raw(""),
            Line::styled(
                "  Nothing changes on your network until you start.",
                theme::s(theme::faint()),
            ),
        ];
        f.render_widget(Paragraph::new(lines), area);
        return;
    }
    let settings = &s.store.settings;
    let state = if running {
        vec![
            Span::styled("● Running", theme::bold(theme::good())),
            Span::styled(
                if s.started_at > 0 {
                    format!(
                        "  for {}",
                        text::duration(crate::model::now().saturating_sub(s.started_at))
                    )
                } else {
                    String::new()
                },
                theme::s(theme::dim()),
            ),
        ]
    } else {
        vec![
            Span::styled("○ Stopped", theme::bold(theme::dim())),
            Span::styled("  s to start", theme::s(theme::faint())),
        ]
    };
    let mode = match settings.route_mode.as_str() {
        "global" => vec![
            Span::styled("Global", theme::s(theme::text())),
            Span::styled(" → ", theme::s(theme::dim())),
            Span::styled(
                app.label(&settings.global_target),
                theme::s(theme::target(&settings.global_target)),
            ),
        ],
        "direct" => vec![Span::styled("Direct", theme::s(theme::text()))],
        _ => {
            let final_tag = app
                .doc()
                .pointer("/route/final")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            vec![
                Span::styled("Rule", theme::s(theme::text())),
                Span::styled(
                    format!(
                        "  {} rules, else ",
                        native::array(app.doc(), "/route/rules").len()
                    ),
                    theme::s(theme::dim()),
                ),
                Span::styled(app.label(final_tag), theme::s(theme::target(final_tag))),
            ]
        }
    };
    let port = native::array(app.doc(), "/inbounds")
        .iter()
        .find(|i| i["type"] == "mixed" || i["type"] == "http" || i["type"] == "socks")
        .map(|i| {
            format!(
                "{}:{}",
                i["listen"].as_str().unwrap_or("127.0.0.1"),
                i["listen_port"]
            )
        })
        .unwrap_or_else(|| "no proxy port".into());
    let mut capture = vec![Span::styled(port, theme::s(theme::text()))];
    if app.system_proxy_on() {
        capture.push(Span::styled(" · System proxy", theme::s(theme::good())));
    }
    if app.tun_configured() {
        capture.push(Span::styled(" · TUN", theme::s(theme::good())));
    }
    let check = &s.connectivity;
    let check_line = match check.state.as_str() {
        "passed" => vec![
            Span::styled("✓ Reachable", theme::s(theme::good())),
            Span::styled(
                format!(
                    "  {} ago",
                    text::ago(crate::model::now().saturating_sub(check.checked_at))
                ),
                theme::s(theme::dim()),
            ),
        ],
        "failed" => vec![Span::styled(
            text::fit(&format!("✕ {}", check.detail), 34),
            theme::s(theme::bad()),
        )],
        _ => vec![Span::styled(
            if running {
                "Not checked  v to check"
            } else {
                "—"
            },
            theme::s(theme::faint()),
        )],
    };
    let version = s
        .version
        .split_whitespace()
        .last()
        .unwrap_or("")
        .trim_start_matches('v')
        .to_string();
    let host = if s.ssh {
        format!(" · SSH {}", s.host)
    } else {
        String::new()
    };
    let lines = vec![
        title("STATUS", "", area.width),
        kv("", state),
        kv("Mode", mode),
        kv("Capture", capture),
        kv("Check", check_line),
        kv(
            "Core",
            vec![Span::styled(
                format!("sing-box {version}{host}"),
                theme::s(theme::dim()),
            )],
        ),
    ];
    f.render_widget(Paragraph::new(lines), area);
}

fn draw_traffic(f: &mut Frame, area: Rect, app: &App) {
    let s = &app.snap;
    let [head, spark, foot] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);
    let right = if s.connected && s.api_ready && s.status.traffic_available {
        format!(
            "↓ {}   ↑ {}",
            text::rate(s.status.downlink),
            text::rate(s.status.uplink)
        )
    } else {
        String::new()
    };
    f.render_widget(
        Paragraph::new(vec![title("TRAFFIC", &right, area.width)]),
        head,
    );
    let width = spark.width as usize;
    let data: Vec<u64> = app
        .traffic
        .iter()
        .rev()
        .take(width)
        .rev()
        .map(|(d, u)| (*d + *u).max(0) as u64)
        .collect();
    if data.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                if s.connected {
                    "Waiting for samples…"
                } else {
                    "Start the core to see traffic."
                },
                theme::s(theme::faint()),
            )),
            spark,
        );
    } else {
        f.render_widget(
            Sparkline::default()
                .data(&data)
                .style(theme::s(theme::accent())),
            spark,
        );
    }
    if s.connected {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Session  ", theme::s(theme::dim())),
                Span::styled(
                    format!(
                        "↓ {}  ↑ {}",
                        text::bytes(s.status.downlink_total),
                        text::bytes(s.status.uplink_total)
                    ),
                    theme::s(theme::text()),
                ),
                Span::styled(
                    format!(
                        " · {} conns",
                        app.history
                            .total_live
                            .max(s.status.connections_out as usize)
                    ),
                    theme::s(theme::dim()),
                ),
            ])),
            foot,
        );
    }
}

fn draw_groups(f: &mut Frame, area: Rect, app: &App) {
    let tags = groups(app);
    let mut lines = vec![title(
        "PROXY GROUPS",
        if app.snap.connected {
            "live"
        } else {
            "defaults"
        },
        area.width,
    )];
    if tags.is_empty() {
        lines.push(Line::styled(
            "  No groups yet · 2 → Groups → n",
            theme::s(theme::faint()),
        ));
    }
    let height = area.height.saturating_sub(1) as usize;
    let start = modal::scroll(app.overview.group, height, tags.len());
    let w = area.width as usize;
    for (i, tag) in tags.iter().enumerate().skip(start).take(height) {
        let v = native::array(app.doc(), "/outbounds")
            .iter()
            .find(|v| native::tag(v) == tag)
            .cloned()
            .unwrap_or_default();
        let (current, delay) = proxies::current_member(app, tag, &v);
        let kind = labels::protocol(v["type"].as_str().unwrap_or(""));
        let name_w = (w / 3).clamp(10, 24);
        let member_w = w.saturating_sub(name_w + 8 + 10 + 3);
        lines.push(modal::row(
            i == app.overview.group,
            vec![
                Span::styled(
                    text::cell(&app.label(tag), name_w),
                    theme::bold(theme::text()),
                ),
                Span::styled(text::cell(kind, 8), theme::s(theme::dim())),
                Span::styled(
                    text::cell(&app.label(&current), member_w),
                    theme::s(theme::target(&current)),
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

fn draw_apps(f: &mut Frame, area: Rect, app: &App) {
    let mut apps = app.history.apps();
    apps.sort_by_key(|a| std::cmp::Reverse(a.up + a.down));
    let mut lines = vec![title("TOP APPS", "3 Activity", area.width)];
    if apps.is_empty() {
        lines.push(Line::styled(
            if app.snap.connected {
                "  No traffic observed yet."
            } else {
                "  —"
            },
            theme::s(theme::faint()),
        ));
    }
    let max = apps.first().map(|a| a.up + a.down).unwrap_or(1).max(1);
    let w = area.width as usize;
    let bar_w = (w / 4).clamp(6, 24);
    for a in apps.iter().take(area.height.saturating_sub(1) as usize) {
        let total = a.up + a.down;
        let filled = ((total as f64 / max as f64) * bar_w as f64).ceil() as usize;
        let name = if a.name.is_empty() {
            "Unknown app".to_string()
        } else {
            a.name.clone()
        };
        let top_target = a
            .targets
            .iter()
            .max_by_key(|(_, n)| **n)
            .map(|(t, _)| t.clone())
            .unwrap_or_default();
        let name_w = w.saturating_sub(bar_w + 10 + 16 + 2).clamp(8, 28);
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(text::cell(&name, name_w), theme::s(theme::text())),
            Span::styled("▮".repeat(filled.min(bar_w)), theme::s(theme::accent())),
            Span::styled(
                "▯".repeat(bar_w - filled.min(bar_w)),
                theme::s(theme::faint()),
            ),
            Span::styled(text::right(&text::bytes(total), 10), theme::s(theme::dim())),
            Span::styled(
                format!("  {}", text::fit(&app.label(&top_target), 14)),
                theme::s(theme::target(&top_target)),
            ),
        ]));
    }
    f.render_widget(Paragraph::new(lines), area);
}
