//! Live evidence: connections, the applications observed behind them, and logs.
use super::{flows, history, modal, quick_rule, text, theme, App};
use crossterm::event::{KeyCode as K, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    #[default]
    Requests,
    Apps,
    Logs,
}
impl Section {
    const ALL: [Self; 3] = [Self::Requests, Self::Apps, Self::Logs];
    fn name(self) -> &'static str {
        match self {
            Self::Requests => "Connections",
            Self::Apps => "Apps",
            Self::Logs => "Logs",
        }
    }
    fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sort {
    #[default]
    Recent,
    Traffic,
}
impl Sort {
    fn label(self) -> &'static str {
        match self {
            Self::Recent => "recent",
            Self::Traffic => "traffic",
        }
    }
}

#[derive(Default)]
pub struct State {
    pub section: Section,
    pub selected: [usize; 3],
    pub sort: Sort,
    pub app_filter: Option<String>,
}

pub fn capturing(_: &App) -> bool {
    false
}

fn connections(app: &App) -> Vec<&history::Entry> {
    let mut rows: Vec<_> = app
        .history
        .entries
        .iter()
        .filter(|e| {
            app.activity
                .app_filter
                .as_ref()
                .is_none_or(|name| e.app() == *name)
        })
        .collect();
    match app.activity.sort {
        Sort::Recent => rows.sort_by_key(|e| std::cmp::Reverse((e.open, e.created()))),
        Sort::Traffic => rows.sort_by_key(|e| std::cmp::Reverse(e.total())),
    }
    rows
}

fn apps(app: &App) -> Vec<history::AppStat> {
    let mut rows = app.history.apps();
    match app.activity.sort {
        Sort::Recent => rows.sort_by_key(|a| std::cmp::Reverse(a.last)),
        Sort::Traffic => rows.sort_by_key(|a| std::cmp::Reverse(a.up + a.down)),
    }
    rows
}

fn selected_connection(app: &App) -> Option<crate::api::Connection> {
    connections(app)
        .get(app.activity.selected[Section::Requests.index()])
        .map(|e| e.c.clone())
}

fn selected_app_sample(app: &App) -> Option<crate::api::Connection> {
    let row = apps(app)
        .get(app.activity.selected[Section::Apps.index()])?
        .clone();
    app.history
        .entries
        .iter()
        .filter(|e| e.app() == row.name)
        .max_by_key(|e| e.created())
        .map(|e| e.c.clone())
}

fn row_count(app: &App) -> usize {
    match app.activity.section {
        Section::Requests => connections(app).len(),
        Section::Apps => apps(app).len(),
        Section::Logs => app.logs.len(),
    }
}

pub fn key(app: &mut App, k: KeyEvent) {
    let index = app.activity.section.index();
    let n = row_count(app);
    match k.code {
        K::Char(']') | K::Right => {
            app.activity.section = Section::ALL[(index + 1) % Section::ALL.len()];
        }
        K::Char('[') | K::Left => {
            app.activity.section =
                Section::ALL[(index + Section::ALL.len() - 1) % Section::ALL.len()];
        }
        K::Down | K::Char('j') => {
            app.activity.selected[index] =
                (app.activity.selected[index] + 1).min(n.saturating_sub(1));
        }
        K::Up | K::Char('k') => {
            app.activity.selected[index] = app.activity.selected[index].saturating_sub(1);
        }
        K::PageDown => {
            app.activity.selected[index] =
                (app.activity.selected[index] + 10).min(n.saturating_sub(1));
        }
        K::PageUp => app.activity.selected[index] = app.activity.selected[index].saturating_sub(10),
        K::Char('o') if app.activity.section != Section::Logs => {
            app.activity.sort = if app.activity.sort == Sort::Recent {
                Sort::Traffic
            } else {
                Sort::Recent
            };
            app.activity.selected[index] = 0;
        }
        K::Enter if app.activity.section == Section::Requests => {
            if let Some(c) = selected_connection(app) {
                app.push(flows::connection_details(app, &c));
            }
        }
        K::Enter if app.activity.section == Section::Apps => {
            if let Some(row) = apps(app).get(app.activity.selected[index]) {
                app.activity.app_filter = Some(row.name.clone());
                app.activity.section = Section::Requests;
                app.activity.selected[Section::Requests.index()] = 0;
            }
        }
        K::Esc
            if app.activity.section == Section::Requests && app.activity.app_filter.is_some() =>
        {
            app.activity.app_filter = None;
        }
        K::Char('r') if app.activity.section == Section::Requests => {
            if let Some(c) = selected_connection(app) {
                quick_rule::from_connection(app, c, false);
            }
        }
        K::Char('r') if app.activity.section == Section::Apps => {
            if let Some(c) = selected_app_sample(app) {
                quick_rule::from_connection(app, c, true);
            }
        }
        K::Char('f') if app.activity.section == Section::Apps => {
            quick_rule::enable_process_discovery(app)
        }
        K::Char('c') => {
            app.history.clear();
            app.activity.selected = [0; 3];
            app.toast("Activity history cleared");
        }
        K::Char('x') if app.activity.section == Section::Requests => {
            if let Some(c) = selected_connection(app).filter(|c| c.closed_at == 0) {
                let id = c.id.clone();
                app.push(super::modal::Confirm::new(
                    "Close connection?",
                    format!(
                        "Close the selected connection to {}? The application may reconnect.",
                        history::host(&c)
                    ),
                    "Close",
                    Box::new(move |app| app.send(crate::runtime::Action::CloseConnection(id))),
                ));
            }
        }
        _ => {}
    }
}

