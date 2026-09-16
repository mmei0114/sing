use super::*;
mod overview;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap,
    },
    Frame,
};
const ACCENT: Color = Color::Rgb(114, 216, 191);
const MUTED: Color = Color::Rgb(135, 151, 169);
const BACKGROUND: Color = Color::Rgb(17, 23, 30);
const SURFACE: Color = Color::Rgb(26, 35, 44);
const BORDER: Color = Color::Rgb(56, 72, 88);
fn block(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(format!(" {title} "))
        .border_style(Style::default().fg(BORDER))
}
fn section(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::TOP)
        .padding(Padding::horizontal(1))
        .title(format!(" {title} "))
        .title_style(
            Style::default()
                .fg(Color::Rgb(218, 226, 233))
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(BORDER))
}
fn rate(bytes: i64) -> String {
    let bytes = bytes.max(0) as f64;
    if bytes >= 1_048_576.0 {
        format!("{:.1} MiB/s", bytes / 1_048_576.0)
    } else if bytes >= 1024.0 {
        format!("{:.1} KiB/s", bytes / 1024.0)
    } else {
        format!("{bytes:.0} B/s")
    }
}
fn group_member(a: &App, group: &Value) -> String {
    if a.snapshot.connected {
        if !a.snapshot.api_ready {
            return "Unknown · API unavailable".into();
        }
        a.snapshot
            .groups
            .group
            .iter()
            .find(|g| g.tag == native::tag(group))
            .map(|g| a.label(&g.selected))
            .unwrap_or_else(|| "Unknown".into())
    } else {
        group["default"]
            .as_str()
            .map(|s| a.label(s))
            .unwrap_or_else(|| "Automatic / first member".into())
    }
}
fn fact(label: &str, value: impl Into<String>) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<16}"), Style::default().fg(MUTED)),
        Span::raw(value.into()),
    ])
}
pub(super) fn details(a: &App, v: &Value) -> String {
    let display = |key: &str| {
        let value = &v[key];
        if value.is_null() {
            "Default / unavailable".into()
        } else if let Some(items) = value.as_array() {
            items.iter().map(text).collect::<Vec<_>>().join(", ")
        } else {
            text(value)
        }
    };
    if a.page == 7 {
        if let Ok(connection) = serde_json::from_value(v.clone()) {
            return observations::details(a, &connection);
        }
        let process = v
            .pointer("/process/path")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or("Unavailable");
        return format!("Destination  {}\nDomain       {}\nSource       {}\nNetwork      {}\nInbound      {}\nOutbound     {}\nRule         {}\nProcess      {}\nUploaded     {} bytes\nDownloaded   {} bytes",display("destination"),display("domain"),display("source"),display("network"),display("inbound"),a.label(&display("outbound")),display("rule"),process,display("uplink_total"),display("downlink_total"));
    }
    if [1, 2].contains(&a.page) || (a.page == 4 && a.tabs[4] == 0) {
        let mut lines = vec![
            a.label(native::tag(v)),
            format!("Type  {}", display("type")),
        ];
        for (key, label) in [
            ("server", "Server"),
            ("server_port", "Port"),
            ("listen", "Listen"),
            ("listen_port", "Port"),
            ("address", "Addresses"),
            ("path", "Path"),
            ("stack", "Stack"),
            ("dns_mode", "Interface DNS"),
            ("auto_route", "Automatic routes"),
            ("interval", "Test interval"),
            ("tolerance", "Tolerance (ms)"),
        ] {
            if v.get(key).is_some() {
                lines.push(format!("{label}  {}", display(key)));
            }
        }
        if let Some(d) = v["detour"].as_str() {
            lines.push(format!("Via  {}", a.label(d)));
        } else if a.page == 4 {
            lines.push("Via  Direct dial".into());
        }
        if v.get("domain_resolver").is_some() {
            lines.push(format!("Bootstrap  {}", display("domain_resolver")));
        }
        if let Some(members) = v["outbounds"].as_array() {
            let selected = a
                .snapshot
                .groups
                .group
                .iter()
                .find(|g| g.tag == native::tag(v))
                .map(|g| g.selected.as_str())
                .or_else(|| {
                    (!a.snapshot.connected)
                        .then(|| v["default"].as_str())
                        .flatten()
                })
                .unwrap_or("");
            lines.push(format!(
                "\n{}  {}",
                if a.snapshot.connected {
                    "Current member"
                } else {
                    "Default (core stopped)"
                },
                if selected.is_empty() {
                    "Unknown / automatic".into()
                } else {
                    a.label(selected)
                }
            ));
            lines.push("Members".into());
            for m in members {
                let tag = m.as_str().unwrap_or("");
                lines.push(format!(
                    "{} {}",
                    if tag == selected { "›" } else { " " },
                    a.label(tag)
                ));
            }
        }
        return lines.join("\n");
    }
    if a.page == 6 {
        return serde_json::to_string_pretty(&config::redacted(v)).unwrap_or_default();
    }
    v.as_object()
        .map(|m| {
            m.iter()
                .filter(|(k, _)| !["rules", "servers"].contains(&k.as_str()))
                .map(|(k, _)| format!("{}  {}", k.replace('_', " "), display(k)))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}
pub(super) fn help() -> String {
    [
        "Navigation",
        "1–5: Overview / Proxies / Routing / Network / Activity",
        "Tab / Shift+Tab: switch between content and page actions",
        "F6: focus bottom controls / return to content; Esc: return",
        "Arrow keys: choose   Enter: open / execute   Esc: return",
        "[ / ]: previous / next subpage   ,: Settings   /: filter",
        "",
        "Common tasks",
        "I: Import Subscription from any page",
        "New Group: Proxies / Proxy Groups",
        "C: Import Rule Set in Routing; includes target selection",
        "DNS: Network / DNS; Left/Right switches Servers / Rules / Options",
        "System Proxy / TUN: Network / Capture",
        "Overview: l focuses Connections; arrows browse, Enter details, Esc returns",
        "Browsing pauses the displayed sample; r returns to live sampling",
        ": Actions: This Page / Go To / Application; type to search",
        "Connections / Logs / Diagnostics: Activity",
        "",
        "Editing",
        "a Add   e Edit   E Native JSON   x Remove   J/K Move rule",
        "F2 / Ctrl+S or Save Draft button: save without applying",
        "A or Review Changes: inspect changes, then explicitly Apply",
        "Native JSON may expose credentials. Do not share that screen.",
        "",
        "Operation",
        "M Mode   c Start   d Stop   t Test selected proxy",
        "v Connectivity check (Overview)   V Core check   p Preview",
        "R Restore system proxy   q Close interface (core keeps running)",
        "",
        "Global / Direct are traffic routing overrides; DNS is not implicitly changed.",
        "System Proxy and TUN are different capture mechanisms.",
        "Over SSH these controls affect the remote host, not your local computer.",
        "Forms preserve untouched native fields. Subscription updates preserve user policy.",
    ]
    .join("\n")
}
fn navigation(a: &App, width: u16) -> Vec<Line<'static>> {
    let mut lines = vec![];
    let mut spans = vec![];
    let mut used = 0;
    for (i, page) in PAGES.iter().enumerate() {
        let label = if width < 65 {
            format!("{} {page} ", i + 1)
        } else {
            format!(" {} {page}  ", i + 1)
        };
        let size = label.len() as u16;
        if used + size > width && !spans.is_empty() {
            lines.push(Line::from(std::mem::take(&mut spans)));
            used = 0;
        }
        let style = if i == a.workspace() {
            Style::default()
                .fg(ACCENT)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(MUTED)
        };
        spans.push(Span::styled(label, style));
        used += size;
    }
    if !spans.is_empty() {
        lines.push(Line::from(spans));
    }
    lines
}
// Chrome uses quiet, explicit key hints; form buttons remain conventional buttons.
fn controls(
    a: &App,
    buttons: &[navigation::Button],
    focus: Option<usize>,
    width: u16,
) -> Vec<Line<'static>> {
    let mut lines = vec![];
    let mut spans = vec![];
    let mut used = 0;
    for (i, (label, command)) in buttons.iter().enumerate() {
        let key = match command {
            Command::Key(c) => c.to_string(),
            Command::Open(10, 0) => ",".into(),
            Command::Import => "I".into(),
            Command::Group => "g".into(),
            Command::Convert => "C".into(),
            Command::Select | Command::Details => "Enter".into(),
            Command::Actions => ":".into(),
            Command::Observe => "l".into(),
            _ => String::new(),
        };
        let size = label.len() + if key.is_empty() { 0 } else { key.len() + 1 } + 2;
        if used + size > width as usize && !spans.is_empty() {
            lines.push(Line::from(std::mem::take(&mut spans)));
            used = 0;
        }
        let selected = focus == Some(i);
        let style = if selected {
            Style::default()
                .fg(BACKGROUND)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD)
        } else if a.unavailable(*command).is_some() {
            Style::default().fg(MUTED).add_modifier(Modifier::DIM)
        } else {
            Style::default()
        };
        if !key.is_empty() {
            spans.push(Span::styled(
                format!("{key} "),
                if selected {
                    style
                } else {
                    Style::default().fg(ACCENT)
                },
            ));
        }
        spans.push(Span::styled(label.to_string(), style));
        spans.push(Span::raw("  "));
        used += size;
    }
    if !spans.is_empty() {
        lines.push(Line::from(spans));
    }
    lines
}
fn subnavigation(a: &App, width: u16) -> Vec<Line<'static>> {
    let mut lines = vec![];
    let mut spans = vec![];
    let mut used = 0;
    for (i, (label, _, _)) in a.destinations().iter().enumerate() {
        let size = label.len() + 3;
        if used + size > width as usize && !spans.is_empty() {
            lines.push(Line::from(std::mem::take(&mut spans)));
            used = 0;
        }
        spans.push(Span::styled(
            format!(" {label}  "),
            if i == a.subtab() {
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(MUTED)
            },
        ));
        used += size;
    }
    if !spans.is_empty() {
        if used + 5 <= width as usize {
            spans.push(Span::styled("[ / ]", Style::default().fg(MUTED)));
        }
        lines.push(Line::from(spans));
    }
    lines
}
fn button_lines(
    buttons: &[navigation::Button],
    focus: Option<usize>,
    width: u16,
) -> Vec<Line<'static>> {
    let mut lines = vec![];
    let mut spans = vec![];
    let mut used = 0;
    for (i, (label, _)) in buttons.iter().enumerate() {
        let label = format!("[{label}] ");
        let size = label.len() as u16;
        if used + size > width && !spans.is_empty() {
            lines.push(Line::from(std::mem::take(&mut spans)));
            used = 0;
        }
        spans.push(Span::styled(
            label,
            if focus == Some(i) {
                Style::default()
                    .bg(ACCENT)
                    .fg(Color::Rgb(17, 23, 30))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(MUTED)
            },
        ));
        used += size;
    }
    if !spans.is_empty() {
        lines.push(Line::from(spans));
    }
    lines
}
pub(super) fn draw(f: &mut Frame, a: &App) {
    let area = f.area();
    f.render_widget(
        Block::default().style(
            Style::default()
                .bg(BACKGROUND)
                .fg(Color::Rgb(218, 226, 233)),
        ),
        area,
    );
    if area.width < 54 || area.height < 18 {
        f.render_widget(
            Paragraph::new("Resize to 54 × 18 or larger.\nq exits; core keeps running."),
            area,
        );
        return;
    }
    let tabs = navigation(a, area.width);
    let global = controls(
        a,
        &a.global_buttons(),
        (a.focus == Focus::Global).then_some(a.control),
        area.width.saturating_sub(2),
    );
    let actions = controls(
        a,
        &a.buttons(),
        (a.focus == Focus::Actions).then_some(a.control),
        area.width.saturating_sub(2),
    );
    let subnav_lines = subnavigation(a, area.width);
    let notice = if a.searching {
        format!("/{}", a.filter.value)
    } else if a.busy {
        "Working…".into()
    } else {
        model::clean(&a.notice)
    };
    let notice_height = if notice.is_empty() { 0 } else { 2 };
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(tabs.len() as u16),
            Constraint::Length(subnav_lines.len() as u16),
            Constraint::Length(actions.len().max(1) as u16),
            Constraint::Min(5),
            Constraint::Length(notice_height),
            Constraint::Length(1),
            Constraint::Length(global.len() as u16 + 1),
        ])
        .split(area);
    let state = if a.snapshot.system_proxy.pending_restore {
        "RECOVERY REQUIRED"
    } else if a.snapshot.connected && a.snapshot.api_ready {
        "RUNNING"
    } else if a.snapshot.connected {
        "API UNAVAILABLE"
    } else {
        "STOPPED"
    };
    let mode = if a.snapshot.connected {
        a.snapshot
            .running_settings
            .as_ref()
            .map(|s| s.route_mode.as_str())
            .unwrap_or("unknown")
    } else {
        a.snapshot.store.settings.route_mode.as_str()
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " sing  ",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if a.demo {
                    "DEMO  ".into()
                } else if area.width >= 80 {
                    format!(
                        "{}{}  ",
                        model::clean(&a.snapshot.host)
                            .chars()
                            .take(16)
                            .collect::<String>(),
                        if a.snapshot.ssh { " [SSH]" } else { "" }
                    )
                } else if a.snapshot.ssh {
                    "SSH  ".into()
                } else {
                    String::new()
                },
                Style::default().fg(MUTED),
            ),
            Span::styled(
                state,
                Style::default().fg(if a.snapshot.system_proxy.pending_restore {
                    Color::LightRed
                } else if a.snapshot.connected && a.snapshot.api_ready {
                    ACCENT
                } else {
                    MUTED
                }),
            ),
            Span::raw(format!(" · {mode}  ")),
            Span::styled(
                if a.snapshot.dirty {
                    "Draft · A Review"
                } else {
                    "Saved"
                },
                Style::default().fg(if a.snapshot.dirty {
                    Color::Yellow
                } else {
                    MUTED
                }),
            ),
        ]))
        .style(Style::default().bg(SURFACE)),
        rows[0],
    );
    f.render_widget(Paragraph::new(tabs), rows[1]);
    f.render_widget(Paragraph::new(subnav_lines), rows[2]);
    f.render_widget(
        Paragraph::new(actions).block(Block::default().padding(Padding::horizontal(1))),
        rows[3],
    );
    let content = rows[4];
    if a.page == 0 {
        overview::draw(f, a, content);
    } else if a.page == 7 {
        overview::connections(f, a, content, true);
    } else if a.page == 8 {
        f.render_widget(
            Paragraph::new(
                a.snapshot
                    .activity
                    .iter()
                    .rev()
                    .take(content.height as usize)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .block(section("Activity · r core logs")),
            content,
        );
    } else if a.page == 9 {
        f.render_widget(Paragraph::new("r  Inspect native references and saved / running configuration\n\nThis report does not measure network performance.").wrap(Wrap{trim:false}).block(section("Diagnostics")),content);
    } else if a.page == 10 {
        let message = match a.tabs[10] {
            0 => format!("Core  {}\nVersion  {}\n\nInstall a verified core or choose an existing binary.\nInstallation does not start the proxy.", a.snapshot.core, a.snapshot.version),
            1 => "Language  English\nKeyboard  Tab for page actions; F6 for bottom controls\nNavigation  1–5 workspaces; [ / ] subpages\nLayout  Adaptive list and details\n\nNames from subscriptions keep their original language.".into(),
            _ => format!("Host  {}{}\n\nClosing this interface leaves the running core active.\nStop restores sing-owned proxy settings before stopping.\n\nNetwork configuration is under Network.", a.snapshot.host, if a.snapshot.ssh { " (remote host over SSH)" } else { "" }),
        };
        f.render_widget(
            Paragraph::new(message)
                .wrap(Wrap { trim: false })
                .block(section("Settings")),
            content,
        );
    } else if a.page == 11 {
        let tun_count = native::array(&a.doc(), "/inbounds")
            .iter()
            .filter(|v| v["type"] == "tun")
            .count();
        let text = format!("System Proxy  {}\nTUN  {}\n\nProxy ports only affect apps configured to use them.\nSystem Proxy covers apps that follow system settings.\nTUN is an inbound; Configure TUN opens that same object.\n{}",
            if a.snapshot.system_proxy.effective { "Verified" } else if a.snapshot.system_proxy.configured { "Not verified" } else { "Off" },
            if a.snapshot.running_tun { "Running" } else if tun_count > 0 { "Configured in draft" } else { "Not configured" },
            if a.snapshot.ssh { "SSH: these controls affect the remote host, not your local computer." } else { "Changes require Review before they affect the network." });
        f.render_widget(
            Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .block(section("Capture")),
            content,
        );
    } else {
        let heading = if a.page == 7 {
            let age = model::now().saturating_sub(a.connections.observed_at);
            if !a.snapshot.connected {
                "Connections · core stopped".into()
            } else if a.connections.observed_at == 0 {
                "Connections · no sample yet".into()
            } else {
                format!(
                    "Connections · {}s ago{} · {} / {}",
                    age,
                    if age > 5 { " (stale)" } else { "" },
                    a.connections.items.len(),
                    a.connections.total
                )
            }
        } else {
            let title = match (a.page, a.tabs[a.page]) {
                (2, 2) => "All Outbounds",
                (3, 1) => "Routing Options",
                (4, 0) => "DNS Servers",
                (4, 1) => "DNS Rules",
                (4, _) => "DNS Options",
                _ => a
                    .destinations()
                    .get(a.subtab())
                    .map(|d| d.0)
                    .unwrap_or("Details"),
            };
            format!(
                "{title}{}",
                if a.filter.value.is_empty() {
                    String::new()
                } else {
                    format!(" · Filter: {}", model::clean(&a.filter.value))
                }
            )
        };
        let panel = if a.page == 4 {
            let mut title = vec![Span::raw(" DNS  ")];
            for (i, name) in ["Servers", "Rules", "Options"].iter().enumerate() {
                title.push(Span::styled(
                    format!(" {name} "),
                    if a.tabs[4] == i {
                        Style::default()
                            .fg(ACCENT)
                            .add_modifier(Modifier::UNDERLINED)
                    } else {
                        Style::default().fg(MUTED)
                    },
                ));
            }
            title.push(Span::styled(" ← / → ", Style::default().fg(MUTED)));
            if !a.filter.value.is_empty() {
                title.push(Span::styled(
                    format!("Filter: {} ", model::clean(&a.filter.value)),
                    Style::default().fg(MUTED),
                ));
            }
            section("").title(Line::from(title))
        } else {
            section(&heading)
        };
        let mut inner = panel.inner(content);
        f.render_widget(panel, content);
        if a.page == 3 && a.tabs[3] == 0 && inner.height > 0 {
            let default = a
                .doc()
                .pointer("/route/final")
                .and_then(Value::as_str)
                .map(|s| a.label(s))
                .unwrap_or_else(|| "Core default (first outbound)".into());
            let footer = Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1);
            f.render_widget(
                Paragraph::new(format!("Unmatched traffic → {default}   [Options]"))
                    .style(Style::default().fg(MUTED)),
                footer,
            );
            inner.height -= 1;
        }
        let split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(if area.width >= 115 {
                vec![Constraint::Percentage(50), Constraint::Percentage(50)]
            } else {
                vec![Constraint::Percentage(100), Constraint::Length(0)]
            })
            .split(inner);
        let items = a.rows();
        let mut state = ListState::default().with_selected(Some(a.selected[a.page]));
        f.render_stateful_widget(
            List::new(
                items
                    .iter()
                    .map(|(_, label, value)| {
                        if a.page == 2 && a.tabs[2] == 0 {
                            ListItem::new(vec![
                                Line::from(vec![
                                    Span::styled(
                                        model::clean(&a.label(native::tag(value))),
                                        Style::default().add_modifier(Modifier::BOLD),
                                    ),
                                    Span::styled(
                                        format!(
                                            "   {} · {} members",
                                            text(&value["type"]),
                                            native::array(value, "/outbounds").len()
                                        ),
                                        Style::default().fg(MUTED),
                                    ),
                                ]),
                                Line::styled(
                                    model::clean(&format!(
                                        "  {} → {}",
                                        if a.snapshot.connected {
                                            "Current"
                                        } else {
                                            "Default"
                                        },
                                        group_member(a, value)
                                    )),
                                    Style::default().fg(MUTED),
                                ),
                            ])
                        } else {
                            ListItem::new(model::clean(label))
                        }
                    })
                    .collect::<Vec<_>>(),
            )
            .highlight_style(Style::default().bg(Color::Rgb(33, 53, 60)).fg(
                if a.focus == Focus::Content {
                    ACCENT
                } else {
                    MUTED
                },
            ))
            .highlight_symbol("› "),
            split[0],
            &mut state,
        );
        if items.is_empty() {
            f.render_widget(
                Paragraph::new(if a.page == 6 {
                    "No additional sections. E opens the full document."
                } else if a.page == 7 {
                    "No sampled connections. r refreshes; h toggles closed."
                } else if a.snapshot.store.native.is_none() {
                    "Initialize native configuration on Overview (u)."
                } else if a.page == 5 && a.tabs[5] == 0 {
                    "No subscriptions. Choose Import Subscription above."
                } else if a.page == 5 {
                    "No rule sets. Choose Import Rule Set above."
                } else {
                    "No matching items. Use the actions above or clear the filter."
                })
                .wrap(Wrap { trim: false }),
                split[0],
            );
        }
        if split[1].width > 0 {
            if let Some((_, _, v)) = a.current() {
                f.render_widget(
                    Paragraph::new(details(a, &v))
                        .wrap(Wrap { trim: false })
                        .block(
                            Block::default()
                                .borders(Borders::LEFT)
                                .padding(Padding::horizontal(1))
                                .border_style(Style::default().fg(BORDER))
                                .title(if a.page == 7 {
                                    "Observation · Enter details"
                                } else {
                                    "Details · e form · E native"
                                }),
                        ),
                    split[1],
                );
            }
        }
    }
    f.render_widget(
        Paragraph::new(notice)
            .style(Style::default().fg(if a.error { Color::LightRed } else { MUTED }))
            .wrap(Wrap { trim: false }),
        rows[5],
    );
    f.render_widget(
        Paragraph::new(if a.focus == Focus::Global {
            " ←→ Choose  Enter Open  Esc Back  q Quit"
        } else if a.focus == Focus::Actions {
            " ←→ Choose  Enter Open  Tab Content  q Quit"
        } else if a.focus == Focus::Connections {
            " ↑↓ Browse  Enter Details  r Live  Esc Groups  q Quit"
        } else if area.width < 70 {
            " Tab Actions  F6 Controls  / Filter  q Quit"
        } else {
            " ↑↓ Select  Enter Open  / Filter  Tab Actions  F6 Controls  q Quit"
        })
        .style(Style::default().fg(MUTED)),
        rows[6],
    );
    f.render_widget(
        Paragraph::new(if a.dialog.is_some() {
            vec![Line::styled(
                "Dialog open · Esc Back · global shortcuts paused",
                Style::default().fg(MUTED),
            )]
        } else if a.searching {
            vec![Line::styled(
                "Filtering · Enter / Esc Finish · type to search",
                Style::default().fg(MUTED),
            )]
        } else {
            global
        })
        .style(Style::default().bg(SURFACE))
        .block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(BORDER))
                .padding(Padding::horizontal(1)),
        ),
        rows[7],
    );
    if let Some(d) = &a.dialog {
        f.buffer_mut()
            .set_style(area, Style::default().add_modifier(Modifier::DIM));
        let available_width = if content.width >= 70 {
            content.width - 4
        } else {
            content.width
        };
        let available_height = rows[5].y.saturating_sub(rows[2].y);
        let compact = match d {
            Dialog::Form(form) if matches!(form.action, FormAction::Import { .. }) => {
                Some((76, form.visible_fields() as u16 * 2 + 8))
            }
            Dialog::Select { choices, .. } => Some((76, choices.len().min(16) as u16 + 4)),
            Dialog::Commands { .. } => Some((86, 18)),
            _ => None,
        };
        let width = available_width.min(compact.map_or(104, |c| c.0));
        let height = available_height.min(compact.map_or(available_height, |c| c.1));
        let dialog_area = Rect::new(
            content.x + (content.width - width) / 2,
            rows[2].y + (available_height - height) / 2,
            width,
            height,
        );
        draw_dialog(f, d, dialog_area, Some(a));
    }
}
pub(super) fn draw_dialog(f: &mut Frame, d: &Dialog, area: Rect, app: Option<&App>) {
    let label = |s: &str| app.map_or_else(|| s.to_string(), |a| a.label(s));
    f.render_widget(Clear, area);
    f.render_widget(
        Block::default().style(Style::default().bg(SURFACE).fg(Color::Rgb(218, 226, 233))),
        area,
    );
    if let Dialog::Form(form) = d {
        if matches!(form.action, FormAction::Import { .. }) {
            super::subscriptions::draw_source(f, form, area);
            return;
        }
    }
    if matches!(d, Dialog::Subscription(_) | Dialog::Setup(_)) {
        if let Some(a) = app {
            super::subscriptions::draw(f, d, area, a);
        }
        return;
    }
    if let (Dialog::Group(g) | Dialog::InlineGroup { editor: g, .. }, Some(app)) = (d, app) {
        super::group::draw(f, g, area, app, matches!(d, Dialog::InlineGroup { .. }));
        return;
    }
    if let Dialog::RuleImport(r) = d {
        if r.preview.is_none() {
            draw_dialog(f, &Dialog::Form(r.form.clone()), area, app);
            return;
        }
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(2)])
        .split(area);
    let hint = match d {
        Dialog::Form(form) => match form.action {
            FormAction::Import { .. } | FormAction::Convert => " F2 Review import  Esc Cancel\n Tab/↑↓ Field  ←→ Choice  Ctrl+U Clear",
            _ => " F2 Save draft  Esc Cancel\n Tab/↑↓ Field  Space Choose  Ctrl+U Clear",
        },
        Dialog::Json { .. } => " F2 Save draft  Esc Cancel\n Ctrl+U Clear  Arrow keys Move cursor",
        Dialog::Members { .. } => " Space Toggle  F2 Use members  Esc Back\n Returns to group form; F2 there saves the draft",
        Dialog::Text {
            action: Some(_), ..
        } => " Enter Confirm  ↑↓ Scroll  Esc Cancel",
        _ => " Enter Select  ↑↓ Move  Esc Close",
    };
    f.render_widget(
        Paragraph::new(hint).style(Style::default().fg(ACCENT)),
        rows[1],
    );
    if let Dialog::Form(form) = d {
        f.render_widget(Clear, rows[1]);
        let submit = if matches!(form.action, FormAction::Import { .. } | FormAction::Convert) {
            "Review Import"
        } else {
            "Save Draft"
        };
        let controls = [(submit, Command::Key(' ')), ("Cancel", Command::Key(' '))];
        f.render_widget(
            Paragraph::new(button_lines(
                &controls,
                form.selected.checked_sub(form.fields.len()),
                rows[1].width,
            )),
            Rect::new(rows[1].x, rows[1].y, rows[1].width, 1),
        );
        f.render_widget(
            Paragraph::new(" Tab Focus  Enter Action  F2 Save  Esc Cancel")
                .style(Style::default().fg(MUTED)),
            Rect::new(
                rows[1].x,
                rows[1].y + 1,
                rows[1].width,
                rows[1].height.saturating_sub(1),
            ),
        );
    }
    match d{
        Dialog::ApplyReview{summary,diff,expanded,focus,scroll,..}=>{f.render_widget(Paragraph::new(if *expanded{diff.as_str()}else{summary.as_str()}).wrap(Wrap{trim:false}).scroll((*scroll,0)).block(block(if *expanded{"Native Diff"}else{"Review Changes"})),rows[0]);f.render_widget(Clear,rows[1]);let labels=["Apply",if *expanded{"Summary"}else{"Native Diff"},"Back"].iter().enumerate().map(|(i,s)|format!("{}[{s}]",if *focus==i{"›"}else{" "})).collect::<Vec<_>>().join(" ");f.render_widget(Paragraph::new(format!("{labels}\nTab Focus · Enter Action · ↑↓ Scroll · Esc Back")).style(Style::default().fg(ACCENT)),rows[1]);},
        Dialog::References(b)=>{draw_choices(f,&b.title,if b.items.is_empty(){vec!["No known native tag references".into()]}else{b.items.iter().map(|(label,_)|label.clone()).collect()},b.selected,rows[0]);f.render_widget(Clear,rows[1]);f.render_widget(Paragraph::new("[Open Reference] Enter · ↑↓ Select · Esc Close\nBack to References returns to this list."),rows[1]);},
        Dialog::Failure{editor,message,scroll}=>{f.render_widget(Paragraph::new(message.as_str()).wrap(Wrap{trim:false}).scroll((*scroll,0)).block(block("Action Failed")),rows[0]);f.render_widget(Clear,rows[1]);f.render_widget(Paragraph::new(if editor.is_some(){"[Back] Enter / Esc Return with inputs preserved\n↑↓ Scroll · Retry only after reviewing the error."}else{"[Close] Enter / Esc"}).style(Style::default().fg(ACCENT)),rows[1]);},
        Dialog::Group(_)|Dialog::InlineGroup{..}|Dialog::Subscription(_)|Dialog::Setup(_)=>{},
        Dialog::SetupReview{body,scroll,..}=>{f.render_widget(Paragraph::new(body.as_str()).wrap(Wrap{trim:false}).scroll((*scroll,0)).block(block("Review Setup")),rows[0]);f.render_widget(Clear,rows[1]);f.render_widget(Paragraph::new("[Save Draft] Enter Confirm · Esc Back to Setup\nNext: review Start / Apply separately."),rows[1]);},
        Dialog::RuleReport{parent,scroll}=>f.render_widget(Paragraph::new(parent.report()).wrap(Wrap{trim:false}).scroll((*scroll,0)).block(block("Conversion Report · Esc Back")),rows[0]),
        Dialog::RuleImport(r)=>{
            let p=r.preview.as_ref().unwrap();
            let b=block("Import Rule Set · Target and Position");let inner=b.inner(rows[0]);f.render_widget(b,rows[0]);
            let mut lines=vec![Line::raw(r.preview_label()),Line::raw(format!("{} conversion warnings · DNS unchanged",p.warnings.len())),Line::raw(if r.shadow_warning(){"Warning: an earlier catch-all rule may shadow this rule."}else{"Source review is available before saving."})];
            let labels=[format!("Send matching traffic to: {}",r.choices.get(r.target).map(|v|model::clean(&v.1)).unwrap_or_default()),format!("Position: {}",r.position_label()),"[New Group]".into(),"[Conversion Report]".into(),"[Change Source]".into(),"[Save Rule to Draft]".into(),"[Cancel]".into()];
            for(i,label)in labels.iter().enumerate(){lines.push(Line::styled(format!("{} {label}",if r.focus==i{"›"}else{" "}),Style::default().fg(if r.focus==i{ACCENT}else{MUTED})));}
            let scroll=(r.focus+3).saturating_sub(inner.height.saturating_sub(1)as usize);
            f.render_widget(Paragraph::new(lines).scroll((scroll as u16,0)),inner);
            f.render_widget(Clear,rows[1]);f.render_widget(Paragraph::new(" Tab Focus  ←→ Target / Position\n Enter Action  Esc Back").style(Style::default().fg(ACCENT)),rows[1]);
        },
        Dialog::Discard{..}=>f.render_widget(Paragraph::new("Discard unsaved changes?\n\nEnter Discard · Esc Keep editing").block(block("Unsaved Changes")),rows[0]),
        Dialog::Commands{choices,query,selected}=>draw_palette(f,choices,query,*selected,rows[0],app),
        Dialog::Text{title,text,scroll,..}=>f.render_widget(Paragraph::new(text.as_str()).wrap(Wrap{trim:false}).scroll((*scroll,0)).block(block(title)),rows[0]),
        Dialog::Form(form)=>{let inner=block(&form.title).inner(rows[0]);f.render_widget(block(&form.title),rows[0]);let visible=(inner.height as usize/2).max(1);let start=form.selected.saturating_sub(visible-1);let mut lines=vec![];for(i,field)in form.fields.iter().enumerate().skip(start).take(visible){lines.push(Line::styled(format!("{} {}",if i==form.selected{"›"}else{" "},field.label),Style::default().fg(if i==form.selected{ACCENT}else{MUTED})));let value=if ["source","url"].contains(&field.key.as_str())&&!field.input.value.is_empty(){"[private value · Ctrl+U to replace]".into()}else{if matches!(field.kind,Kind::Choice(_)){label(&field.input.value)}else if matches!(field.kind,Kind::Members(_)){serde_json::from_str::<Vec<String>>(&field.input.value).unwrap_or_default().iter().map(|s|label(s)).collect::<Vec<_>>().join(", ")}else{model::clean(&field.input.value)}};lines.push(Line::raw(format!("  {}",if value.is_empty(){"(default / omitted)"}else{&value})));}f.render_widget(Paragraph::new(lines),inner);},
        Dialog::Json{edit,input}=>{let b=block(if edit.pointer.is_empty(){"Native document · credentials visible"}else{&edit.pointer});let inner=b.inner(rows[0]);f.render_widget(b,rows[0]);let line=input.value[..input.cursor].bytes().filter(|c|*c==b'\n').count();let col=input.value[..input.cursor].rsplit('\n').next().unwrap_or("").chars().count();let sy=line.saturating_sub(inner.height.saturating_sub(1)as usize);let sx=col.saturating_sub(inner.width.saturating_sub(1)as usize);f.render_widget(Paragraph::new(input.value.as_str()).scroll((sy as u16,sx as u16)),inner);f.set_cursor_position((inner.x+(col-sx)as u16,inner.y+(line-sy)as u16));},
        Dialog::Add{choices,selected}=>draw_choices(f,"Add object",choices.iter().map(|(s,_)|s.clone()).collect(),*selected,rows[0]),
        Dialog::Members{choices,chosen,selected,..}=>draw_choices(f,"Members · Space toggles",choices.iter().map(|s|format!("[{}] {}",if chosen.contains(s){"x"}else{" "},label(s))).collect(),*selected,rows[0]),
        Dialog::Select{group,choices,selected}=>draw_choices(f,&label(group),choices.iter().map(|s|label(s)).collect(),*selected,rows[0]),
        Dialog::Auth{kind,..}=>f.render_widget(Paragraph::new(format!("Administrator authorization: {kind}\n\nEnter opens sudo. sing never receives your password.\nEsc cancels without enabling takeover.")).wrap(Wrap{trim:false}).block(block("Authorization")),rows[0]),
    }
}
fn draw_palette(
    f: &mut Frame,
    choices: &[navigation::PaletteItem],
    query: &Input,
    selected: usize,
    area: Rect,
    app: Option<&App>,
) {
    let mut items = vec![];
    let mut previous = "";
    let mut visual_selected = 0;
    for (i, item) in choices
        .iter()
        .filter(|item| {
            item.label
                .to_lowercase()
                .contains(&query.value.to_lowercase())
        })
        .enumerate()
    {
        if previous != item.section {
            items.push(ListItem::new(Line::styled(
                item.section,
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )));
            previous = item.section;
        }
        if i == selected {
            visual_selected = items.len();
        }
        let reason = app.and_then(|app| app.unavailable(item.command));
        items.push(
            ListItem::new(format!(
                "  {}{}",
                item.label,
                reason.map(|s| format!(" · {s}")).unwrap_or_default()
            ))
            .style(if reason.is_some() {
                Style::default().fg(MUTED)
            } else {
                Style::default()
            }),
        );
    }
    if items.is_empty() {
        items.push(ListItem::new("No matching actions"));
    }
    let mut state = ListState::default().with_selected(Some(visual_selected));
    f.render_stateful_widget(
        List::new(items)
            .block(block(&format!("Actions · {}", query.value)))
            .highlight_symbol("› ")
            .highlight_style(Style::default().bg(Color::Rgb(33, 53, 60))),
        area,
        &mut state,
    );
}
fn draw_choices(f: &mut Frame, title: &str, choices: Vec<String>, selected: usize, area: Rect) {
    let mut s = ListState::default().with_selected(Some(selected));
    f.render_stateful_widget(
        List::new(choices.into_iter().map(ListItem::new).collect::<Vec<_>>())
            .block(block(title))
            .highlight_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
            .highlight_symbol("› "),
        area,
        &mut s,
    );
}
