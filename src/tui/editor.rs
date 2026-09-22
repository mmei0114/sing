//! The one object editor. DNS servers, rules, inbounds, nodes, log settings…
//! all open here and speak the same keys: ↑↓ field, Enter change, ⌫ clear,
//! a all fields, e JSON, ^S save, Esc cancel.
use super::{
    input::Input,
    labels,
    modal::{self, any, Choice, Confirm, Modal, Outcome, Picker, TextArea},
    schema::{self, Field, Kind, Ns, Object},
    text, theme, App,
};
use crate::{
    native::{self, Edit, GroupChange},
    runtime::Action,
};
use crossterm::event::{KeyCode as K, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};
use serde_json::{json, Value};

pub type LocalSave = Box<dyn FnMut(&mut App, Value) -> Result<(), String>>;
pub enum Target {
    Native {
        pointer: String,
        revision: String,
    },
    Group {
        revision: String,
        original: Option<String>,
    },
    Local(LocalSave),
}

#[derive(Clone)]
enum Row {
    Name,
    Field(Field),
    Extra(String),
    More(usize),
}

pub struct Editor {
    pub obj: Object,
    pub value: Value,
    original: Value,
    target: Target,
    title: String,
    selected: usize,
    show_all: bool,
    editing: Option<Input>,
    name: Option<Input>,
    original_name: String,
    pub error: String,
}

fn is_set(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
        _ => true,
    }
}
fn secret_key(key: &str) -> bool {
    [
        "password",
        "uuid",
        "private_key",
        "secret",
        "auth_key",
        "pre_shared_key",
        "private_key_passphrase",
    ]
    .contains(&key)
}

