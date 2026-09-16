use super::*;

#[derive(Clone)]
pub(super) struct RuleImport {
    pub revision: String,
    pub form: Form,
    pub preview: Option<runtime::RulesPreview>,
    pub pending_id: Option<String>,
    pub warnings_reviewed: bool,
    pub choices: Vec<(String, String)>,
    pub target: usize,
    pub position: usize,
    pub rules: Vec<Value>,
    pub group: Option<native::GroupChange>,
    pub focus: usize,
}

impl RuleImport {
    pub fn preview_label(&self) -> String {
        let Some(p) = &self.preview else {
            return String::new();
        };
        if ["native-source", "native-srs"].contains(&p.format.as_str()) {
            format!(
                "{} · native reference · contents loaded on Apply",
                model::clean(&p.name)
            )
        } else {
            format!(
                "{} · {} · {} / {} supported",
                model::clean(&p.name),
                p.format,
                p.count,
                p.input_count
            )
        }
    }
    pub fn new(edit: Edit, app: &App) -> Self {
        let doc = app.doc();
        let mut choices: Vec<_> = ["/outbounds", "/endpoints"]
            .iter()
            .flat_map(|p| native::array(&doc, p))
            .map(|v| (native::tag(v).to_string(), app.label(native::tag(v))))
            .collect();
        choices.push(("reject".into(), "Reject".into()));
        let target = choices
            .iter()
            .position(|(t, _)| {
                Some(t.as_str()) == doc.pointer("/route/final").and_then(Value::as_str)
            })
            .unwrap_or(0);
        let rules = native::array(&doc, "/route/rules").to_vec();
        Self {
            revision: edit.revision,
            form: Form {
                title: "Import Rule Set".into(),
                fields: vec![
                    Field::new("name", "Name (optional)", &json!(""), Kind::String),
                    Field::new(
                        "source",
                        "Rule URL / pasted rules / local file",
                        &json!(""),
                        Kind::String,
                    ),
                    Field::new(
                        "format",
                        "Format",
                        &json!("auto"),
                        Kind::Choice(vec![
                            "auto".into(),
                            "qx".into(),
                            "clash".into(),
                            "domain".into(),
                            "ipcidr".into(),
                            "native".into(),
                            "native-source".into(),
                            "native-srs".into(),
                        ]),
                    ),
                ],
                selected: 1,
                action: FormAction::Convert,
            },
            preview: None,
            pending_id: None,
            warnings_reviewed: false,
            choices,
            target,
            position: rules.len(),
            rules,
            group: None,
            focus: 0,
        }
    }
    pub fn prepare(&self) -> Action {
        let get = |k: &str| {
            self.form
                .fields
                .iter()
                .find(|f| f.key == k)
                .unwrap()
                .input
                .value
                .clone()
        };
        Action::PrepareRuleDraft {
            source: get("source"),
            name: get("name"),
            format: get("format"),
            revision: self.revision.clone(),
        }
    }
    pub fn commit(&self) -> Result<Action> {
        let preview = self.preview.as_ref().context("Review the source first")?;
        ensure!(
            preview.warnings.is_empty() || self.warnings_reviewed,
            "Open Conversion Report to review excluded rules before saving"
        );
        let target = self
            .choices
            .get(self.target)
            .context("Choose a target")?
            .0
            .clone();
        let group = self
            .group
            .clone()
            .filter(|g| native::tag(&g.value) == target);
        Ok(Action::CommitRuleDraft {
            id: preview.draft_id.clone(),
            revision: self.revision.clone(),
            target,
            position: self.position,
            group,
        })
    }
    pub fn report(&self) -> String {
        let Some(p) = &self.preview else {
            return String::new();
        };
        let mut text = format!(
            "{} · {}\n{} / {} supported · +{} / -{}\nDNS unchanged\n\n",
            p.name, p.format, p.count, p.input_count, p.added, p.removed
        );
        if !p.warnings.is_empty() {
            text.push_str("Conversion warnings — saving accepts these exclusions:\n");
            text.push_str(&p.warnings.join("\n"));
            text.push_str("\n\n");
        }
        if ["native-source", "native-srs"].contains(&p.format.as_str()) {
            text = format!(
                "{} · {}\nNative reference; rule count unknown. DNS unchanged.\n\n",
                p.name, p.format
            );
        } else if p.format == "native" {
            text.push_str("Native conditions preserved; selected core validates on Apply.\n\n");
        }
        if !p.policies.is_empty() {
            text.push_str("Source policies are not executed: ");
            text.push_str(&p.policies.join(", "));
            text.push_str("\n\n");
        }
        text.push_str(&p.sample.join("\n"));
        text
    }
    pub fn position_label(&self) -> String {
        if self.position == self.rules.len() {
            "After the last rule, before the default target".into()
        } else {
            format!(
                "Before {}: {}",
                self.position + 1,
                title(&self.rules[self.position])
            )
        }
    }
    pub fn shadow_warning(&self) -> bool {
        self.rules.iter().take(self.position).any(|r| {
            r.as_object().is_some_and(|m| {
                m.keys()
                    .all(|k| ["action", "outbound", "server"].contains(&k.as_str()))
            }) && ["route", "reject"].contains(&r["action"].as_str().unwrap_or(""))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn press(a: &mut App, code: K) -> Option<Action> {
        a.key(KeyEvent::new(code, M::NONE)).unwrap()
    }
    fn reply(ok: bool) -> Reply {
        serde_json::from_value(json!({"ok":ok,"message":"fixture reply","needs_auth":false}))
            .unwrap()
    }
    fn imported(a: &mut App) {
        let e = native::read(&a.snapshot.store, "/route".into()).unwrap();
        a.dialog = Some(Dialog::RuleImport(Box::new(RuleImport::new(e, a))));
        for c in "HOST-SUFFIX,video.invalid,Media".chars() {
            press(a, K::Char(c));
        }
        press(a, K::Tab);
        press(a, K::Tab);
        assert!(matches!(
            press(a, K::Enter),
            Some(Action::PrepareRuleDraft { .. })
        ));
        let mut r = reply(true);
        r.rules_preview = Some(runtime::RulesPreview {
            draft_id: "preview-token".into(),
            revision: native::revision(&a.snapshot.store),
            name: "Video".into(),
            format: "qx".into(),
            input_count: 2,
            count: 1,
            added: 1,
            removed: 0,
            target: String::new(),
            warnings: vec!["Line 2: unsupported match".into()],
            policies: vec!["Ignored external policy".into()],
            sample: vec!["domain_suffix video.invalid".into()],
        });
        a.receive(r).unwrap();
    }
    #[test]
    fn inline_group_is_staged_and_cancel_returns_to_parent_without_writes() {
        let mut a = App::new(sample().unwrap(), true);
        let before = a.doc();
        imported(&mut a);
        press(&mut a, K::Tab);
        press(&mut a, K::Tab);
        press(&mut a, K::Enter);
        assert!(matches!(a.dialog, Some(Dialog::InlineGroup { .. })));
        press(&mut a, K::Char('X'));
        press(&mut a, K::Esc);
        assert!(matches!(a.dialog, Some(Dialog::Discard { .. })));
        press(&mut a, K::Esc); // Keep editing the inline group.
        assert!(matches!(a.dialog, Some(Dialog::InlineGroup { .. })));
        press(&mut a, K::Esc);
        press(&mut a, K::Enter); // Discard only the inline edits.
        assert!(matches!(a.dialog, Some(Dialog::RuleImport(_))));
        press(&mut a, K::Enter); // New Group remains focused.
        for c in "Media".chars() {
            press(&mut a, K::Char(c));
        }
        for _ in 0..3 {
            press(&mut a, K::Tab);
        }
        press(&mut a, K::Char(' '));
        for _ in 0..3 {
            press(&mut a, K::Tab);
        }
        assert!(press(&mut a, K::Enter).is_none()); // Use Group, no persistence action.
        let Some(Dialog::RuleImport(r)) = &a.dialog else {
            panic!()
        };
        assert_eq!(r.group.as_ref().unwrap().name, "Media");
        assert_eq!(
            r.form.fields[1].input.value,
            "HOST-SUFFIX,video.invalid,Media"
        );
        assert_eq!(a.doc(), before);
        press(&mut a, K::Esc);
        assert!(
            matches!(press(&mut a,K::Enter),Some(Action::CancelRuleDraft(id)) if id=="preview-token")
        );
        assert!(a.dialog.is_none());
        assert_eq!(a.doc(), before);
    }
    #[test]
    fn rule_report_and_failed_commit_preserve_inputs_and_position() {
        let mut a = App::new(sample().unwrap(), true);
        imported(&mut a);
        let Some(Dialog::RuleImport(r)) = &mut a.dialog else {
            panic!()
        };
        r.position = 0;
        r.focus = 3;
        press(&mut a, K::Enter);
        let Some(Dialog::RuleReport { parent, .. }) = &a.dialog else {
            panic!()
        };
        assert!(parent.report().contains("unsupported match"));
        assert!(parent.report().contains("DNS unchanged"));
        press(&mut a, K::Esc);
        press(&mut a, K::Tab);
        press(&mut a, K::Tab);
        assert!(matches!(
            press(&mut a, K::Enter),
            Some(Action::CommitRuleDraft { position: 0, .. })
        ));
        a.receive(reply(false)).unwrap();
        assert!(matches!(a.dialog, Some(Dialog::Failure { .. })));
        assert!(press(&mut a, K::Enter).is_none());
        let Some(Dialog::RuleImport(r)) = &a.dialog else {
            panic!()
        };
        assert_eq!(r.position, 0);
        assert_eq!(r.preview.as_ref().unwrap().draft_id, "preview-token");
        assert_eq!(r.focus, 5);
        press(&mut a, K::BackTab);
        assert!(matches!(
            press(&mut a, K::Enter),
            Some(Action::ReadNative(_))
        ));
        let mut refreshed = reply(true);
        refreshed.edit = Some(native::read(&a.snapshot.store, "/route".into()).unwrap());
        a.receive(refreshed).unwrap();
        press(&mut a, K::Esc);
        assert!(matches!(
            press(&mut a, K::Enter),
            Some(Action::CancelRuleDraft(_))
        ));
    }
    #[test]
    fn warning_is_scoped_to_certain_earlier_catch_all_rules() {
        let a = App::new(sample().unwrap(), true);
        let mut r = RuleImport::new(
            native::read(&a.snapshot.store, "/route".into()).unwrap(),
            &a,
        );
        r.rules = vec![
            json!({"action":"sniff"}),
            json!({"action":"route","outbound":"direct"}),
        ];
        r.position = 1;
        assert!(!r.shadow_warning());
        r.position = 2;
        assert!(r.shadow_warning());
        r.rules[1]["domain_suffix"] = json!(["limited.invalid"]);
        assert!(!r.shadow_warning());
    }
    #[test]
    fn saving_before_reading_conversion_warnings_keeps_the_form() {
        let mut a = App::new(sample().unwrap(), true);
        imported(&mut a);
        let Some(Dialog::RuleImport(r)) = &mut a.dialog else {
            panic!()
        };
        r.focus = 5;
        assert!(press(&mut a, K::Enter).is_none());
        assert!(a.error && a.notice.contains("Conversion Report"));
        let Some(Dialog::RuleImport(r)) = &a.dialog else {
            panic!()
        };
        assert_eq!(r.focus, 3);
        assert!(r.preview.is_some());
        assert!(!r.form.fields[1].input.value.is_empty());
    }
    #[test]
    fn binding_form_has_visible_actions_on_small_and_wide_terminals() {
        let mut a = App::new(sample().unwrap(), true);
        imported(&mut a);
        for (w, h) in [(54, 18), (80, 24), (140, 40)] {
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            t.draw(|f| {
                view::draw_dialog(
                    f,
                    a.dialog.as_ref().unwrap(),
                    ratatui::layout::Rect::new(0, 0, w, h),
                    Some(&a),
                )
            })
            .unwrap();
            let text = t
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            for expected in [
                "Send matching traffic to",
                "Position:",
                "[New Group]",
                "[Save Rule to Draft]",
                "[Cancel]",
            ] {
                assert!(text.contains(expected), "{w}x{h}: {expected}");
            }
        }
    }
}
