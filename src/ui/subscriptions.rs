use super::*;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

pub(super) fn draw_source(f: &mut Frame, form: &Form, area: Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)])
        .split(area);
    let accent = Color::Rgb(114, 216, 191);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Import Subscription ");
    let inner = block.inner(rows[0]);
    f.render_widget(block, rows[0]);
    let mut lines = vec![];
    for (i, field) in form.fields.iter().take(form.visible_fields()).enumerate() {
        lines.push(Line::styled(
            format!(
                "{} {}",
                if form.selected == i { "›" } else { " " },
                field.label
            ),
            Style::default().fg(if form.selected == i {
                accent
            } else {
                Color::Gray
            }),
        ));
        let value = if field.input.value.is_empty() {
            if field.key == "name" {
                "Automatic from source"
            } else if field.key == "source" {
                "Paste a URL, node links, or file path"
            } else {
                "Use default"
            }
            .to_string()
        } else if ["source", "user_agent"].contains(&field.key.as_str()) {
            "[private value · Ctrl+U to replace]".into()
        } else {
            model::clean(&field.input.value)
        };
        lines.push(Line::raw(format!("  {value}")));
    }
    let advanced = matches!(form.action, FormAction::Import { advanced: true });
    let configured = form
        .fields
        .iter()
        .any(|v| v.key == "user_agent" && !v.input.value.is_empty());
    lines.push(Line::styled(
        format!(
            "{} [{} Advanced{}]",
            if form.selected == form.visible_fields() {
                "›"
            } else {
                " "
            },
            if advanced { "−" } else { "+" },
            if configured { " · configured" } else { "" }
        ),
        Style::default().fg(if form.selected == form.visible_fields() {
            accent
        } else {
            Color::Gray
        }),
    ));
    let focused_line = (form.selected * 2).min(lines.len().saturating_sub(1));
    let scroll = focused_line.saturating_sub(inner.height.saturating_sub(1) as usize);
    f.render_widget(Paragraph::new(lines).scroll((scroll as u16, 0)), inner);
    let labels = ["Review Import", "Cancel"]
        .iter()
        .enumerate()
        .map(|(i, label)| {
            format!(
                "{}[{label}]",
                if form.selected == form.submit_focus() + i {
                    "›"
                } else {
                    " "
                }
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    f.render_widget(
        Paragraph::new(format!(
            "{labels}\nTab Focus · Enter Action · F2 Review · Esc Cancel"
        ))
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(accent)),
        rows[1],
    );
}
#[derive(Clone)]
pub(super) struct SubscriptionReview {
    pub preview: runtime::ImportPreview,
    pub form: Option<Form>,
    pub focus: usize,
    pub scroll: u16,
}
impl SubscriptionReview {
    pub fn buttons(&self) -> Vec<&'static str> {
        if self.form.is_some() {
            vec![
                "Save & Set Up",
                "Save Only",
                "Back to Source",
                "Refresh Preview",
                "Cancel",
            ]
        } else {
            vec!["Save Updates", "Refresh Preview", "Cancel"]
        }
    }
    pub fn report(&self) -> String {
        let p = &self.preview;
        format!("{} · {}\n{} nodes · +{} / -{}\nDraft only; running configuration stays unchanged.\n\n{}\n{}\n{}",p.name,p.format,p.count,p.added,p.removed,p.sources.join("\n"),p.names.join("\n"),p.warnings.join("\n"))
    }
}
#[derive(Clone)]
pub(super) struct Setup {
    pub revision: String,
    pub targets: Vec<(String, String)>,
    pub target: usize,
    pub mode: usize,
    pub captures: Vec<(&'static str, &'static str)>,
    pub capture: usize,
    pub focus: usize,
}
impl Setup {
    pub fn new(a: &App) -> Result<Self> {
        let doc = native::migration(&a.snapshot.store)?;
        let mut targets = vec![(String::new(), "Keep current target".into())];
        targets.extend(
            ["/outbounds", "/endpoints"]
                .iter()
                .flat_map(|p| native::array(&doc, p))
                .map(|v| (native::tag(v).into(), a.label(native::tag(v)))),
        );
        let mut captures = vec![
            ("keep", "Keep current capture"),
            ("port", "Proxy Ports — configure apps manually"),
        ];
        if cfg!(target_os = "macos") && !a.snapshot.ssh {
            captures.push(("system", "System Proxy — supported apps"));
        }
        captures.push(("tun", "TUN — administrator access required"));
        Ok(Self {
            revision: native::revision(&a.snapshot.store),
            targets,
            target: 0,
            mode: ["rule", "global", "direct"]
                .iter()
                .position(|s| *s == a.snapshot.store.settings.route_mode)
                .unwrap_or(0),
            captures,
            capture: 0,
            focus: 0,
        })
    }
    pub fn change(&self) -> native::ConnectionSetup {
        native::ConnectionSetup {
            revision: self.revision.clone(),
            target: (self.target > 0).then(|| self.targets[self.target].0.clone()),
            mode: ["rule", "global", "direct"][self.mode].into(),
            capture: self.captures[self.capture].0.into(),
        }
    }
    pub fn dirty(&self, a: &App) -> bool {
        self.target > 0
            || self.capture > 0
            || ["rule", "global", "direct"][self.mode] != a.snapshot.store.settings.route_mode
    }
}
pub(super) fn draw(f: &mut Frame, d: &Dialog, area: Rect, a: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)])
        .split(area);
    let accent = Color::Rgb(114, 216, 191);
    match d {
        Dialog::Subscription(r) => {
            f.render_widget(
                Paragraph::new(r.report())
                    .wrap(Wrap { trim: false })
                    .scroll((r.scroll, 0))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(" Review Subscription "),
                    ),
                chunks[0],
            );
            let labels = r
                .buttons()
                .iter()
                .enumerate()
                .map(|(i, b)| format!("{}[{b}]", if i == r.focus { "›" } else { " " }))
                .collect::<Vec<_>>()
                .join(" ");
            f.render_widget(
                Paragraph::new(format!(
                    "{labels}\nTab Focus · Enter Action · ↑↓ Scroll · Esc Cancel"
                ))
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(accent)),
                chunks[1],
            );
        }
        Dialog::Setup(s) => {
            let b = Block::default()
                .borders(Borders::ALL)
                .title(" Connection Setup ");
            let inner = b.inner(chunks[0]);
            f.render_widget(b, chunks[0]);
            let current = a.doc();
            let target = if s.mode == 1 {
                a.snapshot.store.settings.global_target.clone()
            } else {
                current
                    .pointer("/route/final")
                    .and_then(Value::as_str)
                    .unwrap_or("proxy")
                    .into()
            };
            let labels = [
                format!("Target: {}", s.targets[s.target].1),
                format!("Mode: {}", ["Rule", "Global", "Direct"][s.mode]),
                format!("Capture: {}", s.captures[s.capture].1),
                "[Review Setup]".into(),
                "[Done for Now]".into(),
                "[Network Settings]".into(),
                "[Core Installation]".into(),
            ];
            let mut lines = vec![
                Line::raw(format!(
                    "Current target: {} · capture: {}",
                    a.label(&target),
                    if native::uses_tun(&a.snapshot.store) {
                        "Custom / TUN"
                    } else {
                        &a.snapshot.store.settings.mode
                    }
                )),
                Line::raw(if a.snapshot.ssh {
                    "SSH: remote host only. TUN can interrupt this session."
                } else {
                    "Rule target: unmatched traffic. Global has its own target."
                }),
                Line::raw("DNS and rules unchanged. Startup requires confirmation."),
            ];
            for (i, l) in labels.iter().enumerate() {
                lines.push(Line::styled(
                    format!("{} {l}", if s.focus == i { "›" } else { " " }),
                    Style::default().fg(if s.focus == i { accent } else { Color::Gray }),
                ));
            }
            let scroll = (s.focus + 3).saturating_sub(inner.height.saturating_sub(1) as usize);
            f.render_widget(Paragraph::new(lines).scroll((scroll as u16, 0)), inner);
            f.render_widget(Paragraph::new("Tab Focus · ←→ Choose · Enter Action\nEsc closes setup; saved subscription is kept.").style(Style::default().fg(accent)),chunks[1]);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn press(a: &mut App, key: K) -> Option<Action> {
        a.key(KeyEvent::new(key, M::NONE)).unwrap()
    }
    fn reply(ok: bool) -> Reply {
        serde_json::from_value(json!({"ok":ok,"message":"fixture reply","needs_auth":false}))
            .unwrap()
    }
    fn preview(a: &mut App) {
        a.import(false);
        for c in "trojan://private@fixture.invalid:443#Fixture".chars() {
            press(a, K::Char(c));
        }
        assert!(matches!(press(a, K::F(2)), Some(Action::Import { .. })));
        let mut r = reply(true);
        r.preview = Some(runtime::ImportPreview {
            id: "preview-id".into(),
            revision: "server-revision".into(),
            sources: vec![],
            name: "Fixture".into(),
            format: "uri".into(),
            count: 1,
            added: 1,
            removed: 0,
            warnings: vec![],
            names: vec!["Fixture".into()],
        });
        a.receive(r).unwrap();
    }
    #[test]
    fn import_advanced_is_optional_and_retains_values_when_collapsed() {
        let mut a = App::new(sample().unwrap(), true);
        a.import(false);
        assert!(press(&mut a, K::F(2)).is_none());
        assert!(a.notice.contains("Enter a subscription"));
        for c in "https://fixture.invalid/sub?token=secret".chars() {
            press(&mut a, K::Char(c));
        }
        press(&mut a, K::Tab); // Advanced
        press(&mut a, K::Enter);
        press(&mut a, K::BackTab); // User-Agent
        for c in "fixture-agent".chars() {
            press(&mut a, K::Char(c));
        }
        press(&mut a, K::Tab);
        press(&mut a, K::Enter); // Collapse; keep User-Agent
        let Some(Dialog::Form(form)) = &a.dialog else {
            panic!()
        };
        assert_eq!(form.visible_fields(), 2);
        assert_eq!(form.fields[2].input.value, "fixture-agent");
        press(&mut a, K::Tab);
        assert!(
            matches!(press(&mut a, K::Enter), Some(Action::Import { user_agent, .. }) if user_agent == "fixture-agent")
        );
        let mut r = reply(false);
        r.message =
            "Cannot fetch https://fixture.invalid/sub?token=secret with fixture-agent".into();
        a.snapshot.store.secret.clear();
        a.receive(r).unwrap();
        let Some(Dialog::Failure { message, .. }) = &a.dialog else {
            panic!()
        };
        assert_eq!(message, "Cannot fetch [private value] with [private value]");
        assert!(press(&mut a, K::Enter).is_none());
        let Some(Dialog::Form(form)) = &a.dialog else {
            panic!()
        };
        assert_eq!(form.visible_fields(), 2);
        assert!(form.fields[1].input.value.contains("token=secret"));
        assert!(
            matches!(press(&mut a, K::Enter), Some(Action::Import { user_agent, .. }) if user_agent == "fixture-agent")
        );
    }
    #[test]
    fn source_and_error_views_fit_small_terminals_without_private_values() {
        let mut a = App::new(sample().unwrap(), true);
        a.import(false);
        for advanced in [false, true] {
            let Some(Dialog::Form(form)) = &mut a.dialog else {
                panic!()
            };
            form.action = FormAction::Import { advanced };
            form.fields[1].input = Input::new("https://fixture.invalid/?token=secret".into());
            form.fields[2].input = Input::new("private-agent".into());
            let count = form.submit_focus() + 2;
            for focus in 0..count {
                let Some(Dialog::Form(form)) = &mut a.dialog else {
                    panic!()
                };
                form.selected = focus;
                for (w, h) in [(54, 18), (80, 24), (140, 40)] {
                    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
                    t.draw(|f| view::draw(f, &a)).unwrap();
                    let text = t
                        .backend()
                        .buffer()
                        .content
                        .iter()
                        .map(|c| c.symbol())
                        .collect::<String>();
                    assert!(
                        text.contains("Review Import")
                            && text.contains("Cancel")
                            && text.contains('›')
                    );
                    assert!(!text.contains("token=secret") && !text.contains("private-agent"));
                }
            }
        }
        press(&mut a, K::F(2));
        a.receive(reply(false)).unwrap();
        for (w, h) in [(54, 18), (80, 24), (140, 40)] {
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            t.draw(|f| view::draw(f, &a)).unwrap();
            let text = t
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            assert!(text.contains("Action Failed") && text.contains("[Back]"));
        }
    }
    #[test]
    fn save_and_setup_uses_server_revision_not_redacted_snapshot_hash() {
        let mut a = App::new(sample().unwrap(), true);
        preview(&mut a);
        let action = press(&mut a, K::Enter).unwrap();
        assert!(matches!(action,Action::CommitSubscriptions{ref id,..} if id=="preview-id"));
        assert!(matches!(
            a.receive(reply(true)).unwrap(),
            Some(Action::ConnectionSetupInfo)
        ));
        a.snapshot.store.secret.clear();
        let mut r = reply(true);
        r.edit = Some(Edit {
            revision: "authoritative-new-revision".into(),
            pointer: String::new(),
            value: a.doc(),
        });
        a.receive(r).unwrap();
        for _ in 0..3 {
            press(&mut a, K::Tab);
        }
        assert!(
            matches!(press(&mut a,K::Enter),Some(Action::ReviewConnectionSetup(s)) if s.revision=="authoritative-new-revision")
        );
        a.receive(reply(false)).unwrap();
        assert!(matches!(a.dialog, Some(Dialog::Failure { .. })));
        assert!(press(&mut a, K::Enter).is_none());
        assert!(matches!(a.dialog, Some(Dialog::Setup(_))));
    }
    #[test]
    fn save_only_and_cancel_do_not_start_setup_or_erase_inputs() {
        let mut a = App::new(sample().unwrap(), true);
        let before = a.doc();
        preview(&mut a);
        press(&mut a, K::Tab);
        press(&mut a, K::Enter);
        a.receive(reply(true)).unwrap();
        assert!(a.dialog.is_none());
        preview(&mut a);
        press(&mut a, K::Enter);
        a.receive(reply(false)).unwrap();
        assert!(matches!(a.dialog, Some(Dialog::Failure { .. })));
        assert!(press(&mut a, K::Esc).is_none());
        let Some(Dialog::Subscription(r)) = &a.dialog else {
            panic!()
        };
        assert!(r.form.as_ref().unwrap().fields[1]
            .input
            .value
            .contains("private"));
        press(&mut a, K::Tab);
        press(&mut a, K::Tab);
        assert!(matches!(
            press(&mut a, K::Enter),
            Some(Action::CancelSubscriptions(_))
        ));
        assert!(matches!(a.dialog, Some(Dialog::Form(_))));
        press(&mut a, K::Esc);
        assert!(matches!(a.dialog, Some(Dialog::Discard { .. })));
        press(&mut a, K::Enter);
        assert_eq!(a.doc(), before);
    }
    #[test]
    fn setup_review_back_preserves_choices_and_ssh_omits_system_proxy() {
        let mut a = App::new(sample().unwrap(), true);
        a.snapshot.ssh = true;
        let mut setup = Setup::new(&a).unwrap();
        assert!(!setup.captures.iter().any(|c| c.0 == "system"));
        setup.target = 1;
        setup.focus = 3;
        let change = setup.change();
        a.dialog = Some(Dialog::Setup(Box::new(setup)));
        press(&mut a, K::Enter);
        let mut r = reply(true);
        r.config = Some("Preview only".into());
        r.confirm = Some(Action::SaveConnectionSetup(change));
        a.receive(r).unwrap();
        assert!(matches!(a.dialog, Some(Dialog::SetupReview { .. })));
        press(&mut a, K::Esc);
        let Some(Dialog::Setup(s)) = &a.dialog else {
            panic!()
        };
        assert_eq!(s.target, 1);
    }
    #[test]
    fn review_buttons_render_without_credentials_on_small_terminals() {
        let mut a = App::new(sample().unwrap(), true);
        preview(&mut a);
        for (w, h) in [(54, 18), (80, 24), (140, 40)] {
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            t.draw(|f| view::draw(f, &a)).unwrap();
            let text = t
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            assert!(text.contains("Save & Set Up") && text.contains("Cancel"));
            assert!(!text.contains("private@"));
        }
        a.dialog = Some(Dialog::Setup(Box::new(Setup::new(&a).unwrap())));
        for focus in 0..7 {
            let Some(Dialog::Setup(s)) = &mut a.dialog else {
                panic!()
            };
            s.focus = focus;
            let mut t = Terminal::new(TestBackend::new(80, 24)).unwrap();
            t.draw(|f| view::draw(f, &a)).unwrap();
            let text = t
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            assert!(text.contains("›"));
            assert!(text.contains("Tab Focus"));
        }
    }
}