impl Editor {
    pub fn new(obj: Object, value: Value, target: Target, title: impl Into<String>) -> Self {
        let value = if value.is_null() { json!({}) } else { value };
        Self {
            obj,
            original: value.clone(),
            value,
            target,
            title: title.into(),
            selected: 0,
            show_all: false,
            editing: None,
            name: None,
            original_name: String::new(),
            error: String::new(),
        }
    }
    /// Groups and other labelled objects carry a display name outside the
    /// native document.
    pub fn with_name(mut self, name: &str) -> Self {
        self.name = Some(Input::new(name.into()));
        self.original_name = name.into();
        self
    }
    fn dirty(&self) -> bool {
        self.value != self.original
            || self
                .name
                .as_ref()
                .is_some_and(|n| n.value != self.original_name)
    }
    fn rows(&self) -> Vec<Row> {
        let mut rows = vec![];
        if self.name.is_some() {
            rows.push(Row::Name);
        }
        let types = self.obj.types();
        let mut known: Vec<&str> = vec![];
        if !types.is_empty() {
            rows.push(Row::Field(Field {
                key: "type",
                label: "Type",
                kind: Kind::Enum(types),
                common: true,
                help: "",
            }));
            known.push("type");
        }
        let group = labels::is_group(&self.value) && self.name.is_some();
        if self.obj.tagged() {
            known.push("tag");
            if !group || self.show_all {
                rows.push(Row::Field(Field {
                    key: "tag",
                    label: "Tag",
                    kind: Kind::Text,
                    common: true,
                    help: "Stable name other objects use to refer to this one.",
                }));
            }
        }
        if matches!(
            self.obj,
            Object::RouteRule | Object::DnsRule | Object::HeadlessRule
        ) && self.value["type"] == "logical"
        {
            known.push("type");
        }
        let mut hidden = 0;
        for field in schema::fields(self.obj, &self.value) {
            known.push(field.key);
            if field.common || self.show_all || is_set(&self.value[field.key]) {
                rows.push(Row::Field(field));
            } else {
                hidden += 1;
            }
        }
        if let Some(map) = self.value.as_object() {
            for key in map.keys() {
                if !known.contains(&key.as_str()) {
                    rows.push(Row::Extra(key.clone()));
                }
            }
        }
        if hidden > 0 {
            rows.push(Row::More(hidden));
        }
        rows
    }
    fn set(&mut self, key: &str, v: Value) {
        let map = self.value.as_object_mut().expect("object");
        if is_set(&v) || matches!(v, Value::Bool(_)) {
            map.insert(key.into(), v);
        } else {
            map.remove(key);
        }
        self.error.clear();
    }
    fn summary(&self, app: &App, key: &str, kind: &Kind) -> (String, ratatui::style::Color) {
        let v = &self.value[key];
        if !is_set(v) && !matches!(v, Value::Bool(_)) {
            return ("—".into(), theme::faint());
        }
        match kind {
            Kind::Secret => ("••••••••".into(), theme::dim()),
            Kind::Bool => {
                if v == true {
                    ("on".into(), theme::good())
                } else {
                    ("off".into(), theme::dim())
                }
            }
            Kind::Ref(Ns::Outbound) => {
                let t = v.as_str().unwrap_or("");
                (app.label(t), theme::target(t))
            }
            Kind::Ref(_) => (app.label(v.as_str().unwrap_or("")), theme::text()),
            Kind::Refs(_) | Kind::List { .. } => {
                let items: Vec<String> = match v {
                    Value::Array(a) => a
                        .iter()
                        .map(|x| {
                            let s = x
                                .as_str()
                                .map(str::to_string)
                                .unwrap_or_else(|| x.to_string());
                            if matches!(kind, Kind::Refs(_)) {
                                app.label(&s)
                            } else {
                                s
                            }
                        })
                        .collect(),
                    other => vec![other
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| other.to_string())],
                };
                let text = match items.len() {
                    0 => "—".into(),
                    1..=3 => items.join(", "),
                    n => format!("{} +{}", items[..2].join(", "), n - 2),
                };
                (text, theme::text())
            }
            Kind::NestedList(obj) => {
                let items = native::array(v, "");
                let first = items
                    .first()
                    .map(|i| labels::matcher(&app.snap.store, i))
                    .unwrap_or_default();
                (
                    match items.len() {
                        0 => "—".into(),
                        1 => first,
                        n => format!(
                            "{n} {} · {first} …",
                            if *obj == Object::HeadlessRule {
                                "conditions"
                            } else {
                                "items"
                            }
                        ),
                    },
                    theme::text(),
                )
            }
            Kind::Nested(_) => {
                let n = v.as_object().map_or(0, |o| o.len());
                let enabled = v["enabled"] == true;
                (
                    format!(
                        "{n} field{}{}",
                        if n == 1 { "" } else { "s" },
                        if enabled { " · enabled" } else { "" }
                    ),
                    theme::text(),
                )
            }
            _ => {
                let s = v
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| v.to_string());
                (crate::model::clean(&s), theme::text())
            }
        }
    }

    fn choices(app: &App, ns: Ns) -> Vec<Choice> {
        let doc = app.doc();
        match ns {
            Ns::Outbound => labels::targets(&app.snap.store, doc, false),
            Ns::Dns => native::array(doc, "/dns/servers")
                .iter()
                .map(|s| {
                    let tag = native::tag(s);
                    Choice::new(
                        tag,
                        app.label(tag),
                        format!(
                            "{} {}",
                            s["type"].as_str().unwrap_or(""),
                            s["server"].as_str().unwrap_or("")
                        ),
                    )
                })
                .collect(),
            Ns::RuleSet => native::array(doc, "/route/rule_set")
                .iter()
                .map(|s| {
                    let tag = native::tag(s);
                    Choice::new(tag, app.label(tag), s["type"].as_str().unwrap_or(""))
                })
                .collect(),
            Ns::Inbound => native::array(doc, "/inbounds")
                .iter()
                .map(|s| {
                    let tag = native::tag(s);
                    Choice::new(tag, tag, s["type"].as_str().unwrap_or(""))
                })
                .collect(),
        }
    }

    fn activate(&mut self, app: &mut App, row: Row) -> Outcome {
        let (key, kind, label) = match &row {
            Row::Name => {
                self.editing = self.name.clone();
                return Outcome::Stay;
            }
            Row::More(_) => {
                self.show_all = true;
                return Outcome::Stay;
            }
            Row::Extra(k) => (k.clone(), Kind::Json, k.clone()),
            Row::Field(f) => (f.key.to_string(), f.kind.clone(), f.label.to_string()),
        };
        let current = self.value[&key].clone();
        let set_top = move |key: String| {
            move |app: &mut App, v: Value| {
                if let Some(ed) = app.top::<Editor>() {
                    ed.set(&key, v);
                }
            }
        };
        match kind {
            Kind::Text | Kind::Secret | Kind::Number | Kind::Duration => {
                let s = current.as_str().map(str::to_string).unwrap_or_else(|| {
                    if current.is_null() {
                        String::new()
                    } else {
                        current.to_string()
                    }
                });
                self.editing = Some(Input::new(s));
            }
            Kind::Bool => {
                let next = current != true;
                self.set(&key, json!(next));
            }
            Kind::Enum(options) => {
                let required = ["type", "action", "mode"].contains(&key.as_str());
                let mut choices: Vec<Choice> = vec![];
                if !required {
                    choices.push(Choice::new("", "— not set", "use the core default"));
                }
                choices.extend(options.iter().map(|o| Choice::new(*o, *o, "")));
                let apply = set_top(key.clone());
                let numeric = key == "ip_version";
                return Outcome::Push(Box::new(Picker::single(
                    &label,
                    choices,
                    current
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| current.to_string())
                        .as_str(),
                    Box::new(move |app, v| {
                        let v = v.into_iter().next().unwrap_or_default();
                        let value = if numeric {
                            v.parse::<u64>().map(|n| json!(n)).unwrap_or(json!(v))
                        } else {
                            json!(v)
                        };
                        apply(app, value)
                    }),
                )));
            }
            Kind::Ref(ns) => {
                let mut choices = vec![Choice::new("", "— none", "")];
                choices.extend(Self::choices(app, ns));
                let apply = set_top(key.clone());
                return Outcome::Push(Box::new(Picker::single(
                    &label,
                    choices,
                    current.as_str().unwrap_or(""),
                    Box::new(move |app, v| {
                        apply(app, json!(v.into_iter().next().unwrap_or_default()))
                    }),
                )));
            }
            Kind::Refs(ns) => {
                let chosen: Vec<String> = match &current {
                    Value::Array(a) => a
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect(),
                    Value::String(s) => vec![s.clone()],
                    _ => vec![],
                };
                let mut choices = Self::choices(app, ns);
                for c in &chosen {
                    if !choices.iter().any(|x| &x.value == c) {
                        choices.push(Choice::new(c.clone(), c.clone(), "missing"));
                    }
                }
                let apply = set_top(key.clone());
                return Outcome::Push(Box::new(Picker::multi(
                    &label,
                    choices,
                    chosen,
                    Box::new(move |app, v| apply(app, json!(v))),
                )));
            }
            Kind::List { numbers } => {
                let lines = match &current {
                    Value::Array(a) => a
                        .iter()
                        .map(|x| {
                            x.as_str()
                                .map(str::to_string)
                                .unwrap_or_else(|| x.to_string())
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                    Value::Null => String::new(),
                    other => other
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| other.to_string()),
                };
                let apply = set_top(key.clone());
                return Outcome::Push(Box::new(TextArea::new(
                    &format!("{label} · one per line"),
                    lines,
                    Box::new(move |app, s| {
                        let mut items = vec![];
                        for line in s.lines().map(str::trim).filter(|l| !l.is_empty()) {
                            if numbers {
                                match line.parse::<u64>() {
                                    Ok(n) => items.push(json!(n)),
                                    Err(_) => {
                                        return Err(format!("“{line}” is not a whole number"))
                                    }
                                }
                            } else {
                                items.push(json!(line));
                            }
                        }
                        apply(app, json!(items));
                        Ok(())
                    }),
                )));
            }
            Kind::Json => {
                let pretty = if current.is_null() {
                    String::new()
                } else {
                    serde_json::to_string_pretty(&current).unwrap_or_default()
                };
                let apply = set_top(key.clone());
                return Outcome::Push(Box::new(TextArea::new(
                    &label,
                    pretty,
                    Box::new(move |app, s| {
                        if s.trim().is_empty() {
                            apply(app, Value::Null);
                            return Ok(());
                        }
                        let v: Value =
                            serde_json::from_str(&s).map_err(|e| format!("Invalid JSON: {e}"))?;
                        apply(app, v);
                        Ok(())
                    }),
                )));
            }
            Kind::Nested(obj) => {
                let apply = set_top(key.clone());
                let mut apply = Some(apply);
                return Outcome::Push(Box::new(Editor::new(
                    obj,
                    current,
                    Target::Local(Box::new(move |app, v| {
                        if let Some(f) = apply.take() {
                            f(app, v);
                        }
                        Ok(())
                    })),
                    obj.name(),
                )));
            }
            Kind::NestedList(obj) => {
                let apply = set_top(key.clone());
                let mut apply = Some(apply);
                let items = native::array(&current, "").to_vec();
                return Outcome::Push(Box::new(ObjectList::new(
                    obj,
                    &label,
                    items,
                    Box::new(move |app, v| {
                        if let Some(f) = apply.take() {
                            f(app, json!(v));
                        }
                    }),
                )));
            }
        }
        Outcome::Stay
    }

    fn commit_edit(&mut self, row: &Row) {
        let Some(input) = self.editing.take() else {
            return;
        };
        match row {
            Row::Name => {
                self.name = Some(input);
            }
            Row::Field(f) => {
                let s = input.value.trim().to_string();
                let v = match f.kind {
                    Kind::Number if !s.is_empty() => match s.parse::<i64>() {
                        Ok(n) => json!(n),
                        Err(_) => {
                            self.error = format!("{} needs a whole number", f.label);
                            self.editing = Some(input);
                            return;
                        }
                    },
                    Kind::Secret => json!(input.value),
                    _ => json!(s),
                };
                self.set(f.key, v);
            }
            _ => {}
        }
    }

    pub fn save(&mut self, app: &mut App) -> Outcome {
        let value = self.value.clone();
        match &mut self.target {
            Target::Local(f) => match f(app, value) {
                Ok(()) => Outcome::Close,
                Err(e) => {
                    self.error = e;
                    Outcome::Stay
                }
            },
            Target::Native { pointer, revision } => {
                if self.obj.tagged() && native::tag(&value).is_empty() {
                    self.error = "Tag is required".into();
                    return Outcome::Stay;
                }
                let edit = Edit {
                    revision: revision.clone(),
                    pointer: pointer.clone(),
                    value,
                };
                app.request_busy(Action::WriteNative(edit), "Saving", Box::new(saved));
                Outcome::Stay
            }
            Target::Group { revision, original } => {
                let name = self
                    .name
                    .as_ref()
                    .map(|n| n.value.trim().to_string())
                    .unwrap_or_default();
                let change = GroupChange {
                    revision: revision.clone(),
                    original_tag: original.clone(),
                    name,
                    value,
                };
                app.request_busy(Action::WriteGroup(change), "Saving", Box::new(saved));
                Outcome::Stay
            }
        }
    }
}