pub fn paste(_: &mut App, _: &str) {}

fn tabs(app: &App, width: u16) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for s in Section::ALL {
        let on = s == app.activity.section;
        spans.push(Span::styled(
            format!(" {} ", s.name()),
            if on {
                theme::bold(theme::accent())
            } else {
                theme::s(theme::dim())
            },
        ));
        spans.push(Span::raw("  "));
    }
    let suffix = match &app.activity.app_filter {
        Some(name) => format!("App: {name} · Esc clears"),
        None if app.activity.section != Section::Logs => {
            format!("Sort: {}", app.activity.sort.label())
        }
        None => String::new(),
    };
    let used: usize = spans.iter().map(|s| text::width(&s.content)).sum();
    if used + text::width(&suffix) + 1 < width as usize {
        spans.push(Span::styled(
            format!(
                "{}{}",
                " ".repeat(width as usize - used - text::width(&suffix) - 1),
                suffix
            ),
            theme::s(theme::faint()),
        ));
    }
    Line::from(spans)
}

pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let [nav, body] = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(area);
    f.render_widget(Paragraph::new(tabs(app, nav.width)), nav);
    match app.activity.section {
        Section::Requests => draw_connections(f, body, app),
        Section::Apps => draw_apps(f, body, app),
        Section::Logs => draw_logs(f, body, app),
    }
}

