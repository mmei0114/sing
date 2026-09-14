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
fn details(a: &App, v: &Value) -> String {
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
            lines.push("\nMembers".into());
            let selected = a
                .snapshot
                .groups
                .group
                .iter()
                .find(|g| g.tag == native::tag(v))
                .map(|g| g.selected.as_str())
                .or_else(|| v["default"].as_str())
                .unwrap_or("");
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
    "1–9 / 0 / -: open the numbered top page\nTab: focus top navigation / content; ←→: choose page\n↑↓ or j/k: list movement   /: filter   [ / ]: subpage\na: add   e: form   E: native JSON   x: remove\nEnter: details / select group member\nJ / K: move rule down / up\nF2 / Ctrl+S: save draft   Esc: cancel / close\nA: Review & Apply   V: core check   p: redacted preview\nc: Start (when stopped)   d: Stop   q: close interface\n\nOverview: a imports a node subscription.\nInbounds: s system integration, separate from listeners.\nOutbounds: g new group, a other outbound, Enter selects a member, t latency.\nResources / Subscriptions: a import, r refresh, x remove.\nResources / Rule sets: C import QX/Clash list and choose its routing target; a adds a native rule set. Check conversion warnings before confirming.\nDNS / Routing: Options subpage sets defaults.\nConnections: r refresh, h recent closed, x close one.\nSettings: e core path, i install. Advanced: additional sections, E entire document, b rollback.\n\nJSON fields use native syntax. Blank optional fields omit the override. Form edits preserve fields not changed by the form. E exposes credentials: do not share its screen.\n\nGlobal / Direct override traffic routing. DNS is unchanged. TUN can change routes and interface DNS on this host, including over SSH. Management API address/credentials are reserved for sing.\n\nSubscription refresh reports conflicts with locally modified nodes. Rename a node tag to detach it before replacing its subscription version. Review listener exposure and network paths before applying imported configuration.".into()
}
fn navigation(a: &App, width: u16) -> Vec<Line<'static>> {
    let mut lines = vec![];
    let mut spans = vec![];
    let mut used = 0;
    for (i, page) in PAGES.iter().enumerate() {
        let label = format!(" {} {page} ", PAGE_KEYS[i]);
        let size = label.len() as u16;
        if used + size > width && !spans.is_empty() {
            lines.push(Line::from(std::mem::take(&mut spans)));
            used = 0;
        }
        let style = if i == a.page {
            Style::default()
                .bg(ACCENT)
                .fg(Color::Rgb(17, 23, 30))
                .add_modifier(Modifier::BOLD)
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
fn page_actions(a: &App) -> &'static str {
    if a.nav {
        return "←→ Choose page · Enter Open · Tab Back to content";
    }
    match (a.page, a.tabs[a.page]) {
        (0, _) if a.snapshot.store.native.is_none() => "u Upgrade config · a Import subscription · i Install core",
        (0, _) => "a Import subscription · Enter Select node · M Routing mode",
        (1, _) => "a Add inbound · e Edit · E JSON · s System proxy · x Remove",
        (2, _) => "g New group · a Add outbound · Enter Select node\ne Edit · E JSON · t Test latency · / Filter",
        (3, 0) => "a Add rule · e Edit · J/K Reorder · x Remove · [] Options",
        (3, _) => "e Edit routing defaults · E JSON · [] Rules",
        (4, 0) => "a Add resolver · e Edit · E JSON · x Remove · [] Rules/Options",
        (4, 1) => "a Add DNS rule · e Edit · J/K Reorder · [] Resolvers/Options",
        (4, _) => "e Edit DNS defaults · E JSON · [] Resolvers/Rules",
        (5, 0) => "a Import subscription · r Refresh · x Remove\n[] Rule sets",
        (5, _) => "C Import QX/Clash rules · a Add native rule set\ne Edit · r Refresh converted rules · [] Subscriptions",
        (6, _) => "e Edit section · E Full JSON · b Rollback",
        (7, _) => "r Refresh · Enter Details · h Include closed · x Close one",
        (8, _) => "r Read core logs",
        (9, _) => "r Inspect configuration · V Core check · p Preview",
        (10, _) => "e Core settings · i Install core",
        _ => "",
    }
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
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(tabs.len() as u16),
            Constraint::Min(5),
            Constraint::Length(2),
            Constraint::Length(2),
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
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " sing  ",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                "{}{} · {state}{}{}",
                a.snapshot.host,
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
    f.render_widget(Paragraph::new(tabs), rows[1]);
    let content = rows[2];
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
                lines.push("\na  Import a node subscription to begin".into());
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
        f.render_widget(Paragraph::new(format!("Core  {}\n\ne  Edit core path\ni  Install verified core\n\nSystem integration is under Inbounds → s.\nInterface language: English",a.snapshot.core)).wrap(Wrap{trim:false}).block(block("Settings")),content);
    } else {
        let tabnames = a.tab_names();
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
        } else if tabnames.is_empty() {
            PAGES[a.page].into()
        } else {
            format!(
                "{} · {}",
                PAGES[a.page],
                tabnames
                    .iter()
                    .enumerate()
                    .map(|(i, n)| if i == a.tabs[a.page] {
                        format!("[{n}]")
                    } else {
                        n.to_string()
                    })
                    .collect::<Vec<_>>()
                    .join("  ")
            )
        };
        let inner = block(&heading).inner(content);
        f.render_widget(block(&heading), content);
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
            .highlight_style(Style::default().bg(Color::Rgb(33, 53, 60)).fg(ACCENT))
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
                    "No subscriptions. a imports a subscription link."
                } else if a.page == 5 {
                    "No rule sets. C imports a QX/Clash rule list."
                } else {
                    "No items. a adds an object; ? shows help."
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
        Paragraph::new(page_actions(a))
            .style(Style::default().fg(ACCENT))
            .wrap(Wrap { trim: false }),
        rows[3],
    );
    f.render_widget(
        Paragraph::new(notice)
            .style(Style::default().fg(if a.error { Color::LightRed } else { MUTED }))
            .wrap(Wrap { trim: false }),
        rows[4],
    );
    f.render_widget(
        Paragraph::new(" Tab Pages  ? Help  A Apply  d Stop  q Quit")
            .style(Style::default().fg(MUTED)),
        rows[5],
    );
    if let Some(d) = &a.dialog {
        let dialog_area = Rect::new(
            content.x,
            content.y,
            content.width,
            content.height + rows[3].height,
        );
        draw_dialog(f, d, dialog_area, Some(a));
    }
}
pub(super) fn draw_dialog(f: &mut Frame, d: &Dialog, area: Rect, app: Option<&App>) {
    let label = |s: &str| app.map_or_else(|| s.to_string(), |a| a.label(s));
    f.render_widget(Clear, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(2)])
        .split(area);
    let hint = match d {
        Dialog::Form(form) => match form.action {
            FormAction::Import | FormAction::Convert => " F2 Review import  Esc Cancel\n Tab/↑↓ Field  ←→ Choice  Ctrl+U Clear",
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
    match d{
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