fn saved(app: &mut App, r: crate::runtime::Reply) {
    if r.ok {
        if app.top::<Editor>().is_some() {
            app.modals.pop();
        }
        app.toast("Saved to draft · A applies");
    } else if let Some(ed) = app.top::<Editor>() {
        ed.error = runtime_clean(&r.message);
    } else {
        app.error(r.message);
    }
}
fn runtime_clean(s: &str) -> String {
    crate::model::clean(&s.replace('\n', " · "))
}

impl Modal for Editor {
    any!();
    fn key(&mut self, app: &mut App, k: KeyEvent) -> Outcome {
        let rows = self.rows();
        self.selected = self.selected.min(rows.len().saturating_sub(1));
        let row = rows.get(self.selected).cloned();
        if let Some(input) = &mut self.editing {
            match k.code {
                K::Enter => {
                    if let Some(row) = row {
                        self.commit_edit(&row);
                    }
                }
                K::Esc => self.editing = None,
                K::Tab | K::Down => {
                    if let Some(row) = row {
                        self.commit_edit(&row);
                    }
                    if self.editing.is_none() {
                        self.selected = (self.selected + 1).min(rows.len().saturating_sub(1));
                    }
                }
                K::BackTab | K::Up => {
                    if let Some(row) = row {
                        self.commit_edit(&row);
                    }
                    if self.editing.is_none() {
                        self.selected = self.selected.saturating_sub(1);
                    }
                }
                _ if modal::is_save(&k) => {
                    if let Some(row) = row {
                        self.commit_edit(&row);
                    }
                    if self.editing.is_none() {
                        return self.save(app);
                    }
                }
                _ => input.key(k, false),
            }
            return Outcome::Stay;
        }
        if modal::is_save(&k) {
            return self.save(app);
        }
        match k.code {
            K::Esc => {
                if self.dirty() {
                    return Outcome::Push(Box::new(Confirm::new(
                        "Discard changes?",
                        "Your edits to this object have not been saved.",
                        "Discard",
                        Box::new(|app| {
                            app.modals.pop();
                        }),
                    )));
                }
                return Outcome::Close;
            }
            K::Down | K::Char('j') | K::Tab => {
                self.selected = (self.selected + 1).min(rows.len().saturating_sub(1))
            }
            K::Up | K::Char('k') | K::BackTab => self.selected = self.selected.saturating_sub(1),
            K::PageDown => self.selected = (self.selected + 10).min(rows.len().saturating_sub(1)),
            K::PageUp => self.selected = self.selected.saturating_sub(10),
            K::Home => self.selected = 0,
            K::End => self.selected = rows.len().saturating_sub(1),
            K::Char('a') => self.show_all = !self.show_all,
            K::Enter | K::Char(' ') => {
                if let Some(row) = row {
                    return self.activate(app, row);
                }
            }
            K::Backspace | K::Delete => match row {
                Some(Row::Field(f)) if !["type", "action"].contains(&f.key) => {
                    self.set(f.key, Value::Null)
                }
                Some(Row::Extra(key)) => self.set(&key, Value::Null),
                _ => {}
            },
            K::Char('e') => {
                let pretty = serde_json::to_string_pretty(&self.value).unwrap_or_default();
                return Outcome::Push(Box::new(TextArea::new(
                    &format!("{} · JSON", self.title),
                    pretty,
                    Box::new(|app, s| {
                        let v: Value =
                            serde_json::from_str(&s).map_err(|e| format!("Invalid JSON: {e}"))?;
                        if !v.is_object() {
                            return Err("Expected a JSON object".into());
                        }
                        if let Some(ed) = app.top::<Editor>() {
                            ed.value = v;
                            ed.error.clear();
                        }
                        Ok(())
                    }),
                )));
            }
            _ => {}
        }
        Outcome::Stay
    }
    fn paste(&mut self, s: &str) {
        if let Some(input) = &mut self.editing {
            input.insert(s.trim_end_matches('\n'));
        }
    }
    fn draw(&self, f: &mut Frame, area: Rect, app: &App) {
        let r = modal::popup(
            area,
            96,
            (self.rows().len() as u16 + 7).max(10).min(area.height),
        );
        let path = match &self.target {
            Target::Native { pointer, .. } => pointer.trim_start_matches('/').replace('/', " › "),
            Target::Group { .. } => "outbounds".into(),
            Target::Local(_) => "nested".into(),
        };
        let path = if path.ends_with('-') {
            format!("{} (new)", path.trim_end_matches(" › -"))
        } else {
            path
        };
        let inner = modal::frame(f, r, &self.title, &path);
        let rows = self.rows();
        let selected = self.selected.min(rows.len().saturating_sub(1));
        let help = match rows.get(selected) {
            Some(Row::Field(field)) => {
                let mut h = field.help.to_string();
                if h.is_empty() {
                    h = field.key.to_string();
                } else {
                    h = format!("{} · {h}", field.key);
                }
                h
            }
            Some(Row::Extra(k)) => format!("{k} · Not described by sing; kept exactly as written."),
            Some(Row::Name) => "Display name shown in sing. The native tag stays the same.".into(),
            Some(Row::More(_)) => "Show every documented field.".into(),
            None => String::new(),
        };
        let used_by = if self.obj.tagged() {
            let tag = native::tag(&self.original);
            if tag.is_empty() {
                String::new()
            } else {
                let n = native::links::links(app.doc())
                    .iter()
                    .filter(|l| l.tag == tag)
                    .count();
                if n > 0 {
                    format!("Used in {n} place{}", if n == 1 { "" } else { "s" })
                } else {
                    String::new()
                }
            }
        } else {
            String::new()
        };
        let [list, _, foot] = Layout::vertical([
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(if self.error.is_empty() { 2 } else { 3 }),
        ])
        .areas(inner);
        let height = list.height as usize;
        let start = modal::scroll(selected, height, rows.len());
        let label_w = 28usize.min(list.width as usize / 2);
        let value_w = (list.width as usize).saturating_sub(label_w + 2);
        let mut lines = vec![];
        let mut previous_common = true;
        for (i, row) in rows.iter().enumerate().skip(start).take(height) {
            let on = i == selected;
            let (label, value, color, dim_label) = match row {
                Row::Name => {
                    let n = self
                        .name
                        .as_ref()
                        .map(|n| n.value.clone())
                        .unwrap_or_default();
                    (
                        "Name".to_string(),
                        if n.is_empty() { "—".into() } else { n },
                        theme::text(),
                        false,
                    )
                }
                Row::Field(field) => {
                    let (v, c) = self.summary(app, field.key, &field.kind);
                    let v = if matches!(field.kind, Kind::Text) && secret_key(field.key) && v != "—"
                    {
                        "••••••••".into()
                    } else {
                        v
                    };
                    let separator = previous_common && !field.common && self.show_all;
                    previous_common = field.common;
                    if separator && i > start {
                        // Visual hint that advanced fields follow.
                    }
                    (field.label.to_string(), v, c, !field.common)
                }
                Row::Extra(k) => {
                    let v = &self.value[k];
                    (
                        k.clone(),
                        crate::model::clean(&v.to_string()),
                        theme::violet(),
                        true,
                    )
                }
                Row::More(n) => (
                    format!("＋ {n} more fields"),
                    "a".into(),
                    theme::faint(),
                    true,
                ),
            };
            let editing_this = on && self.editing.is_some();
            let value_line = if editing_this {
                let secret = matches!(row, Row::Field(fd) if fd.kind == Kind::Secret);
                self.editing.as_ref().unwrap().line(value_w, true, secret)
            } else {
                Line::from(Span::styled(text::fit(&value, value_w), theme::s(color)))
            };
            let mut spans = vec![Span::styled(
                text::cell(&label, label_w),
                theme::s(if dim_label {
                    theme::dim()
                } else {
                    theme::text()
                }),
            )];
            spans.extend(value_line.spans);
            lines.push(modal::row(on, spans));
        }
        f.render_widget(Paragraph::new(lines), list);
        let mut foot_lines = vec![Line::from(vec![Span::styled(
            text::fit(
                &help,
                (foot.width as usize).saturating_sub(text::width(&used_by) + 2),
            ),
            theme::s(theme::dim()),
        )])];
        if !used_by.is_empty() {
            f.render_widget(
                Paragraph::new(Span::styled(used_by, theme::s(theme::faint()))).right_aligned(),
                Rect { height: 1, ..foot },
            );
        }
        if !self.error.is_empty() {
            foot_lines.push(Line::styled(
                format!("✕ {}", self.error),
                theme::s(theme::bad()),
            ));
        }
        foot_lines.push(Line::from(vec![Span::styled(
            if self.dirty() { "● unsaved" } else { "" },
            theme::s(theme::warn()),
        )]));
        f.render_widget(Paragraph::new(foot_lines).wrap(Wrap { trim: true }), foot);
    }
    fn hints(&self) -> Vec<(&'static str, &'static str)> {
        if self.editing.is_some() {
            return vec![("enter", "done"), ("esc", "undo"), ("^S", "save")];
        }
        vec![
            ("↑↓", "field"),
            ("enter", "change"),
            ("⌫", "clear"),
            (
                "a",
                if self.show_all {
                    "common fields"
                } else {
                    "all fields"
                },
            ),
            ("e", "JSON"),
            ("^S", "save"),
            ("esc", "cancel"),
        ]
    }
}

