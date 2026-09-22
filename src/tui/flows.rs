//! Short product flows shared by the three workspaces.
use super::{
    labels,
    modal::{any, Confirm, Modal, Outcome, Picker, Prompt, TextView},
    text, theme, App,
};
use crate::{native, runtime::Action};
use crossterm::event::{KeyCode as K, KeyEvent};
use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};

pub struct Auth {
    kind: String,
    after: Action,
    message: String,
}
impl Auth {
    pub fn new(kind: String, after: Action, message: String) -> Self {
        Self {
            kind,
            after,
            message,
        }
    }
}
impl Modal for Auth {
    any!();
    fn key(&mut self, app: &mut App, k: KeyEvent) -> Outcome {
        match k.code {
            K::Enter => {
                app.pending_auth = Some((self.kind.clone(), self.after.clone()));
                Outcome::Close
            }
            K::Esc => Outcome::Close,
            _ => Outcome::Stay,
        }
    }
    fn draw(&self, f: &mut Frame, area: Rect, _: &App) {
        let r = super::modal::popup(area, 70, 9);
        let inner = super::modal::frame(f, r, "Administrator access", "");
        f.render_widget(
            Paragraph::new(format!(
                "{}\n\nEnter continues in the normal terminal. Esc cancels.",
                self.message
            ))
            .wrap(Wrap { trim: false }),
            inner,
        );
    }
}

pub fn help(_: &App) -> TextView {
    TextView::plain(
        "sing help",
        "Three workspaces\n\
         1 Overview      health, traffic and current groups\n\
         2 Policies      proxy groups, ordered rules and sources\n\
         3 Activity      connections, observed apps and logs\n\n\
         Always available\n\
         s Start / Stop   m Mode   t TUN   : Config   A Review & Apply\n\n\
         Lists\n\
         ↑↓ or j/k move   Enter opens   ←/→ changes section\n\
         n new   e edit   x remove   Alt+↑↓ reorder (Option on Mac)\n\n\
         Activity\n\
         o sorts by recency / traffic. / filters. r creates or extends a local rule.\n\
         Browsing holds the view; collection continues. Space returns live. f configures process discovery.\n\
         d toggles the connection detail pane in wide terminals; Enter opens details at any size.\n\
         App names come from connections observed by sing-box; they are not a system-wide process inventory.\n\n\
         Overview: Tab switches groups / connections; c opens Activity; v checks HTTPS.\n\
         p toggles local macOS system proxy. q exits the UI, leaving the core running.\n\n\
         Saving changes the draft. A reviews and applies it. The running core is not changed while browsing or editing.",
    )
}

pub fn review_apply(app: &mut App) {
    app.request_busy(
        Action::ReviewApply,
        "Preparing review",
        Box::new(|app, r| {
            if !r.ok {
                return app.error(r.message);
            }
            let body = r
                .config
                .unwrap_or_else(|| "No readable summary was returned.".into());
            let Some(confirm) = r.confirm else {
                return app.error("The manager did not return an apply action");
            };
            app.push(Confirm::new(
                "Review & Apply",
                body,
                if app.snap.connected {
                    "Apply & Restart"
                } else {
                    "Apply & Start"
                },
                Box::new(move |app| {
                    app.request_busy(confirm, "Applying", Box::new(super::notify));
                }),
            ));
        }),
    );
}

pub fn import_subscription(app: &mut App) {
    app.push(Prompt::new(
        "Import subscription",
        "Paste a subscription URL, a local file path, or one or more share links. The source stays private. You will review detected nodes before anything is saved.",
        "",
        Box::new(|app, source| {
            if source.trim().is_empty() {
                return Err("A source is required".into());
            }
            app.request_busy(
                Action::Import { source, name: String::new(), user_agent: String::new() },
                "Reading subscription",
                Box::new(|app, r| {
                    if !r.ok {
                        return app.error(r.message);
                    }
                    let Some(p) = r.preview else {
                        return app.error("The manager returned no subscription preview");
                    };
                    let mut body = format!(
                        "{} · {}\n{} nodes · +{} / -{}\n\nSave updates only the nodes owned by this source. Groups, routing and DNS are preserved.",
                        p.name, p.format, p.count, p.added, p.removed
                    );
                    if !p.names.is_empty() {
                        body.push_str("\n\nDetected\n");
                        body.push_str(&p.names.iter().take(8).map(|n| format!("• {n}")).collect::<Vec<_>>().join("\n"));
                        if p.names.len() > 8 {
                            body.push_str(&format!("\n• … and {} more", p.names.len() - 8));
                        }
                    }
                    if !p.warnings.is_empty() {
                        body.push_str("\n\nNeeds attention\n");
                        body.push_str(&p.warnings.iter().map(|w| format!("• {w}")).collect::<Vec<_>>().join("\n"));
                    }
                    let action = Action::CommitSubscriptions { id: p.id, revision: p.revision };
                    app.push(Confirm::new(
                        "Review subscription",
                        body,
                        "Save to Draft",
                        Box::new(move |app| {
                            app.request_busy(action, "Saving subscription", Box::new(super::notify));
                        }),
                    ));
                }),
            );
            Ok(())
        }),
    ));
}

