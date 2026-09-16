use super::*;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
const ACCENT: Color = Color::Rgb(114, 216, 191);
const MUTED: Color = Color::Rgb(135, 151, 169);
fn block(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .title(format!(" {title} "))
        .border_style(Style::default().fg(Color::Rgb(56, 72, 88)))
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
        "Tab / Shift+Tab: focus controls, navigation, actions, content, Review",
        "Arrow keys: choose   Enter: open / execute   Esc: return",
        "[ / ]: previous / next subpage   ,: Settings   /: filter",
        "",
        "Common tasks",
        "Import Subscription: Overview or Proxies",
        "New Group: Proxies / Proxy Groups",
        "Import Rule Set: Routing; includes target selection",
        "DNS: Network / DNS; Servers, Rules and Options share one editor",
        "System Proxy / TUN: Network / Capture",
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
            format!(" {page} ")
        } else {
            format!(" {} {page} ", i + 1)
        };
        let size = label.len() as u16;
        if used + size > width && !spans.is_empty() {
            lines.push(Line::from(std::mem::take(&mut spans)));
            used = 0;
        }
        let style = if i == a.workspace() {
            Style::default()
                .bg(ACCENT)
                .fg(Color::Rgb(17, 23, 30))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        };
        spans.push(Span::styled(
            label,
            if a.focus == Focus::Navigation {
                style.add_modifier(Modifier::UNDERLINED)
            } else {
                style
            },
        ));
        used += size;
    }
    if !spans.is_empty() {
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
                .bg(Color::Rgb(17, 23, 30))
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
    let global = button_lines(
        &a.global_buttons(),
        (a.focus == Focus::Global).then_some(a.control),
        area.width,
    );
    let actions = button_lines(
        &a.buttons(),
        (a.focus == Focus::Actions).then_some(a.control),
        area.width,
    );
    let subnav: Vec<_> = a
        .destinations()
        .into_iter()
        .map(|(label, p, t)| (label, Command::Open(p, t)))
        .collect();
    let subnav_lines = button_lines(&subnav, Some(a.subtab()), area.width);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(global.len() as u16),
            Constraint::Length(tabs.len() as u16),
            Constraint::Length(subnav_lines.len() as u16),
            Constraint::Length(actions.len() as u16),
            Constraint::Min(5),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
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
    let capture = match (a.snapshot.system_proxy.effective, a.snapshot.running_tun) {
        (true, true) => "System + TUN",
        (true, false) => "System verified",
        (false, true) => "TUN",
        _ => "Ports / no takeover",
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
            Span::raw(format!(
                "{}{} · {state} · {capture} · {mode}{}{}",
                model::clean(&a.snapshot.host)
                    .chars()
                    .take(18)
                    .collect::<String>(),
                if a.snapshot.ssh { " [SSH]" } else { "" },
                if a.snapshot.dirty {
                    " · Draft changed"
                } else {
                    ""
                },
                if a.demo { " · DEMO" } else { "" }
            )),
        ])),
        rows[0],
    );
    f.render_widget(Paragraph::new(global), rows[1]);
    f.render_widget(Paragraph::new(tabs), rows[2]);
    f.render_widget(
        Paragraph::new(subnav_lines).style(if a.focus == Focus::Subnavigation {
            Style::default().add_modifier(Modifier::UNDERLINED)
        } else {
            Style::default()
        }),
        rows[3],
    );
    f.render_widget(Paragraph::new(actions), rows[4]);
    let content = rows[5];
    if a.page == 0 {
        let s = &a.snapshot;
        let doc = a.doc();
        let live = s.running_settings.as_ref();
        let mut lines = vec![
            format!(
                "Core  {}",
                if s.version.is_empty() {
                    "Not installed"
                } else {
                    &s.version
                }
            ),
            format!(
                "Routing  {}",
                live.map(|v| v.route_mode.as_str())
                    .unwrap_or(s.store.settings.route_mode.as_str())
            ),
            format!(
                "System proxy  {}",
                if s.system_proxy.effective {
                    "Verified"
                } else if s.system_proxy.configured {
                    "Configured; not verified"
                } else {
                    "Off"
                }
            ),
            format!(
                "TUN  {}",
                if s.connected {
                    if s.running_tun {
                        "Running"
                    } else {
                        "Off"
                    }
                } else if native::uses_tun(&s.store) {
                    "Configured in draft"
                } else {
                    "Off"
                }
            ),
            format!("Internet check  {}", s.connectivity.state),
            format!(
                "Traffic  ↑ {} B/s  ↓ {} B/s",
                s.status.uplink, s.status.downlink
            ),
            String::new(),
        ];
        if !s.selection_recovery.is_empty() {
            lines.push(s.selection_recovery.clone());
        }
        if s.store.native.is_none() {
            lines.push("Native configuration upgrade is ready.".into());
            lines.push("u  Review upgrade · original state will be backed up".into());
        } else {
            let count = content.height.saturating_sub(12).max(1) as usize;
            for (index, g) in native::array(&doc, "/outbounds")
                .iter()
                .filter(|v| v["type"] == "selector" || v["type"] == "urltest")
                .enumerate()
                .skip(a.selected[0].saturating_sub(count - 1))
                .take(count)
            {
                let member = if s.connected {
                    s.groups
                        .group
                        .iter()
                        .find(|live| live.tag == native::tag(g))
                        .map(|g| g.selected.as_str())
                        .unwrap_or("Unknown")
                } else {
                    g["default"].as_str().unwrap_or("Automatic / first member")
                };
                lines.push(format!(
                    "{} {} → {}",
                    if index == a.selected[0] { "›" } else { " " },
                    a.label(native::tag(g)),
                    a.label(member)
                ));
            }
            if s.store.nodes.is_empty() {
                lines.push("\nImport Subscription to add your first nodes.".into());
            }
        }
        f.render_widget(
            Paragraph::new(lines.join("\n"))
                .wrap(Wrap { trim: false })
                .block(block("Overview")),
            content,
        );
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
            .block(block("Activity · r core logs")),
            content,
        );
    } else if a.page == 9 {
        f.render_widget(Paragraph::new("r  Inspect native references and saved / running configuration\n\nThis report does not measure network performance.").wrap(Wrap{trim:false}).block(block("Diagnostics")),content);
    } else if a.page == 10 {
        let message = match a.tabs[10] {
            0 => format!("Core  {}\nVersion  {}\n\nInstall a verified core or choose an existing binary.\nInstallation does not start the proxy.", a.snapshot.core, a.snapshot.version),
            1 => "Language  English\nKeyboard  Tab to focus, Enter to open\nLayout  Adaptive list and details\n\nNames from subscriptions keep their original language.".into(),
            _ => format!("Host  {}{}\n\nClosing this interface leaves the running core active.\nStop restores sing-owned proxy settings before stopping.\n\nNetwork configuration is under Network.", a.snapshot.host, if a.snapshot.ssh { " (remote host over SSH)" } else { "" }),
        };
        f.render_widget(
            Paragraph::new(message)
                .wrap(Wrap { trim: false })
                .block(block("Settings")),
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
                .block(block("Capture")),
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
        let mut inner = block(&heading).inner(content);
        f.render_widget(block(&heading), content);
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
                    .map(|(_, label, _)| ListItem::new(model::clean(label)))
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
                        .block(block(if a.page == 7 {
                            "Observation · Enter details"
                        } else {
                            "Details · e form · E native"
                        })),
                    split[1],
                );
            }
        }
    }
    let notice = if a.searching {
        format!("/{}", a.filter.value)
    } else if a.busy {
        "Working…".into()
    } else {
        model::clean(&a.notice)
    };
    f.render_widget(
        Paragraph::new(notice)
            .style(Style::default().fg(if a.error { Color::LightRed } else { MUTED }))
            .wrap(Wrap { trim: false }),
        rows[6],
    );
    f.render_widget(
        Paragraph::new(format!(
            "{}   [Review Changes]",
            if a.snapshot.dirty {
                "Draft changes · not applied"
            } else {
                "No pending configuration changes"
            }
        ))
        .style(if a.focus == Focus::Review {
            Style::default().fg(ACCENT).add_modifier(Modifier::REVERSED)
        } else {
            Style::default().fg(MUTED)
        }),
        rows[7],
    );
    f.render_widget(
        Paragraph::new(" Tab Focus  Enter Open  / Search  ? Help  q Quit")
            .style(Style::default().fg(MUTED)),
        rows[8],
    );
    if let Some(d) = &a.dialog {
        let dialog_area = Rect::new(
            content.x,
            rows[3].y,
            content.width,
            rows[6].y.saturating_sub(rows[3].y),
        );
        draw_dialog(f, d, dialog_area, Some(a));
    }
}
pub(super) fn draw_dialog(f: &mut Frame, d: &Dialog, area: Rect, app: Option<&App>) {
    let label = |s: &str| app.map_or_else(|| s.to_string(), |a| a.label(s));
    f.render_widget(Clear, area);
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
        Dialog::Commands{choices,query,selected}=>draw_choices(f,&format!("Actions · {}",query.value),choices.iter().filter(|(name,_)|name.to_lowercase().contains(&query.value.to_lowercase())).map(|(name,_)|name.to_string()).collect(),*selected,rows[0]),
        Dialog::Text{title,text,scroll,..}=>f.render_widget(Paragraph::new(text.as_str()).wrap(Wrap{trim:false}).scroll((*scroll,0)).block(block(title)),rows[0]),
        Dialog::Form(form)=>{let inner=block(&form.title).inner(rows[0]);f.render_widget(block(&form.title),rows[0]);let visible=(inner.height as usize/2).max(1);let start=form.selected.saturating_sub(visible-1);let mut lines=vec![];for(i,field)in form.fields.iter().enumerate().skip(start).take(visible){lines.push(Line::styled(format!("{} {}",if i==form.selected{"›"}else{" "},field.label),Style::default().fg(if i==form.selected{ACCENT}else{MUTED})));let value=if ["source","url"].contains(&field.key.as_str())&&!field.input.value.is_empty(){"[private value · Ctrl+U to replace]".into()}else{if matches!(field.kind,Kind::Choice(_)){label(&field.input.value)}else if matches!(field.kind,Kind::Members(_)){serde_json::from_str::<Vec<String>>(&field.input.value).unwrap_or_default().iter().map(|s|label(s)).collect::<Vec<_>>().join(", ")}else{model::clean(&field.input.value)}};lines.push(Line::raw(format!("  {}",if value.is_empty(){"(default / omitted)"}else{&value})));}f.render_widget(Paragraph::new(lines),inner);},
        Dialog::Json{edit,input}=>{let b=block(if edit.pointer.is_empty(){"Native document · credentials visible"}else{&edit.pointer});let inner=b.inner(rows[0]);f.render_widget(b,rows[0]);let line=input.value[..input.cursor].bytes().filter(|c|*c==b'\n').count();let col=input.value[..input.cursor].rsplit('\n').next().unwrap_or("").chars().count();let sy=line.saturating_sub(inner.height.saturating_sub(1)as usize);let sx=col.saturating_sub(inner.width.saturating_sub(1)as usize);f.render_widget(Paragraph::new(input.value.as_str()).scroll((sy as u16,sx as u16)),inner);f.set_cursor_position((inner.x+(col-sx)as u16,inner.y+(line-sy)as u16));},
        Dialog::Add{choices,selected}=>draw_choices(f,"Add object",choices.iter().map(|(s,_)|s.clone()).collect(),*selected,rows[0]),
        Dialog::Members{choices,chosen,selected,..}=>draw_choices(f,"Members · Space toggles",choices.iter().map(|s|format!("[{}] {}",if chosen.contains(s){"x"}else{" "},label(s))).collect(),*selected,rows[0]),
        Dialog::Select{group,choices,selected}=>draw_choices(f,&label(group),choices.iter().map(|s|label(s)).collect(),*selected,rows[0]),
        Dialog::Auth{kind,..}=>f.render_widget(Paragraph::new(format!("Administrator authorization: {kind}\n\nEnter opens sudo. sing never receives your password.\nEsc cancels without enabling takeover.")).wrap(Wrap{trim:false}).block(block("Authorization")),rows[0]),
    }
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