// ---- Nested object lists (rules inside rule sets and logical rules) -----------------
pub type ListDone = Box<dyn FnMut(&mut App, Vec<Value>)>;
pub struct ObjectList {
    obj: Object,
    title: String,
    items: Vec<Value>,
    original: Vec<Value>,
    selected: usize,
    on_done: ListDone,
}
impl ObjectList {
    pub fn new(obj: Object, title: &str, items: Vec<Value>, on_done: ListDone) -> Self {
        Self {
            obj,
            title: title.into(),
            original: items.clone(),
            items,
            selected: 0,
            on_done,
        }
    }
}
fn list_top(app: &mut App) -> Option<&mut ObjectList> {
    app.top::<ObjectList>()
}
impl Modal for ObjectList {
    any!();
    fn key(&mut self, app: &mut App, k: KeyEvent) -> Outcome {
        let n = self.items.len();
        if let Some(down) = reorder_direction(k) {
            if let Some(to) = adjacent(self.selected, n, down) {
                self.items.swap(self.selected, to);
                self.selected = to;
            }
            return Outcome::Stay;
        }
        if modal::is_save(&k) {
            (self.on_done)(app, self.items.clone());
            return Outcome::Close;
        }
        match k.code {
            K::Esc => {
                if self.items != self.original {
                    return Outcome::Push(Box::new(Confirm::new(
                        "Discard changes?",
                        "Changes to this list have not been kept.",
                        "Discard",
                        Box::new(|app| {
                            app.modals.pop();
                        }),
                    )));
                }
                return Outcome::Close;
            }
            K::Down | K::Char('j') => self.selected = (self.selected + 1).min(n.saturating_sub(1)),
            K::Up | K::Char('k') => self.selected = self.selected.saturating_sub(1),
            K::Char('x') | K::Delete if n > 0 => {
                self.items.remove(self.selected);
                self.selected = self.selected.min(self.items.len().saturating_sub(1));
            }
            K::Enter if n > 0 => {
                let i = self.selected;
                return Outcome::Push(Box::new(Editor::new(
                    self.obj,
                    self.items[i].clone(),
                    Target::Local(Box::new(move |app, v| {
                        if let Some(list) = list_top(app) {
                            list.items[i] = v;
                        }
                        Ok(())
                    })),
                    format!("{} {}", self.obj.name(), i + 1),
                )));
            }
            K::Char('n') => {
                let obj = self.obj;
                let choices: Vec<Choice> = schema::templates(obj)
                    .into_iter()
                    .map(|(name, detail, v)| Choice::new(v.to_string(), name, detail))
                    .collect();
                return Outcome::Push(Box::new(Picker::single(
                    &format!("New {}", obj.name()),
                    choices,
                    "",
                    Box::new(move |app, picked| {
                        let Some(v) = picked
                            .first()
                            .and_then(|s| serde_json::from_str::<Value>(s).ok())
                        else {
                            return;
                        };
                        app.push(Editor::new(
                            obj,
                            v,
                            Target::Local(Box::new(|app, v| {
                                if let Some(list) = list_top(app) {
                                    list.items.push(v);
                                    list.selected = list.items.len() - 1;
                                }
                                Ok(())
                            })),
                            format!("New {}", obj.name()),
                        ));
                    }),
                )));
            }
            K::Char('e') => {
                let pretty = serde_json::to_string_pretty(&self.items).unwrap_or_default();
                return Outcome::Push(Box::new(TextArea::new(
                    &format!("{} · JSON", self.title),
                    pretty,
                    Box::new(|app, s| {
                        let v: Vec<Value> = serde_json::from_str(&s)
                            .map_err(|e| format!("Invalid JSON list: {e}"))?;
                        if let Some(list) = list_top(app) {
                            list.items = v;
                            list.selected = 0;
                        }
                        Ok(())
                    }),
                )));
            }
            _ => {}
        }
        Outcome::Stay
    }
    fn draw(&self, f: &mut Frame, area: Rect, app: &App) {
        let r = modal::popup(area, 96, area.height);
        let inner = modal::frame(f, r, &self.title, &format!("{} items", self.items.len()));
        let height = inner.height as usize;
        let start = modal::scroll(self.selected, height, self.items.len());
        let mut lines: Vec<Line> = self
            .items
            .iter()
            .enumerate()
            .skip(start)
            .take(height)
            .map(|(i, v)| {
                modal::row(
                    i == self.selected,
                    vec![
                        Span::styled(format!("{:>3}  ", i + 1), theme::s(theme::faint())),
                        Span::styled(
                            text::fit(
                                &labels::matcher(&app.snap.store, v),
                                inner.width as usize - 6,
                            ),
                            theme::s(theme::text()),
                        ),
                    ],
                )
            })
            .collect();
        if lines.is_empty() {
            lines.push(Line::styled(
                "  Empty · n adds one",
                theme::s(theme::faint()),
            ));
        }
        f.render_widget(Paragraph::new(lines), inner);
    }
    fn hints(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("enter", "edit"),
            ("n", "new"),
            ("x", "remove"),
            ("Alt+↑↓", "reorder"),
            ("e", "JSON"),
            ("^S", "keep"),
            ("esc", "cancel"),
        ]
    }
}