fn draw_connections(f: &mut Frame, area: Rect, app: &App) {
    let rows = connections(app);
    if rows.is_empty() {
        let message = if !app.snap.connected {
            "Start the core to observe connections."
        } else if app.activity.app_filter.is_some() {
            "No observed connections for this app. Esc clears the filter."
        } else {
            "Waiting for connection evidence…"
        };
        f.render_widget(Paragraph::new(format!("\n  {message}\n\n  Activity keeps a session history of what the core reports; it does not infer routes from the draft.")), area);
        return;
    }
    let wide = area.width >= 105;
    let (list, detail) = if wide {
        let [a, _, b] = Layout::horizontal([
            Constraint::Percentage(64),
            Constraint::Length(2),
            Constraint::Percentage(36),
        ])
        .areas(area);
        (a, Some(b))
    } else {
        (area, None)
    };
    let selected = app.activity.selected[Section::Requests.index()].min(rows.len() - 1);
    let height = list.height.saturating_sub(2) as usize;
    let start = modal::scroll(selected, height, rows.len());
    let w = list.width as usize;
    let app_w = if w > 80 { 18 } else { 12 };
    let route_w = if w > 80 { 16 } else { 10 };
    let host_w = w.saturating_sub(app_w + route_w + 22).max(10);
    let mut lines = vec![Line::from(vec![
        Span::styled(text::cell("APP", app_w), theme::s(theme::faint())),
        Span::styled(text::cell("DESTINATION", host_w), theme::s(theme::faint())),
        Span::styled(text::cell("ROUTE", route_w), theme::s(theme::faint())),
        Span::styled(text::right("TRAFFIC", 10), theme::s(theme::faint())),
    ])];
    for (i, e) in rows.iter().enumerate().skip(start).take(height) {
        let target = history::route_target(&e.c);
        let process = e.app();
        let process = if process.is_empty() {
            "Unknown app"
        } else {
            &process
        };
        lines.push(modal::row(
            i == selected,
            vec![
                Span::styled(text::cell(process, app_w), theme::s(theme::text())),
                Span::styled(
                    text::cell(&e.host(), host_w),
                    theme::s(if e.open { theme::text() } else { theme::dim() }),
                ),
                Span::styled(
                    text::cell(&app.label(&target), route_w),
                    theme::s(theme::target(&target)),
                ),
                Span::styled(
                    text::right(&text::bytes(e.total()), 10),
                    theme::s(theme::dim()),
                ),
            ],
        ));
    }
    let age = crate::model::now().saturating_sub(app.history.observed_at);
    lines.push(Line::styled(
        format!(
            "{} observed · {} live · sample {}",
            rows.len(),
            app.history.total_live,
            if app.history.observed_at == 0 {
                "not available".into()
            } else {
                format!("{} ago", text::ago(age))
            }
        ),
        theme::s(theme::faint()),
    ));
    f.render_widget(Paragraph::new(lines), list);
    if let Some(detail) = detail {
        let c = &rows[selected].c;
        let process = c
            .process
            .as_ref()
            .map(|p| p.name())
            .filter(|s| !s.is_empty())
            .unwrap_or("Unavailable");
        let body = format!(
            "ROUTING EVIDENCE\n\n{}\n{}\n\nApp\n{}\n\nRule\n{}\n\nChain\n{}\n\nPress r to create an editable rule from this evidence.",
            if c.domain.is_empty() { &c.destination } else { &c.domain },
            c.destination,
            process,
            if c.rule.is_empty() { "Unavailable" } else { &c.rule },
            if c.chain.is_empty() { app.label(&c.outbound) } else { c.chain.iter().map(|t| app.label(t)).collect::<Vec<_>>().join(" → ") }
        );
        f.render_widget(
            Paragraph::new(body)
                .style(theme::s(theme::dim()))
                .wrap(Wrap { trim: false }),
            detail,
        );
    }
}

