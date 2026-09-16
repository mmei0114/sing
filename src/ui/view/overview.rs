//! Overview is a compact control surface, not another configuration editor.
use super::*;
use ratatui::widgets::{Cell, Row, Table, TableState};

pub(super) fn draw(f: &mut Frame, a: &App, area: Rect) {
    if area.width < 108 && area.height < 12 && a.focus == Focus::Connections {
        connections(f, a, area, false);
        return;
    }
    if area.width >= 108 {
        let columns = Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(area);
        let left = Layout::vertical([
            Constraint::Length(summary(a).len() as u16 + 1),
            Constraint::Min(2),
        ])
        .split(columns[0]);
        f.render_widget(
            Paragraph::new(summary(a)).block(section("Overview")),
            left[0],
        );
        groups(f, a, left[1]);
        connections(f, a, columns[1], false);
    } else {
        let summary_height = (summary(a).len() as u16 + 1).min(area.height.saturating_sub(5));
        let remaining = area.height.saturating_sub(summary_height);
        let group_height = (remaining / 2).clamp(2, 7);
        let parts = Layout::vertical([
            Constraint::Length(summary_height),
            Constraint::Length(group_height),
            Constraint::Min(2),
        ])
        .split(area);
        f.render_widget(
            Paragraph::new(summary(a)).block(section("Overview")),
            parts[0],
        );
        groups(f, a, parts[1]);
        connections(f, a, parts[2], false);
    }
}
fn summary(a: &App) -> Vec<Line<'static>> {
    let s = &a.snapshot;
    let mut lines = vec![];
    if a.onboarding() {
        lines.push(Line::styled(
            if s.version.is_empty() {
                "Get started · install a core, then set up a connection"
            } else {
                "Get started · import nodes or complete Connection Setup"
            },
            Style::default().fg(ACCENT),
        ));
    }
    lines.push(fact(
        "System proxy",
        if s.system_proxy.effective {
            "Verified"
        } else if s.system_proxy.configured {
            "Configured · not verified"
        } else {
            "Off · apps may use proxy ports"
        },
    ));
    lines.push(fact(
        "TUN",
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
        },
    ));
    let fresh = model::now().saturating_sub(a.snapshot_at) <= 6;
    lines.push(fact(
        "Traffic",
        if !s.connected {
            "— · core stopped".into()
        } else if !fresh {
            "Unavailable · stale sample".into()
        } else if !s.api_ready || !s.status.traffic_available {
            "Unavailable".into()
        } else {
            format!(
                "↑ {}   ↓ {}",
                rate(s.status.uplink),
                rate(s.status.downlink)
            )
        },
    ));
    lines.push(fact(
        "Last check",
        if s.connectivity.checked_at == 0 {
            "Not checked · v Test Connection".into()
        } else {
            format!(
                "{} · {}s ago",
                if a.probe_stale {
                    "Previous context"
                } else if s.connectivity.state == "passed" {
                    "Passed"
                } else {
                    "Failed"
                },
                model::now().saturating_sub(s.connectivity.checked_at)
            )
        },
    ));
    if s.connectivity.checked_at != 0 {
        lines.push(fact("Target", "www.gstatic.com (HTTPS probe)"));
    }
    if s.system_proxy.pending_restore {
        lines.push(Line::styled(
            "Recovery required · R Restore · : Diagnostics",
            Style::default().fg(Color::LightRed),
        ));
    } else if s.connected && !s.api_ready {
        lines.push(Line::styled(
            "Core API unavailable · : Diagnostics",
            Style::default().fg(Color::Yellow),
        ));
    }
    if !s.selection_recovery.is_empty() {
        lines.push(Line::styled(
            model::clean(&s.selection_recovery),
            Style::default().fg(Color::Yellow),
        ));
    }
    lines
}
fn groups(f: &mut Frame, a: &App, area: Rect) {
    let heading = if a.snapshot.connected {
        "Proxy Groups · Enter Select"
    } else {
        "Proxy Groups · Defaults"
    };
    let heading = if a.filter.value.is_empty() {
        heading.into()
    } else {
        format!("{heading} · /{}", model::clean(&a.filter.value))
    };
    let panel = section(&heading);
    let inner = panel.inner(area);
    f.render_widget(panel, area);
    let rows = a.rows();
    if rows.is_empty() {
        f.render_widget(
            Paragraph::new(if a.filter.value.is_empty() {
                "No groups · 2 Proxies · g New Group"
            } else {
                "No matching groups · Esc Clear filter"
            })
            .style(Style::default().fg(MUTED))
            .wrap(Wrap { trim: false }),
            inner,
        );
    } else {
        let items = rows.iter().map(|(_, _, g)| {
            ListItem::new(model::clean(&format!(
                "{} → {}",
                a.label(native::tag(g)),
                group_member(a, g)
            )))
        });
        let mut state = ListState::default().with_selected(Some(a.selected[0]));
        f.render_stateful_widget(
            List::new(items)
                .highlight_symbol("› ")
                .highlight_style(Style::default().fg(if a.focus == Focus::Content {
                    ACCENT
                } else {
                    MUTED
                })),
            inner,
            &mut state,
        );
    }
}
pub(super) fn connections(f: &mut Frame, a: &App, area: Rect, full: bool) {
    let title = if full {
        format!("Connections · {}", a.connection_status())
    } else {
        "Connections · l Focus · 5 Activity".into()
    };
    let panel = section(&title);
    let inner = panel.inner(area);
    f.render_widget(panel, area);
    if inner.height == 0 {
        return;
    }
    let status_height = u16::from(!full);
    if !full {
        f.render_widget(
            Paragraph::new(a.connection_status()).style(Style::default().fg(MUTED)),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
    }
    let body = Rect::new(
        inner.x,
        inner.y + status_height,
        inner.width,
        inner.height.saturating_sub(status_height),
    );
    if body.height == 0 {
        return;
    }
    let connections = a.connection_rows();
    if connections.is_empty() {
        let message = if !a.snapshot.connected {
            "Start the core to observe connections."
        } else if !a.connection_error.is_empty() {
            &a.connection_error
        } else {
            "No reported connections in this sample."
        };
        f.render_widget(
            Paragraph::new(message)
                .style(Style::default().fg(MUTED))
                .wrap(Wrap { trim: false }),
            body,
        );
        return;
    }
    let rows = connections.iter().map(|c| {
        Row::new(vec![
            Cell::from(observations::destination(c)),
            Cell::from(observations::path(a, c)),
        ])
    });
    let focused = a.focus == Focus::Connections || full && a.focus == Focus::Content;
    let mut table = Table::new(
        rows,
        [Constraint::Percentage(45), Constraint::Percentage(55)],
    )
    .column_spacing(2)
    .row_highlight_style(
        Style::default()
            .bg(if focused { SURFACE } else { BACKGROUND })
            .fg(if focused {
                ACCENT
            } else {
                Color::Rgb(218, 226, 233)
            }),
    )
    .highlight_symbol(if focused { "› " } else { "  " });
    if body.height >= 4 {
        table = table.header(
            Row::new(["Destination", "Core-reported path"]).style(Style::default().fg(MUTED)),
        );
    }
    let mut state = TableState::default().with_selected(Some(a.selected[7]));
    f.render_stateful_widget(table, body, &mut state);
}