// ---- Opening editors --------------------------------------------------------------------
/// Edit the native object at `pointer`, loading unredacted values first.
pub fn open(app: &mut App, pointer: String) {
    let obj = Object::for_pointer(&pointer);
    app.request(
        Action::ReadNative(pointer.clone()),
        Box::new(move |app, r| {
            let Some(edit) = r.edit.filter(|_| r.ok) else {
                return app.error(r.message);
            };
            if !edit.value.is_object() && !edit.value.is_null() {
                return app.error("This value is not an object; use JSON editing in Config → JSON");
            }
            let tag = native::tag(&edit.value).to_string();
            if obj == Object::Outbound && labels::is_group(&edit.value) {
                let name = app.label(&tag);
                let title = format!("Edit Group · {name}");
                app.push(
                    Editor::new(
                        obj,
                        edit.value,
                        Target::Group {
                            revision: edit.revision,
                            original: Some(tag),
                        },
                        title,
                    )
                    .with_name(&name),
                );
                return;
            }
            let title = if tag.is_empty() {
                format!("Edit {}", obj.name())
            } else {
                format!("Edit {} · {}", obj.name(), app.label(&tag))
            };
            app.push(Editor::new(
                obj,
                edit.value,
                Target::Native {
                    pointer: edit.pointer,
                    revision: edit.revision,
                },
                title,
            ));
        }),
    );
}

