//! Live evidence: connections, the applications observed behind them, and logs.
use super::{flows, history, identity, modal, quick_rule, text, theme, App};
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
    pub paused: bool,
    pub query: String,
    pub search: Option<super::input::Input>,
    pub section: Section,
    pub selected: [usize; 3],
    pub sort: Sort,
    pub app_filter: Option<String>,
}

pub fn capturing(app: &App) -> bool {
    app.activity.search.is_some()
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
                .is_none_or(|name| identity::key(&e.c) == *name)
        })
        .filter(|e| {
            format!(
                "{} {} {} {} {}",
                e.app(),
                identity::key(&e.c),
                e.host(),
                history::route_path(&e.c, |t| app.label(t)),
                e.c.rule
            )
            .to_lowercase()
            .contains(&app.activity.query.to_lowercase())
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
    rows.retain(|a| {
        format!("{} {}", a.name, a.path)
            .to_lowercase()
            .contains(&app.activity.query.to_lowercase())
    });
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
        .filter(|e| identity::key(&e.c) == row.key)
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
    if let Some(input) = &mut app.activity.search {
        match k.code {
            K::Esc => app.activity.search = None,
            K::Enter => {
                app.activity.query = input.value.trim().to_string();
                app.activity.search = None;
                app.activity.selected = [0; 3];
            }
            _ => input.key(k, false),
        }
        return;
    }
    if app.activity.section != Section::Logs
        && matches!(
            k.code,
            K::Down | K::Up | K::PageDown | K::PageUp | K::Enter | K::Char('j' | 'k' | 'r' | 'x')
        )
    {
        app.activity.paused = true;
    }
    let index = app.activity.section.index();
    let n = row_count(app);
    match k.code {
        K::Char('/') if app.activity.section != Section::Logs => {
            app.activity.search = Some(super::input::Input::new(app.activity.query.clone()));
            app.activity.paused = true;
        }
        K::Esc if !app.activity.query.is_empty() => {
            app.activity.query.clear();
            app.activity.selected = [0; 3];
        }
        K::Char(' ') if app.activity.section != Section::Logs => {
            if app.activity.paused {
                app.resume_activity();
            } else {
                app.activity.paused = true;
            }
        }
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
                app.activity.app_filter = Some(row.key.clone());
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
        K::Char('f') if app.activity.section != Section::Logs => {
            quick_rule::enable_process_discovery(app)
        }
        K::Char('c') => {
            app.resume_activity();
            app.history.clear();
            app.activity.selected = [0; 3];
            app.toast("Activity history cleared");
        }
        K::Char('x') if app.activity.section == Section::Requests => {
            if let Some(c) = connections(app)
                .get(app.activity.selected[0])
                .filter(|e| e.open)
                .map(|e| e.c.clone())
            {
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

pub fn paste(app: &mut App, s: &str) {
    if let Some(input) = &mut app.activity.search {
        input.insert(s);
    }
}

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
    spans.push(Span::styled("[/]", theme::key()));
    let suffix = match &app.activity.app_filter {
        Some(name) => format!(
            "App: {} · Esc clears",
            app.history
                .apps()
                .iter()
                .find(|a| a.key == *name)
                .map(|a| a.name.as_str())
                .unwrap_or("Unattributed")
        ),
        None if app.activity.section != Section::Logs => {
            format!(
                "{} · {}",
                if app.activity.paused {
                    "Paused · Space live"
                } else {
                    "Live"
                },
                app.activity.sort.label()
            )
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
    let area = area.inner(ratatui::layout::Margin {
        horizontal: 2,
        vertical: 1,
    });
    let [nav, body] = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(area);
    f.render_widget(Paragraph::new(tabs(app, nav.width)), nav);
    if let Some(input) = &app.activity.search {
        let mut spans = vec![Span::styled("/ ", theme::key())];
        spans.extend(
            input
                .line(nav.width.saturating_sub(2) as usize, true, false)
                .spans,
        );
        f.render_widget(
            Paragraph::new(Line::from(spans)),
            Rect {
                y: nav.y + 1,
                height: 1,
                ..nav
            },
        );
    } else if !app.activity.query.is_empty() {
        f.render_widget(
            Paragraph::new(format!("/ {} · Esc clears", app.activity.query))
                .style(theme::s(theme::dim())),
            Rect {
                y: nav.y + 1,
                height: 1,
                ..nav
            },
        );
    }
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
        } else if !app.activity.query.is_empty() {
            "No matching connections. Esc clears the filter."
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
    let app_w = (w / 5).clamp(8, 18);
    let route_w = (w / 5).clamp(8, 16);
    let host_w = w.saturating_sub(app_w + route_w + 11);
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
            "Unattributed"
        } else {
            &process
        };
        lines.push(modal::row(
            i == selected,
            vec![
                Span::styled(text::cell(process, app_w), theme::s(theme::text())),
                Span::styled(
                    format!("{} ", text::cell(&e.host(), host_w.saturating_sub(1))),
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
            "{} observed · {} · sample {}",
            rows.len(),
            if app.activity.paused {
                "Paused · Space live".into()
            } else {
                format!("{} live", app.history.total_live)
            },
            if app.history.observed_at == 0 {
                "not available".into()
            } else {
                text::age(age)
            }
        ),
        theme::s(theme::faint()),
    ));
    f.render_widget(Paragraph::new(lines), list);
    if let Some(detail) = detail {
        let c = &rows[selected].c;
        let name = identity::display(c);
        let process = if name.is_empty() {
            identity::missing_label(app)
        } else {
            &name
        };
        let body = format!(
            "ROUTING EVIDENCE\n\n{}\n{}\n\nApp\n{}\n\nRule\n{}\n\nChain\n{}\n\nPress r to create an editable rule from this evidence.",
            if c.domain.is_empty() { &c.destination } else { &c.domain },
            c.destination,
            process,
            if c.rule.is_empty() { "No matched rule reported" } else { &c.rule },
            history::route_path(c, |t| app.label(t))
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
        let body = if !app.activity.query.is_empty() {
            "\n  No matching applications. Esc clears the filter.".into()
        } else {
            format!(
                "\n  No applications observed yet.\n\n  {}",
                identity::explanation(app)
            )
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
    let name_w = (w / 3).clamp(10, 26);
    let path_w = w.saturating_sub(name_w + 18);
    let mut lines = vec![Line::from(vec![
        Span::styled(text::cell("APPLICATION", name_w), theme::s(theme::faint())),
        Span::styled(text::cell("EXECUTABLE", path_w), theme::s(theme::faint())),
        Span::styled(text::right("OPEN", 6), theme::s(theme::dim())),
        Span::styled(text::right("TRAFFIC", 11), theme::s(theme::dim())),
    ])];
    for (i, a) in rows.iter().enumerate().skip(start).take(height) {
        lines.push(modal::row(
            i == selected,
            vec![
                Span::styled(
                    text::cell(
                        if a.name.is_empty() {
                            "Unattributed"
                        } else {
                            &a.name
                        },
                        name_w,
                    ),
                    theme::s(theme::text()),
                ),
                Span::styled(
                    text::cell(a.path.rsplit('/').next().unwrap_or("—"), path_w),
                    theme::s(theme::dim()),
                ),
                Span::styled(text::right(&a.open.to_string(), 6), theme::s(theme::good())),
                Span::styled(
                    text::right(&text::bytes(a.up + a.down), 11),
                    theme::s(theme::text()),
                ),
            ],
        ));
    }
    lines.push(Line::styled(
        if app.activity.paused {
            "Paused · Space resumes live updates"
        } else if rows.iter().any(|a| a.key.is_empty()) {
            "Unattributed traffic · f explains app discovery"
        } else {
            "Observed socket owners · not a list of installed apps"
        },
        theme::s(theme::faint()),
    ));
    f.render_widget(Paragraph::new(lines), list);
    if let Some(detail) = detail {
        let current = &rows[selected];
        let hosts = app.history.hosts(Some(&current.key));
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
    if app.activity.search.is_some() {
        return vec![("type", "filter"), ("enter", "done"), ("esc", "cancel")];
    }
    match app.activity.section {
        Section::Requests => vec![
            ("↑↓", "move"),
            ("enter", "details"),
            ("r", "rule"),
            ("/", "filter"),
            ("x", "close"),
            ("o", "sort"),
            ("f", "apps"),
        ],
        Section::Apps => vec![
            ("↑↓", "app"),
            ("enter", "links"),
            ("r", "rule"),
            ("/", "filter"),
            ("o", "sort"),
            ("f", "find apps"),
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