fn import_position(app: &App) -> usize {
    native::array(app.doc(), "/route/rules")
        .iter()
        .position(|r| {
            matches!(
                r["action"].as_str().unwrap_or("route"),
                "route" | "reject" | "bypass" | "hijack-dns"
            )
        })
        .unwrap_or_else(|| native::array(app.doc(), "/route/rules").len())
}

pub fn import_rule_set(app: &mut App) {
    let revision = native::revision(&app.snap.store);
    app.push(Prompt::new(
        "Import rule set",
        "Paste a remote URL, local path, or rule text. sing previews supported entries and keeps native source/SRS references intact.",
        "",
        Box::new(move |app, source| {
            if source.trim().is_empty() {
                return Err("A source is required".into());
            }
            app.request_busy(
                Action::PrepareRuleDraft {
                    source,
                    name: String::new(),
                    format: "auto".into(),
                    revision: revision.clone(),
                },
                "Reading rules",
                Box::new(|app, r| {
                    if !r.ok {
                        return app.error(r.message);
                    }
                    let Some(p) = r.rules_preview else {
                        return app.error("The manager returned no rule preview");
                    };
                    let choices = labels::targets(&app.snap.store, app.doc(), true);
                    if choices.is_empty() {
                        return app.error("Create a proxy group or outbound before importing rules");
                    }
                    let title = format!("{} · choose target", p.name);
                    let current = app.doc().pointer("/route/final").and_then(|v| v.as_str()).unwrap_or("");
                    app.push(Picker::single(
                        &title,
                        choices,
                        current,
                        Box::new(move |app, picked| {
                            let Some(target) = picked.into_iter().next() else { return };
                            let mut body = format!(
                                "{} · {}\n{} / {} entries supported\nSend matches to {}\nInsert before the first terminal routing rule.",
                                p.name,
                                p.format,
                                p.count,
                                p.input_count,
                                if target == "reject" { "Reject".into() } else { app.label(&target) }
                            );
                            if !p.warnings.is_empty() {
                                body.push_str("\n\nConversion warnings\n");
                                body.push_str(&p.warnings.iter().map(|w| format!("• {w}")).collect::<Vec<_>>().join("\n"));
                            }
                            let action = Action::CommitRuleDraft {
                                id: p.draft_id,
                                revision: p.revision,
                                target,
                                position: import_position(app),
                                group: None,
                            };
                            app.push(Confirm::new(
                                "Review rule import",
                                body,
                                "Save to Draft",
                                Box::new(move |app| {
                                    app.request_busy(action, "Saving rules", Box::new(super::notify));
                                }),
                            ));
                        }),
                    ));
                }),
            );
            Ok(())
        }),
    ));
}

pub fn connection_details(app: &App, c: &crate::api::Connection) -> TextView {
    let process = c.process.as_ref();
    let chain = super::history::route_path(c, |t| app.label(t));
    let target = super::history::route_target(c);
    let name = super::identity::display(c);
    let mut body = vec![
        Line::from(vec![
            Span::styled("Destination  ", theme::s(theme::dim())),
            Span::raw(if c.domain.is_empty() {
                c.destination.clone()
            } else {
                format!("{}  ({})", c.domain, c.destination)
            }),
        ]),
        Line::from(vec![
            Span::styled("Application  ", theme::s(theme::dim())),
            Span::raw(if name.is_empty() {
                super::identity::missing_label(app).into()
            } else {
                name
            }),
        ]),
        Line::from(vec![
            Span::styled("Process path ", theme::s(theme::dim())),
            Span::raw(
                process
                    .map(|p| p.path.clone())
                    .filter(|p| !p.is_empty())
                    .unwrap_or_else(|| "Not reported by core".into()),
            ),
        ]),
        Line::from(vec![
            Span::styled("Route        ", theme::s(theme::dim())),
            Span::styled(chain, theme::s(theme::target(&target))),
        ]),
        Line::from(vec![
            Span::styled("Matched rule ", theme::s(theme::dim())),
            Span::raw(if c.rule.is_empty() {
                "No matched rule reported".into()
            } else {
                c.rule.clone()
            }),
        ]),
        Line::from(vec![
            Span::styled("Traffic      ", theme::s(theme::dim())),
            Span::raw(format!(
                "↑ {}  ↓ {}",
                text::bytes(c.uplink_total),
                text::bytes(c.downlink_total)
            )),
        ]),
        Line::from(vec![
            Span::styled("Network      ", theme::s(theme::dim())),
            Span::raw(format!(
                "{} · {} · port {}",
                c.network,
                c.protocol,
                super::history::port(c)
            )),
        ]),
    ];
    if let Some(p) = process.filter(|p| p.pid > 0) {
        body.push(Line::raw(format!("Process ID   {}", p.pid)));
    }
    if process.is_none_or(|p| p.path.is_empty()) {
        body.push(Line::raw(""));
        body.push(Line::styled(
            super::identity::explanation(app),
            theme::s(theme::warn()),
        ));
    }
    body.push(Line::raw(""));
    body.push(Line::styled(
        "Domain / IP evidence only; HTTPS URL paths are not inspected.",
        theme::s(theme::dim()),
    ));
    TextView {
        title: "Connection evidence".into(),
        body,
        scroll: 0,
    }
}