/// Create a new object in the list at `list` (e.g. "/dns/servers").
pub fn create(app: &mut App, list: &str, mut value: Value) {
    let obj = Object::for_pointer(&format!("{list}/-"));
    if obj.tagged() {
        let base = native::tag(&value).to_string();
        let base = if base.is_empty() {
            "new".to_string()
        } else {
            base
        };
        let taken: Vec<String> = native::links::definitions(app.doc())
            .into_iter()
            .map(|l| l.tag)
            .collect();
        let mut tag = base.clone();
        let mut n = 2;
        while taken.contains(&tag) {
            tag = format!("{base}-{n}");
            n += 1;
        }
        value["tag"] = json!(tag);
    }
    let list = list.to_string();
    app.request(
        Action::ReadNative(list.clone()),
        Box::new(move |app, r| {
            let Some(edit) = r.edit.filter(|_| r.ok) else {
                return app.error(r.message);
            };
            if obj == Object::Outbound && labels::is_group(&value) {
                app.push(
                    Editor::new(
                        obj,
                        value,
                        Target::Group {
                            revision: edit.revision,
                            original: None,
                        },
                        "New Group",
                    )
                    .with_name(""),
                );
                return;
            }
            app.push(Editor::new(
                obj,
                value,
                Target::Native {
                    pointer: format!("{list}/-"),
                    revision: edit.revision,
                },
                format!("New {}", obj.name()),
            ));
        }),
    );
}

