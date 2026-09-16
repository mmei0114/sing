//! One group worksheet, including members; no second member-save transaction.
use super::*;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

#[derive(Clone)]
pub(super) struct GroupEditor {
    pub edit: Edit,
    pub name: Input,
    original_name: String,
    pub automatic: bool,
    pub members: Vec<String>,
    pub default: String,
    pub query: Input,
    pub choices: Vec<(String, String)>,
    pub selected: usize,
    pub focus: usize,
    pub advanced: bool,
    pub url: Input,
    pub interval: Input,
    pub tolerance: Input,
    pub interrupt: bool,
    doc: Value,
    new: bool,
}

impl GroupEditor {
    pub fn new(mut edit: Edit, app: &App) -> Result<Self> {
        let new = edit.pointer.ends_with("/-");
        if new {
            if edit.value.is_null() {
                edit.value = json!({"type":"selector","outbounds":[]});
            }
            edit.value["tag"] = json!(format!("group-{}", &model::token()?[..12]));
        }
        let v = &edit.value;
        let name = if new {
            String::new()
        } else {
            app.label(native::tag(v))
        };
        let doc = app.doc();
        let mut choices = vec![];
        for path in ["/outbounds", "/endpoints"] {
            for item in native::array(&doc, path) {
                let tag = native::tag(item);
                if tag == native::tag(v) {
                    continue;
                }
                let source = app
                    .snapshot
                    .store
                    .nodes
                    .iter()
                    .find(|n| n.tag() == tag)
                    .and_then(|n| {
                        app.snapshot
                            .store
                            .subscriptions
                            .iter()
                            .find(|s| s.id == n.provider)
                    })
                    .map(|s| format!(" · {}", s.name))
                    .unwrap_or_default();
                choices.push((
                    tag.to_string(),
                    format!("{} · {}{source}", app.label(tag), text(&item["type"])),
                ));
            }
        }
        let members: Vec<String> = native::array(v, "/outbounds")
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        // Retain stale references visibly until the user removes or repairs them.
        for tag in &members {
            if !choices.iter().any(|(t, _)| t == tag) {
                choices.push((tag.clone(), format!("{tag} · missing")));
            }
        }
        Ok(Self {
            name: Input::new(name.clone()),
            original_name: name,
            automatic: v["type"] == "urltest",
            members,
            default: text(&v["default"]),
            query: Input::new(String::new()),
            choices,
            selected: 0,
            focus: 0,
            advanced: false,
            url: Input::new(
                v["url"]
                    .as_str()
                    .unwrap_or("https://www.gstatic.com/generate_204")
                    .into(),
            ),
            interval: Input::new(v["interval"].as_str().unwrap_or("3m").into()),
            tolerance: Input::new(v.get("tolerance").map(text).unwrap_or("50".into())),
            interrupt: v["interrupt_exist_connections"].as_bool().unwrap_or(false),
            doc,
            new,
            edit,
        })
    }
    pub fn save_focus(&self) -> usize {
        if self.advanced {
            if self.automatic {
                10
            } else {
                7
            }
        } else {
            6
        }
    }
    pub fn filtered(&self) -> Vec<&(String, String)> {
        let q = self.query.value.to_lowercase();
        self.choices
            .iter()
            .filter(|(tag, label)| format!("{tag} {label}").to_lowercase().contains(&q))
            .collect()
    }
    pub fn change(&self) -> Result<native::GroupChange> {
        ensure!(!self.name.value.trim().is_empty(), "Group name is required");
        let mut value = self.edit.value.clone();
        let kind = if self.automatic {
            "urltest"
        } else {
            "selector"
        };
        let changed_kind = value["type"] != kind;
        value["type"] = json!(kind);
        value["outbounds"] = json!(self.members);
        if self.automatic {
            value.as_object_mut().unwrap().remove("default");
            for (key, input) in [
                ("url", &self.url),
                ("interval", &self.interval),
                ("tolerance", &self.tolerance),
            ] {
                let initial = match key {
                    "url" => self.edit.value[key]
                        .as_str()
                        .unwrap_or("https://www.gstatic.com/generate_204")
                        .into(),
                    "interval" => self.edit.value[key].as_str().unwrap_or("3m").into(),
                    _ => self.edit.value.get(key).map(text).unwrap_or("50".into()),
                };
                if self.new || changed_kind || input.value != initial {
                    if input.value.is_empty() {
                        value.as_object_mut().unwrap().remove(key);
                    } else {
                        value[key] = if key == "tolerance" {
                            json!(input
                                .value
                                .parse::<u16>()
                                .context("Tolerance must be a whole number from 0 to 65535")?)
                        } else {
                            json!(input.value)
                        };
                    }
                }
            }
        } else {
            if changed_kind {
                for key in ["url", "interval", "tolerance"] {
                    value.as_object_mut().unwrap().remove(key);
                }
            }
            if self.default.is_empty() {
                value.as_object_mut().unwrap().remove("default");
            } else {
                value["default"] = json!(self.default);
            }
        }
        if self.interrupt
            != self.edit.value["interrupt_exist_connections"]
                .as_bool()
                .unwrap_or(false)
        {
            value["interrupt_exist_connections"] = json!(self.interrupt);
        }
        native::validate_group(&self.doc, &value)?;
        Ok(native::GroupChange {
            revision: self.edit.revision.clone(),
            original_tag: (!self.new).then(|| native::tag(&self.edit.value).to_string()),
            name: self.name.value.trim().into(),
            value,
        })
    }
    pub fn dirty(&self) -> bool {
        let v = &self.edit.value;
        self.name.value != self.original_name
            || self.automatic != (v["type"] == "urltest")
            || json!(self.members) != v["outbounds"]
            || self.default != text(&v["default"])
            || self.url.value
                != v["url"]
                    .as_str()
                    .unwrap_or("https://www.gstatic.com/generate_204")
            || self.interval.value != v["interval"].as_str().unwrap_or("3m")
            || self.tolerance.value != v.get("tolerance").map(text).unwrap_or("50".into())
            || self.interrupt != v["interrupt_exist_connections"].as_bool().unwrap_or(false)
    }
    pub fn paste(&mut self, s: &str) {
        match self.focus {
            0 => self.name.insert(s),
            2 => {
                self.query.insert(s);
                self.selected = 0;
            }
            6 if self.advanced && self.automatic => self.url.insert(s),
            7 if self.advanced && self.automatic => self.interval.insert(s),
            8 if self.advanced && self.automatic => self.tolerance.insert(s),
            _ => {}
        }
    }
    pub fn key(&mut self, k: KeyEvent) -> Result<Option<native::GroupChange>> {
        let save = self.save_focus();
        if k.code == K::F(2)
            || (k.code == K::Char('s') && k.modifiers.contains(M::CONTROL))
            || (k.code == K::Enter && self.focus == save)
        {
            return self.change().map(Some);
        }
        if matches!(k.code, K::Tab | K::BackTab) {
            self.focus = (self.focus + if k.code == K::Tab { 1 } else { save + 1 }) % (save + 2);
            return Ok(None);
        }
        match self.focus {
            0 => self.name.key(k, false),
            1 if matches!(k.code, K::Left | K::Right | K::Char(' ') | K::Enter) => {
                self.automatic = !self.automatic;
                if self.automatic {
                    self.advanced = true;
                }
            }
            2 => {
                self.query.key(k, false);
                self.selected = 0;
            }
            3 => {
                let filtered = self.filtered();
                match k.code {
                    K::Up => self.selected = self.selected.saturating_sub(1),
                    K::Down => {
                        self.selected = (self.selected + 1).min(filtered.len().saturating_sub(1))
                    }
                    K::Char(' ') | K::Enter => {
                        if let Some((tag, _)) = filtered.get(self.selected) {
                            let tag = tag.clone();
                            if self.members.contains(&tag) {
                                self.members.retain(|m| m != &tag);
                                if self.default == tag {
                                    self.default =
                                        self.members.first().cloned().unwrap_or_default();
                                }
                            } else {
                                self.members.push(tag.clone());
                                if self.default.is_empty() {
                                    self.default = tag;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            4 if !self.automatic
                && matches!(k.code, K::Left | K::Right | K::Char(' ') | K::Enter) =>
            {
                if !self.members.is_empty() {
                    let i = self
                        .members
                        .iter()
                        .position(|m| m == &self.default)
                        .unwrap_or(0);
                    self.default = self.members[(i + if k.code == K::Left {
                        self.members.len() - 1
                    } else {
                        1
                    }) % self.members.len()]
                    .clone();
                }
            }
            5 if matches!(k.code, K::Enter | K::Char(' ')) => self.advanced = !self.advanced,
            6 if self.advanced && self.automatic => self.url.key(k, false),
            7 if self.advanced && self.automatic => self.interval.key(k, false),
            8 if self.advanced && self.automatic => self.tolerance.key(k, false),
            i if self.advanced
                && i == save - 1
                && matches!(k.code, K::Left | K::Right | K::Char(' ') | K::Enter) =>
            {
                self.interrupt = !self.interrupt
            }
            _ => {}
        }
        Ok(None)
    }
}

pub(super) fn draw(f: &mut Frame, g: &GroupEditor, area: Rect, app: &App, inline: bool) {
    let b = Block::default().borders(Borders::ALL).title(if inline {
        " New Group · saved with rule "
    } else if g.new {
        " New Group "
    } else {
        " Edit Group "
    });
    let inner = b.inner(area);
    f.render_widget(b, area);
    let advanced_height = if g.advanced {
        if g.automatic {
            4
        } else {
            1
        }
    } else {
        0
    };
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(2),
            Constraint::Length(advanced_height),
            Constraint::Length(2),
        ])
        .split(inner);
    let accent = Color::Rgb(114, 216, 191);
    let line = |i: usize, text: String| {
        Line::styled(
            format!("{} {text}", if g.focus == i { "›" } else { " " }),
            Style::default().fg(if g.focus == i { accent } else { Color::Gray }),
        )
    };
    f.render_widget(
        Paragraph::new(vec![
            line(0, format!("Name     {}", model::clean(&g.name.value))),
            line(
                1,
                format!(
                    "Type     {}",
                    if g.automatic {
                        "Automatic (latency test)"
                    } else {
                        "Manual"
                    }
                ),
            ),
            line(2, format!("Search   {}", model::clean(&g.query.value))),
        ]),
        areas[0],
    );
    let items: Vec<_> = g
        .filtered()
        .iter()
        .map(|(tag, label)| {
            ListItem::new(format!(
                "[{}] {}",
                if g.members.contains(tag) { "x" } else { " " },
                model::clean(label)
            ))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(g.selected));
    f.render_stateful_widget(
        List::new(items).highlight_style(Style::default().fg(accent).add_modifier(
            if g.focus == 3 {
                ratatui::style::Modifier::REVERSED
            } else {
                ratatui::style::Modifier::empty()
            },
        )),
        areas[1],
        &mut state,
    );
    f.render_widget(
        Paragraph::new(vec![
            line(
                4,
                format!(
                    "{} selected · Default: {}",
                    g.members.len(),
                    if g.automatic {
                        "Automatic".into()
                    } else if g.default.is_empty() {
                        "First selected member".into()
                    } else {
                        app.label(&g.default)
                    }
                ),
            ),
            line(
                5,
                format!("[{} Advanced]", if g.advanced { "Hide" } else { "Show" }),
            ),
        ]),
        areas[2],
    );
    if g.advanced {
        let mut lines = vec![];
        if g.automatic {
            lines.extend([
                line(6, format!("Test URL   {}", model::clean(&g.url.value))),
                line(7, format!("Interval   {}", model::clean(&g.interval.value))),
                line(
                    8,
                    format!("Tolerance  {} ms", model::clean(&g.tolerance.value)),
                ),
            ]);
        }
        lines.push(line(
            g.save_focus() - 1,
            format!("Interrupt existing connections  {}", g.interrupt),
        ));
        f.render_widget(Paragraph::new(lines), areas[3]);
    }
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                ratatui::text::Span::styled(
                    if inline {
                        "[Use Group] "
                    } else if g.new {
                        "[Create Group] "
                    } else {
                        "[Save Group] "
                    },
                    Style::default().fg(if g.focus == g.save_focus() {
                        accent
                    } else {
                        Color::Gray
                    }),
                ),
                ratatui::text::Span::styled(
                    "[Cancel]",
                    Style::default().fg(if g.focus == g.save_focus() + 1 {
                        accent
                    } else {
                        Color::Gray
                    }),
                ),
            ]),
            Line::raw("Tab Focus · Space Select · Enter Action · Esc Back"),
        ]),
        areas[4],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    fn editor(a: &App) -> GroupEditor {
        GroupEditor::new(
            native::read(&a.snapshot.store, "/outbounds/-".into()).unwrap(),
            a,
        )
        .unwrap()
    }
    fn key(g: &mut GroupEditor, code: K) -> Option<native::GroupChange> {
        g.key(KeyEvent::new(code, M::NONE)).unwrap()
    }
    #[test]
    fn one_worksheet_submits_members_and_default_once() {
        let a = App::new(sample().unwrap(), true);
        let mut g = editor(&a);
        let original = a.doc();
        for c in "Media123".chars() {
            assert!(key(&mut g, K::Char(c)).is_none());
        }
        for _ in 0..3 {
            assert!(key(&mut g, K::Tab).is_none());
        }
        assert_eq!(g.focus, 3);
        key(&mut g, K::Char(' '));
        key(&mut g, K::Down);
        key(&mut g, K::Char(' '));
        assert_eq!(g.members.len(), 2);
        key(&mut g, K::Tab);
        key(&mut g, K::Right);
        assert_eq!(g.default, g.members[1]);
        key(&mut g, K::Tab);
        key(&mut g, K::Tab);
        let change = key(&mut g, K::Enter).unwrap();
        assert_eq!(change.name, "Media123");
        assert_eq!(change.value["outbounds"], json!(g.members));
        assert_eq!(change.value["default"], g.default);
        assert_eq!(a.doc(), original);
    }
    #[test]
    fn removing_default_reselects_a_member_and_empty_edit_is_dirty() {
        let a = App::new(sample().unwrap(), true);
        let mut g = GroupEditor::new(
            native::read(&a.snapshot.store, "/outbounds/0".into()).unwrap(),
            &a,
        )
        .unwrap();
        assert!(!g.dirty());
        let chosen = g.members[0].clone();
        g.default = chosen.clone();
        g.query = Input::new(chosen);
        g.focus = 3;
        key(&mut g, K::Char(' '));
        assert!(g.members.contains(&g.default));
        g.members.clear();
        g.default.clear();
        assert!(g.dirty());
        assert!(g.change().is_err());
    }
    #[test]
    fn automatic_options_and_unknown_fields_roundtrip() {
        let a = App::new(sample().unwrap(), true);
        let mut e = native::read(&a.snapshot.store, "/outbounds/0".into()).unwrap();
        e.value["future_setting"] = json!({"preserve":true});
        let mut g = GroupEditor::new(e, &a).unwrap();
        g.focus = 1;
        key(&mut g, K::Right);
        assert!(g.automatic && g.advanced);
        g.tolerance = Input::new("25".into());
        let c = g.change().unwrap();
        assert_eq!(c.value["type"], "urltest");
        assert_eq!(c.value["tolerance"], 25);
        assert!(c.value.get("default").is_none());
        assert_eq!(c.value["future_setting"]["preserve"], true);
        g.tolerance = Input::new("invalid".into());
        assert!(g.change().is_err());
        key(&mut g, K::Left);
        assert_eq!(g.save_focus(), 7);
        assert_eq!(g.change().unwrap().value["type"], "selector");
    }
    #[test]
    fn group_search_includes_subscription_source() {
        let mut a = App::new(sample().unwrap(), true);
        a.snapshot.store.subscriptions.push(
            serde_json::from_value(json!({
            "id":"demo","name":"Travel Plan","source":"private", "format":"uri", "updated_at":0,"user_agent":""
            }))
            .unwrap(),
        );
        let mut g = editor(&a);
        g.query = Input::new("travel".into());
        assert_eq!(g.filtered().len(), 2);
        assert!(g
            .filtered()
            .iter()
            .all(|(_, label)| label.contains("Travel Plan")));
    }
    #[test]
    fn large_unicode_group_lists_keep_actions_visible() {
        let mut a = App::new(sample().unwrap(), true);
        for i in 0..240 {
            a.snapshot.store.native.as_mut().unwrap()["outbounds"]
                .as_array_mut()
                .unwrap()
                .push(json!({"tag":format!("node-{i}"),"type":"direct"}));
            a.snapshot.store.display_names.insert(
                format!("node-{i}"),
                format!("日本 🇯🇵 {i} {}", "Long name ".repeat(10)),
            );
        }
        let mut g = editor(&a);
        g.name = Input::new("媒体 🎬".into());
        g.automatic = true;
        g.advanced = true;
        g.focus = 3;
        g.selected = g.choices.len() - 1;
        for (w, h) in [(54, 18), (80, 24), (140, 40)] {
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            t.draw(|f| draw(f, &g, Rect::new(0, 0, w, h), &a, true))
                .unwrap();
            let text = t
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            assert!(
                text.contains("[Use Group]") && text.contains("[Cancel]"),
                "{w}x{h}"
            );
            assert!(text.contains("239"));
            assert!(text.contains("Test URL"));
        }
    }
}