fn draw_apps(f: &mut Frame, area: Rect, app: &App) {
    let rows = apps(app);
    if rows.is_empty() {
        let enabled =
            app.doc().pointer("/route/find_process") == Some(&serde_json::Value::Bool(true));
        let body = if enabled {
            "\n  No application identity has been reported yet.\n\n  App rows appear only after sing-box associates a captured connection with a process."
        } else {
            "\n  No application identity has been reported.\n\n  f enables route.find_process in the draft, then A applies it. Process lookup is supported on macOS, Linux and Windows."
        };
        f.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), area);
        return;
    }
    let selected = app.activity.selected[Section::Apps.index()].min(rows.len() - 1);
    let (list, detail) = if area.width >= 105 {
        let [a, _, b] = Layout::horizontal([
            Constraint::Percentage(62),
            Constraint::Length(2),
            Constraint::Percentage(38),
        ])
        .areas(area);
        (a, Some(b))
    } else {
        (area, None)
    };
    let height = list.height.saturating_sub(2) as usize;
    let start = modal::scroll(selected, height, rows.len());
    let w = list.width as usize;
    let name_w = (w / 3).clamp(14, 34);
    let path_w = w.saturating_sub(name_w + 42).max(12);
    let mut lines = vec![Line::from(vec![
        Span::styled(text::cell("APPLICATION", name_w), theme::s(theme::faint())),
        Span::styled(text::cell("EXECUTABLE", path_w), theme::s(theme::faint())),
        Span::styled(text::right("OPEN", 7), theme::s(theme::faint())),
        Span::styled(text::right("REQUESTS", 10), theme::s(theme::faint())),
        Span::styled(text::right("TRAFFIC", 12), theme::s(theme::faint())),
    ])];
    for (i, a) in rows.iter().enumerate().skip(start).take(height) {
        lines.push(modal::row(
            i == selected,
            vec![
                Span::styled(
                    text::cell(
                        if a.name.is_empty() {
                            "Unknown app"
                        } else {
                            &a.name
                        },
                        name_w,
                    ),
                    theme::s(theme::text()),
                ),
                Span::styled(text::cell(&a.path, path_w), theme::s(theme::dim())),
                Span::styled(text::right(&a.open.to_string(), 7), theme::s(theme::good())),
                Span::styled(
                    text::right(&a.connections.to_string(), 10),
                    theme::s(theme::dim()),
                ),
                Span::styled(
                    text::right(&text::bytes(a.up + a.down), 12),
                    theme::s(theme::text()),
                ),
            ],
        ));
    }
    lines.push(Line::styled(
        "Enter shows this app's links · r creates an app rule · o changes sort",
        theme::s(theme::faint()),
    ));
    f.render_widget(Paragraph::new(lines), list);
    if let Some(detail) = detail {
        let current = &rows[selected];
        let hosts = app.history.hosts(Some(&current.name));
        let mut lines = vec![
            Line::styled("OBSERVED LINKS", theme::bold(theme::dim())),
            Line::styled(
                "Domains and IPs reported for this app",
                theme::s(theme::faint()),
            ),
            Line::raw(""),
        ];
        for host in hosts.iter().take(detail.height.saturating_sub(4) as usize) {
            lines.push(Line::from(vec![
                Span::styled(
                    text::cell(&host.host, (detail.width as usize).saturating_sub(18)),
                    theme::s(theme::text()),
                ),
                Span::styled(
                    text::right(&text::bytes(host.traffic), 10),
                    theme::s(theme::dim()),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  → ", theme::s(theme::faint())),
                Span::styled(
                    app.label(&host.target),
                    theme::s(theme::target(&host.target)),
                ),
                Span::styled(
                    format!(" · {} requests", host.connections),
                    theme::s(theme::faint()),
                ),
            ]));
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), detail);
    }
}

fn draw_logs(f: &mut Frame, area: Rect, app: &App) {
    let selected =
        app.activity.selected[Section::Logs.index()].min(app.logs.len().saturating_sub(1));
    let start = app
        .logs
        .len()
        .saturating_sub(area.height as usize)
        .max(selected.saturating_sub(area.height as usize / 2));
    let lines: Vec<Line> = app
        .logs
        .iter()
        .enumerate()
        .skip(start)
        .take(area.height as usize)
        .map(|(i, l)| {
            let color = if l.contains("ERROR") || l.contains("FATAL") {
                theme::bad()
            } else if l.contains("WARN") {
                theme::warn()
            } else {
                theme::dim()
            };
            modal::row(
                i == selected,
                vec![Span::styled(
                    text::fit(l, area.width.saturating_sub(2) as usize),
                    theme::s(color),
                )],
            )
        })
        .collect();
    if lines.is_empty() {
        f.render_widget(Paragraph::new("\n  No core logs available."), area);
    } else {
        f.render_widget(Paragraph::new(lines), area);
    }
}

pub fn hints(app: &App) -> Vec<(&'static str, &'static str)> {
    match app.activity.section {
        Section::Requests => vec![
            ("[/]", "section"),
            ("↑↓", "connection"),
            ("enter", "evidence"),
            ("r", "create rule"),
            ("x", "close"),
            ("o", "sort"),
            ("c", "clear"),
        ],
        Section::Apps => vec![
            ("[/]", "section"),
            ("↑↓", "app"),
            ("enter", "links"),
            ("r", "route app"),
            ("o", "sort"),
            ("f", "find apps"),
            ("c", "clear"),
        ],
        Section::Logs => vec![("[/]", "section"), ("↑↓", "scroll"), ("c", "clear history")],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sections_are_the_three_user_tasks() {
        assert_eq!(
            Section::ALL.map(Section::name),
            ["Connections", "Apps", "Logs"]
        );
    }
}