/// Create an item at an explicit ordered-list position. The shared editor is
/// still the review surface; insertion happens atomically when it is saved.
pub fn create_at(app: &mut App, list: &'static str, index: usize, value: Value) {
    let obj = Object::for_pointer(&format!("{list}/-"));
    app.request(
        Action::ReadNative(list.into()),
        Box::new(move |app, r| {
            let Some(edit) = r.edit.filter(|_| r.ok) else {
                return app.error(r.message);
            };
            let revision = edit.revision;
            let mut items = edit.value.as_array().cloned().unwrap_or_default();
            let at = index.min(items.len());
            app.push(Editor::new(
                obj,
                value,
                Target::Local(Box::new(move |app, value| {
                    items.insert(at.min(items.len()), value);
                    let edit = Edit {
                        revision: revision.clone(),
                        pointer: list.into(),
                        value: json!(items),
                    };
                    app.request_busy(Action::WriteNative(edit), "Saving", Box::new(saved));
                    Ok(())
                })),
                format!("New {} · priority {}", obj.name(), at + 1),
            ));
        }),
    );
}

/// Template picker, then editor.
pub fn create_from_template(app: &mut App, list: &'static str) {
    let obj = Object::for_pointer(&format!("{list}/-"));
    let choices: Vec<Choice> = schema::templates(obj)
        .into_iter()
        .map(|(name, detail, v)| Choice::new(v.to_string(), name, detail))
        .collect();
    app.push(Picker::single(
        &format!("New {}", obj.name()),
        choices,
        "",
        Box::new(move |app, picked| {
            if let Some(v) = picked
                .first()
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
            {
                create(app, list, v);
            }
        }),
    ));
}

/// Remove item `index` from the native list after confirming.
pub fn remove(app: &mut App, list: &'static str, index: usize, what: String) {
    app.push(
        Confirm::new(
            "Remove?",
            format!("Remove {what} from the draft. The running core is unchanged until you apply."),
            "Remove",
            Box::new(move |app| {
                app.request(
                    Action::ReadNative(list.into()),
                    Box::new(move |app, r| {
                        let Some(mut edit) = r.edit.filter(|_| r.ok) else {
                            return app.error(r.message);
                        };
                        let Some(items) = edit.value.as_array_mut() else {
                            return;
                        };
                        if index >= items.len() {
                            return app.error("That item no longer exists");
                        }
                        items.remove(index);
                        app.request(
                            Action::WriteNative(edit),
                            Box::new(|app, r| {
                                if r.ok {
                                    app.toast("Removed from draft · A applies")
                                } else {
                                    app.error(r.message)
                                }
                            }),
                        );
                    }),
                );
            }),
        )
        .danger(),
    );
}

/// The arrow event includes Alt/Option, so no held-key or release tracking is needed.
pub fn reorder_direction(k: KeyEvent) -> Option<bool> {
    if k.modifiers != KeyModifiers::ALT || k.kind == KeyEventKind::Release {
        return None;
    }
    match k.code {
        K::Down => Some(true),
        K::Up => Some(false),
        _ => None,
    }
}

fn adjacent(index: usize, len: usize, down: bool) -> Option<usize> {
    if index >= len {
        return None;
    }
    let to = if down {
        index.checked_add(1)?
    } else {
        index.checked_sub(1)?
    };
    (to < len).then_some(to)
}

pub type Moved = Box<dyn FnOnce(&mut App, usize)>;
/// Save one swap atomically; advance selection only after the manager accepts it.
pub fn shift(app: &mut App, list: &'static str, index: usize, down: bool, moved: Moved) {
    let observed = native::array(app.doc(), list).to_vec();
    let Some(to) = adjacent(index, observed.len(), down) else {
        return;
    };
    if app.busy.is_some() || app.moving_rule {
        return;
    }
    // Lock immediately, including the interval before the read/write jobs are dispatched.
    app.moving_rule = true;
    app.busy = Some(("Moving rule".into(), std::time::Instant::now()));
    app.request_busy(
        Action::ReadNative(list.into()),
        "Moving rule",
        Box::new(move |app, r| {
            app.moving_rule = false;
            let Some(mut edit) = r.edit.filter(|_| r.ok) else {
                return app.error(r.message);
            };
            let Some(items) = edit.value.as_array_mut() else {
                return app.error("The rule list is unavailable. Reopen it before moving rules.");
            };
            // Read unredacted values, but compare using the same redaction as the UI snapshot.
            if crate::config::redacted(&json!(items)) != json!(observed) {
                return app.error("Rules changed. Select the rule again before moving it.");
            }
            items.swap(index, to);
            app.moving_rule = true;
            app.busy = Some(("Moving rule".into(), std::time::Instant::now()));
            app.request_busy(
                Action::WriteNative(edit),
                "Moving rule",
                Box::new(move |app, r| {
                    app.moving_rule = false;
                    if r.ok {
                        moved(app, to);
                    } else {
                        app.error(r.message);
                    }
                }),
            );
        }),
    );
}
