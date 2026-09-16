//! Native-object workspace; English UI, independent of names in user data.
mod forms;
mod group;
mod links;
mod navigation;
mod observations;
mod rule_import;
mod subscriptions;
mod view;
#[cfg(test)]
mod visual_tests;
use crate::{
    api, config, model,
    native::{self, Edit},
    runtime::{self, Action, Reply, Snapshot},
};
use anyhow::{ensure, Context, Result};
use crossterm::{
    event::{self, Event, KeyCode as K, KeyEvent, KeyModifiers as M},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use forms::*;
use navigation::{Command, Focus};
use ratatui::{
    backend::{CrosstermBackend, TestBackend},
    Terminal,
};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    io::{self, IsTerminal},
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};
const PAGES: [&str; 5] = ["Overview", "Proxies", "Routing", "Network", "Activity"];
fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        _ => v.to_string(),
    }
}
fn names(d: &Value, p: &str) -> Vec<String> {
    native::array(d, p)
        .iter()
        .filter_map(|v| v["tag"].as_str().map(str::to_string))
        .collect()
}
fn title(v: &Value) -> String {
    if !native::tag(v).is_empty() {
        return format!("{} · {}", native::tag(v), text(&v["type"]));
    }
    let predicates = v
        .as_object()
        .map(|m| {
            m.iter()
                .filter(|(k, _)| !["action", "outbound", "server"].contains(&k.as_str()))
                .map(|(k, v)| format!("{k}={}", text(v)))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    format!(
        "{} → {} {}",
        if predicates.is_empty() {
            "All traffic"
        } else {
            &predicates
        },
        text(&v["action"]),
        v.get("outbound")
            .or_else(|| v.get("server"))
            .map(text)
            .unwrap_or_default()
    )
}
#[derive(Clone)]
enum Intent {
    Raw,
    Form,
    Group,
    RuleImport,
    RuleContext(Box<rule_import::RuleImport>),
    Setup,
    Add(Value),
    Delete(usize, Value),
    Move(usize, isize),
}
#[derive(Clone)]
enum Dialog {
    ApplyReview {
        summary: String,
        diff: String,
        expanded: bool,
        focus: usize,
        scroll: u16,
        action: Action,
    },
    References(links::Browser),
    Failure {
        editor: Option<Box<Dialog>>,
        message: String,
        scroll: u16,
    },
    Subscription(Box<subscriptions::SubscriptionReview>),
    Setup(Box<subscriptions::Setup>),
    SetupReview {
        parent: Box<subscriptions::Setup>,
        body: String,
        action: Action,
        scroll: u16,
    },
    Group(Box<group::GroupEditor>),
    RuleImport(Box<rule_import::RuleImport>),
    InlineGroup {
        editor: Box<group::GroupEditor>,
        parent: Box<rule_import::RuleImport>,
    },
    RuleReport {
        parent: Box<rule_import::RuleImport>,
        scroll: u16,
    },
    Discard {
        editor: Box<Dialog>,
        after: Option<Box<Dialog>>,
    },
    Commands {
        choices: Vec<navigation::PaletteItem>,
        query: Input,
        selected: usize,
    },
    Text {
        title: String,
        text: String,
        scroll: u16,
        action: Option<Action>,
    },
    Form(Form),
    Json {
        edit: Edit,
        input: Input,
    },
    Add {
        choices: Vec<(String, Value)>,
        selected: usize,
    },
    Members {
        form: Box<Form>,
        choices: Vec<String>,
        chosen: HashSet<String>,
        selected: usize,
    },
    Select {
        group: String,
        choices: Vec<String>,
        selected: usize,
    },
    Auth {
        kind: String,
        after: Action,
    },
}
struct App {
    history: Vec<links::Location>,
    snapshot: Snapshot,
    page: usize,
    focus: Focus,
    control: usize,
    selected: [usize; 12],
    tabs: [usize; 12],
    locations: HashMap<(usize, usize), (usize, String)>,
    filter: Input,
    searching: bool,
    dialog: Option<Dialog>,
    intent: Option<Intent>,
    retry: Option<Dialog>,
    notice: String,
    error: bool,
    busy: bool,
    connections: runtime::ConnectionReport,
    pending_connections: Option<runtime::ConnectionReport>,
    connections_paused: bool,
    connection_error: String,
    snapshot_at: u64,
    probe_stale: bool,
    refresh_requested: bool,
    show_closed: bool,
    quit: bool,
    demo: bool,
}
impl App {
    fn new(snapshot: Snapshot, demo: bool) -> Self {
        Self {
            history: vec![],
            snapshot,
            page: 0,
            focus: Focus::Content,
            control: 0,
            selected: [0; 12],
            tabs: [0; 12],
            locations: HashMap::new(),
            filter: Input::new(String::new()),
            searching: false,
            dialog: None,
            intent: None,
            retry: None,
            notice: String::new(),
            error: false,
            busy: false,
            connections: Default::default(),
            pending_connections: None,
            connections_paused: false,
            connection_error: String::new(),
            snapshot_at: model::now(),
            probe_stale: false,
            refresh_requested: true,
            show_closed: false,
            quit: false,
            demo,
        }
    }
    fn doc(&self) -> Value {
        self.snapshot.store.native.clone().unwrap_or(json!({}))
    }
    fn label(&self, tag: &str) -> String {
        if let Some(resource) = self
            .snapshot
            .store
            .rule_resources
            .iter()
            .find(|r| r.tag() == tag)
        {
            return resource.name.clone();
        }
        if let Some(name) = self.snapshot.store.display_names.get(tag) {
            return name.clone();
        }
        if let Some(n) = self.snapshot.store.nodes.iter().find(|n| n.tag() == tag) {
            return n.name.clone();
        }
        if let Some(g) = self
            .snapshot
            .store
            .proxy_groups
            .iter()
            .find(|g| g.tag() == tag)
        {
            return g.name.clone();
        }
        tag.into()
    }
    fn path(&self) -> &str {
        match (self.page, self.tabs[self.page]) {
            (1, _) => "/inbounds",
            (2, _) => "/outbounds",
            (3, 0) => "/route/rules",
            (3, _) => "/route",
            (4, 0) => "/dns/servers",
            (4, 1) => "/dns/rules",
            (4, _) => "/dns",
            (5, 1) => "/route/rule_set",
            _ => "",
        }
    }
    fn rows(&self) -> Vec<(usize, String, Value)> {
        let doc = self.doc();
        let all:Vec<_>=match self.page{
        0=>native::array(&doc,"/outbounds").iter().enumerate().filter(|(_,v)|v["type"]=="selector"||v["type"]=="urltest").map(|(i,v)|(i,self.label(native::tag(v)),v.clone())).collect(),
        5 if self.tabs[5]==0=>self.snapshot.store.subscriptions.iter().enumerate().map(|(i,s)|(i,format!("{} · {}",s.name,s.format),json!({"name":s.name,"source":s.source,"updated_at":s.updated_at,"warnings":s.warnings}))).collect(),
        7=>self.connection_rows().iter().enumerate().map(|(i,c)|(i,format!("{} → {} {}",observations::destination(c),observations::path(self,c),if c.closed_at==0{""}else{"[closed]"}),serde_json::to_value(c).unwrap())).collect(),
        3 if self.tabs[3]==1=>vec![(0,"Routing options".into(),doc["route"].clone())],4 if self.tabs[4]==2=>vec![(0,"DNS options".into(),doc["dns"].clone())],
        6=>doc.as_object().map(|m|m.iter().filter(|(k,_)|!["inbounds","outbounds","route","dns"].contains(&k.as_str())).enumerate().map(|(i,(k,v))|(i,k.clone(),v.clone())).collect()).unwrap_or_default(),
        _=>native::array(&doc,self.path()).iter().enumerate().map(|(i,v)|{let label=if self.page==2{self.snapshot.store.nodes.iter().find(|n|n.tag()==native::tag(v)).map(|n|format!("{} · {} · {}",n.name,n.kind(),n.server())).unwrap_or_else(||format!("{} · {}",self.label(native::tag(v)),text(&v["type"])))}else if self.page==5&&self.tabs[5]==1&&!native::tag(v).is_empty(){format!("{} · {}",self.label(native::tag(v)),text(&v["type"]))}else{title(v)};let delay=if self.page==2{self.snapshot.groups.group.iter().flat_map(|g|g.items.iter()).filter(|m|m.tag==native::tag(v)&&m.delay>0).max_by_key(|m|m.time).map(|m|format!(" · {} ms",m.delay)).unwrap_or_default()}else{String::new()};(i,format!("{label}{delay}"),v.clone())}).collect()};
        let q = self.filter.value.to_lowercase();
        all.into_iter()
            .filter(|(_, _, v)| {
                if self.page != 2 {
                    return true;
                }
                let group = v["type"] == "selector" || v["type"] == "urltest";
                match self.tabs[2] {
                    0 => group,
                    1 => !group && v["type"] != "direct" && v["type"] != "block",
                    _ => true,
                }
            })
            .filter(|(_, l, v)| format!("{l} {v}").to_lowercase().contains(&q))
            .collect()
    }
    fn current(&self) -> Option<(usize, String, Value)> {
        self.rows().get(self.selected[self.page]).cloned()
    }
    fn open(&mut self, p: String, i: Intent) -> Action {
        self.intent = Some(i);
        Action::ReadNative(p)
    }
    fn note(&mut self, title: &str, text: String, action: Option<Action>) {
        self.dialog = Some(Dialog::Text {
            title: title.into(),
            text,
            scroll: 0,
            action,
        });
    }
    fn settings_form(&mut self, integration: bool, mode: bool) {
        let s = &self.snapshot.store.settings;
        let fields = if integration {
            vec![
                Field::new(
                    "mode",
                    "System proxy (port = off)",
                    &json!(if s.mode == "system" { "system" } else { "port" }),
                    Kind::Choice(if cfg!(target_os = "macos") {
                        vec!["port".into(), "system".into()]
                    } else {
                        vec!["port".into()]
                    }),
                ),
                Field::new("port", "Mixed inbound port", &json!(s.port), Kind::Number),
            ]
        } else if mode {
            vec![
                Field::new(
                    "route_mode",
                    "Traffic routing",
                    &json!(s.route_mode),
                    Kind::Choice(vec!["rule".into(), "global".into(), "direct".into()]),
                ),
                Field::new(
                    "global_target",
                    "Global outbound",
                    &json!(s.global_target),
                    Kind::Choice(names(&self.doc(), "/outbounds")),
                ),
                Field::new(
                    "bypass_lan",
                    "Override: private IPs direct",
                    &json!(s.bypass_lan),
                    Kind::Bool,
                ),
            ]
        } else {
            vec![
                Field::new(
                    "core",
                    "Core path (blank: auto)",
                    &json!(s.core),
                    Kind::String,
                ),
                Field::new(
                    "api_port",
                    "Management API port",
                    &json!(s.api_port),
                    Kind::Number,
                ),
            ]
        };
        self.dialog = Some(Dialog::Form(Form {
            title: if integration {
                "System integration"
            } else if mode {
                "Routing override · DNS unchanged"
            } else {
                "Client settings"
            }
            .into(),
            fields,
            selected: 0,
            action: FormAction::Settings(s.clone()),
        }));
    }
    fn import(&mut self, convert: bool) {
        let mut fields = vec![
            Field::new("name", "Name", &json!(""), Kind::String),
            Field::new(
                "source",
                "URL / pasted content / local file",
                &json!(""),
                Kind::String,
            ),
        ];
        if convert {
            fields.push(Field::new(
                "format",
                "Format",
                &json!("auto"),
                Kind::Choice(vec![
                    "auto".into(),
                    "qx".into(),
                    "clash".into(),
                    "domain".into(),
                    "ipcidr".into(),
                ]),
            ));
            fields.push(Field::new(
                "target",
                "Append routing rule to",
                &json!("proxy"),
                Kind::Choice(names(&self.doc(), "/outbounds")),
            ));
        } else {
            fields.push(Field::new(
                "user_agent",
                "User-Agent (optional)",
                &json!(""),
                Kind::String,
            ));
        }
        self.dialog = Some(Dialog::Form(Form {
            title: if convert {
                "Convert rule subscription"
            } else {
                "Add node subscription"
            }
            .into(),
            fields,
            selected: 1,
            action: if convert {
                FormAction::Convert
            } else {
                FormAction::Import { advanced: false }
            },
        }));
    }
    fn failed(&mut self, message: String) {
        let editor = self.retry.take();
        let mut message = runtime::redact_error(&message, &self.snapshot.store);
        let form = match &editor {
            Some(Dialog::Form(f)) => Some(f),
            Some(Dialog::Subscription(s)) => s.form.as_ref(),
            Some(Dialog::RuleImport(r)) => Some(&r.form),
            _ => None,
        };
        if let Some(form) = form {
            message = runtime::redact_sources(
                &message,
                form.fields
                    .iter()
                    .filter(|f| ["source", "user_agent"].contains(&f.key.as_str()))
                    .map(|f| f.input.value.as_str()),
            );
        }
        self.error = true;
        self.notice = model::clean_multiline(&message);
        // Poll failures must not repeatedly cover an editor. Submitted operations
        // retain their exact editor/preview; returning never resubmits a write.
        if editor.is_some() {
            self.dialog = Some(Dialog::Failure {
                editor: editor.map(Box::new),
                message: self.notice.clone(),
                scroll: 0,
            });
        }
    }
    fn receive(&mut self, r: Reply) -> Result<Option<Action>> {
        self.busy = false;
        if let Some(s) = r.snapshot {
            self.observe_snapshot(s);
        }
        if !r.ok {
            self.failed(r.message);
            return Ok(None);
        }
        let previous = self.retry.take();
        if matches!(&previous,Some(Dialog::Subscription(s)) if s.form.is_some() && s.focus==0)
            && r.preview.is_none()
        {
            self.notice = "Subscription saved. Set up connection or choose Done for Now.".into();
            self.error = false;
            self.intent = Some(Intent::Setup);
            return Ok(Some(Action::ConnectionSetupInfo));
        }
        self.error = false;
        if !r.message.is_empty() {
            self.notice = r.message;
        }
        if r.needs_auth {
            self.dialog = Some(Dialog::Auth {
                kind: r.auth_kind,
                after: r.after_auth.unwrap_or(Action::Connect),
            });
            return Ok(None);
        }
        if let Some(c) = r.connections {
            self.observe_connections(c);
        }
        if let Some(edit) = r.edit {
            match self.intent.take().unwrap_or(Intent::Raw) {
                Intent::Raw => {
                    self.dialog = Some(Dialog::Json {
                        input: Input::new(serde_json::to_string_pretty(&edit.value)?),
                        edit,
                    })
                }
                Intent::Group => {
                    self.dialog = Some(Dialog::Group(Box::new(group::GroupEditor::new(
                        edit, self,
                    )?)))
                }
                Intent::Setup => {
                    let mut setup = subscriptions::Setup::new(self)?;
                    setup.revision = edit.revision;
                    self.dialog = Some(Dialog::Setup(Box::new(setup)));
                }
                Intent::RuleImport => {
                    self.dialog = Some(Dialog::RuleImport(Box::new(rule_import::RuleImport::new(
                        edit, self,
                    ))))
                }
                Intent::RuleContext(previous) => {
                    let mut next = rule_import::RuleImport::new(edit, self);
                    next.form = previous.form.clone();
                    next.form.selected = 1;
                    next.pending_id = previous.pending_id.clone();
                    next.group = previous.group.clone();
                    if let Some(group) = &mut next.group {
                        group.revision = next.revision.clone();
                        next.choices
                            .push((native::tag(&group.value).into(), group.name.clone()));
                    }
                    if let Some((target, _)) = previous.choices.get(previous.target) {
                        if let Some(i) = next.choices.iter().position(|(tag, _)| tag == target) {
                            next.target = i;
                        }
                    }
                    if next.rules == previous.rules {
                        next.position = previous.position;
                    } else {
                        self.notice =
                            "Route order changed. Review the insertion position again.".into();
                    }
                    self.dialog = Some(Dialog::RuleImport(Box::new(next)));
                }
                Intent::Form => {
                    self.dialog = Some(
                        if edit.pointer.starts_with("/outbounds/")
                            && ["selector", "urltest"]
                                .contains(&edit.value["type"].as_str().unwrap_or(""))
                        {
                            Dialog::Group(Box::new(group::GroupEditor::new(edit, self)?))
                        } else {
                            Dialog::Form(object_form(edit, &self.doc())?)
                        },
                    );
                }
                Intent::Add(value) => {
                    let edit = Edit {
                        pointer: format!("{}/-", edit.pointer),
                        value,
                        revision: edit.revision,
                    };
                    self.dialog = Some(
                        if edit.pointer.starts_with("/outbounds/")
                            && ["selector", "urltest"]
                                .contains(&edit.value["type"].as_str().unwrap_or(""))
                        {
                            Dialog::Group(Box::new(group::GroupEditor::new(edit, self)?))
                        } else {
                            Dialog::Form(object_form(edit, &self.doc())?)
                        },
                    );
                }
                Intent::Delete(i, expected) => {
                    let mut e = edit;
                    let list = e.value.as_array_mut().context("Not a list")?;
                    ensure!(
                        list.get(i).is_some_and(|v| config::redacted(v) == expected),
                        "Selected object changed. Refresh and select it again before removing."
                    );
                    list.remove(i);
                    let before = self.doc();
                    let mut after = before.clone();
                    native::set(&mut after, &e.pointer, e.value.clone())?;
                    if let Err(error) = native::links::check_removals(&before, &after) {
                        self.reference_browser()?;
                        self.notice = error.to_string();
                        self.error = true;
                        return Ok(None);
                    }
                    self.note(
                        "Remove from draft?",
                        "Remove this object from the saved draft. Running configuration stays unchanged.".into(),
                        Some(Action::WriteNative(e)),
                    );
                }
                Intent::Move(i, delta) => {
                    let mut e = edit;
                    let list = e.value.as_array_mut().context("Not a list")?;
                    let j = i
                        .saturating_add_signed(delta)
                        .min(list.len().saturating_sub(1));
                    list.swap(i, j);
                    return Ok(Some(Action::WriteNative(e)));
                }
            }
        }
        if let Some(p) = r.preview {
            let form = match &previous {
                Some(Dialog::Form(f)) if matches!(f.action, FormAction::Import { .. }) => {
                    Some(f.clone())
                }
                Some(Dialog::Subscription(s)) => s.form.clone(),
                _ => None,
            };
            self.dialog = Some(Dialog::Subscription(Box::new(
                subscriptions::SubscriptionReview {
                    preview: p,
                    form,
                    focus: 0,
                    scroll: 0,
                },
            )));
        }
        if let Some(p) = r.rules_preview {
            if let Some(Dialog::RuleImport(mut import)) = previous {
                import.pending_id = Some(p.draft_id.clone());
                import.warnings_reviewed = p.warnings.is_empty();
                import.preview = Some(p);
                import.focus = 0;
                self.dialog = Some(Dialog::RuleImport(import));
                return Ok(None);
            }
            self.note(
                "Review conversion",
                format!(
                    "{} · {}\n{} / {} supported · +{} / -{}\nTarget: {}\n\n{}\n\n{}",
                    p.name,
                    p.format,
                    p.count,
                    p.input_count,
                    p.added,
                    p.removed,
                    p.target,
                    p.sample.join("\n"),
                    p.warnings.join("\n")
                ),
                Some(Action::CommitRules),
            );
        }
        if let Some(t) = r.config {
            if let Some(action @ Action::ApplyNative { .. }) = r.confirm.clone() {
                self.dialog = Some(Dialog::ApplyReview {
                    summary: t,
                    diff: r.diff.unwrap_or_else(|| {
                        "Native Diff unavailable; request Review Changes again.".into()
                    }),
                    expanded: false,
                    focus: 0,
                    scroll: 0,
                    action,
                });
                return Ok(None);
            }
            if let Some(Dialog::Setup(parent)) = previous {
                if let Some(action @ Action::SaveConnectionSetup(_)) = r.confirm {
                    self.dialog = Some(Dialog::SetupReview {
                        parent,
                        body: t,
                        action,
                        scroll: 0,
                    });
                    return Ok(None);
                }
            }
            self.note(
                if r.confirm.is_some() {
                    "Review"
                } else {
                    "Details"
                },
                t,
                r.confirm,
            );
        }
        Ok(None)
    }
    fn dialog_key(&mut self, k: KeyEvent, mut d: Dialog) -> Result<Option<Action>> {
        let group_cancel = matches!(&d, Dialog::Group(g)|Dialog::InlineGroup{editor:g,..} if k.code==K::Enter && g.focus==g.save_focus()+1);
        let subscription_cancel = matches!(&d,Dialog::Form(f) if matches!(f.action,FormAction::Import { .. }) && k.code==K::Enter && f.selected==f.submit_focus()+1);
        let rule_cancel = matches!(&d,Dialog::RuleImport(r) if k.code==K::Enter && if r.preview.is_some(){r.focus==6}else{r.form.selected==r.form.fields.len()+1});
        if k.code == K::Esc || group_cancel || rule_cancel || subscription_cancel {
            return Ok(match d {
                Dialog::Failure { editor, .. } => {
                    self.dialog = editor.map(|d| *d);
                    None
                }
                Dialog::Discard { editor, .. } => {
                    self.dialog = Some(*editor);
                    None
                }
                Dialog::Group(ref g) if g.dirty() => {
                    self.dialog = Some(Dialog::Discard {
                        editor: Box::new(d),
                        after: None,
                    });
                    None
                }
                Dialog::InlineGroup {
                    ref editor,
                    ref parent,
                } => {
                    self.dialog = Some(if editor.dirty() {
                        Dialog::Discard {
                            after: Some(Box::new(Dialog::RuleImport(parent.clone()))),
                            editor: Box::new(d),
                        }
                    } else {
                        Dialog::RuleImport(parent.clone())
                    });
                    None
                }
                Dialog::RuleImport(_) => {
                    self.dialog = Some(Dialog::Discard {
                        editor: Box::new(d),
                        after: None,
                    });
                    None
                }
                Dialog::RuleReport { parent, .. } => {
                    self.dialog = Some(Dialog::RuleImport(parent));
                    None
                }
                Dialog::Subscription(_) | Dialog::Setup(_) => {
                    self.dialog = Some(Dialog::Discard {
                        editor: Box::new(d),
                        after: None,
                    });
                    None
                }
                Dialog::SetupReview { parent, .. } => {
                    self.dialog = Some(Dialog::Setup(parent));
                    None
                }
                Dialog::Form(ref f)
                    if matches!(f.action, FormAction::Import { .. })
                        && f.fields.iter().any(|f| !f.input.value.is_empty()) =>
                {
                    self.dialog = Some(Dialog::Discard {
                        editor: Box::new(d),
                        after: None,
                    });
                    None
                }
                Dialog::Members { form, .. } => {
                    self.dialog = Some(Dialog::Form(*form));
                    None
                }
                Dialog::Text {
                    action: Some(Action::CommitImport),
                    ..
                } => Some(Action::CancelImport),
                Dialog::Text {
                    action: Some(Action::CommitRules),
                    ..
                } => Some(Action::CancelRules),
                _ => None,
            });
        }
        let save =
            k.code == K::F(2) || (k.code == K::Char('s') && k.modifiers.contains(M::CONTROL));
        match &mut d {
            Dialog::ApplyReview {
                expanded,
                focus,
                scroll,
                action,
                ..
            } => match k.code {
                K::Tab | K::Right => *focus = (*focus + 1) % 3,
                K::BackTab | K::Left => *focus = (*focus + 2) % 3,
                K::Enter if *focus == 2 => return Ok(None),
                K::Enter if *focus == 1 => {
                    *expanded = !*expanded;
                    *scroll = 0;
                }
                K::Enter => {
                    let next = action.clone();
                    self.retry = Some(d);
                    return Ok(Some(next));
                }
                K::Down => *scroll = scroll.saturating_add(1),
                K::Up => *scroll = scroll.saturating_sub(1),
                K::PageDown => *scroll = scroll.saturating_add(8),
                K::PageUp => *scroll = scroll.saturating_sub(8),
                _ => {}
            },
            Dialog::References(browser) => match k.code {
                K::Tab | K::Down => {
                    browser.selected =
                        (browser.selected + 1).min(browser.items.len().saturating_sub(1))
                }
                K::BackTab | K::Up => browser.selected = browser.selected.saturating_sub(1),
                K::Enter => match self.follow_reference(browser.clone()) {
                    Ok(action) if !browser.items.is_empty() => return Ok(action),
                    Ok(_) => {}
                    Err(e) => {
                        self.error = true;
                        self.notice = e.to_string();
                    }
                },
                _ => {}
            },
            Dialog::Failure { editor, scroll, .. } => match k.code {
                K::Enter => {
                    self.dialog = editor.take().map(|d| *d);
                    return Ok(None);
                }
                K::Down => *scroll = scroll.saturating_add(1),
                K::Up => *scroll = scroll.saturating_sub(1),
                K::PageDown => *scroll = scroll.saturating_add(8),
                K::PageUp => *scroll = scroll.saturating_sub(8),
                _ => {}
            },
            Dialog::SetupReview { action, scroll, .. } => match k.code {
                K::Enter => {
                    let next = action.clone();
                    self.retry = Some(d);
                    return Ok(Some(next));
                }
                K::Down => *scroll = scroll.saturating_add(1),
                K::Up => *scroll = scroll.saturating_sub(1),
                K::PageDown => *scroll = scroll.saturating_add(8),
                K::PageUp => *scroll = scroll.saturating_sub(8),
                _ => {}
            },
            Dialog::Subscription(s) => {
                let n = s.buttons().len();
                match k.code {
                    K::Tab | K::Right => s.focus = (s.focus + 1) % n,
                    K::BackTab | K::Left => s.focus = (s.focus + n - 1) % n,
                    K::Down => s.scroll = s.scroll.saturating_add(1),
                    K::Up => s.scroll = s.scroll.saturating_sub(1),
                    K::PageDown => s.scroll = s.scroll.saturating_add(8),
                    K::PageUp => s.scroll = s.scroll.saturating_sub(8),
                    K::Enter if s.focus == n - 1 => {
                        self.dialog = Some(Dialog::Discard {
                            editor: Box::new(d),
                            after: None,
                        });
                        return Ok(None);
                    }
                    K::Enter if s.form.is_some() && s.focus == 2 => {
                        self.dialog = s.form.clone().map(Dialog::Form);
                        return Ok(Some(Action::CancelSubscriptions(s.preview.id.clone())));
                    }
                    K::Enter if s.focus == n - 2 => {
                        let action = Action::ReprepareSubscriptions(s.preview.id.clone());
                        self.retry = Some(d);
                        return Ok(Some(action));
                    }
                    K::Enter => {
                        let action = Action::CommitSubscriptions {
                            id: s.preview.id.clone(),
                            revision: s.preview.revision.clone(),
                        };
                        self.retry = Some(d);
                        return Ok(Some(action));
                    }
                    _ => {}
                }
            }
            Dialog::Setup(s) => match k.code {
                K::Tab | K::Down => s.focus = (s.focus + 1) % 7,
                K::BackTab | K::Up => s.focus = (s.focus + 6) % 7,
                K::Left | K::Right | K::Char(' ') => {
                    let (value, n) = match s.focus {
                        0 => (&mut s.target, s.targets.len()),
                        1 => (&mut s.mode, 3),
                        2 => (&mut s.capture, s.captures.len()),
                        _ => {
                            self.dialog = Some(d);
                            return Ok(None);
                        }
                    };
                    *value = (*value + if k.code == K::Left { n - 1 } else { 1 }) % n;
                }
                K::Enter if s.focus == 3 => {
                    let change = s.change();
                    self.retry = Some(d);
                    return Ok(Some(Action::ReviewConnectionSetup(change)));
                }
                K::Enter if (4..=6).contains(&s.focus) => {
                    let dirty = s.dirty(self);
                    let destination = match s.focus {
                        5 => Some(11),
                        6 => Some(10),
                        _ => None,
                    };
                    if let Some(page) = destination {
                        self.go(page, 0);
                    }
                    if dirty {
                        self.dialog = Some(Dialog::Discard {
                            editor: Box::new(d),
                            after: None,
                        });
                    }
                    return Ok(None);
                }
                _ => {}
            },
            Dialog::Discard { editor, after } => {
                if k.code == K::Enter {
                    self.dialog = after.take().map(|d| *d);
                    return Ok(match editor.as_ref() {
                        Dialog::RuleImport(r) => r.pending_id.clone().map(Action::CancelRuleDraft),
                        Dialog::Subscription(r) => {
                            Some(Action::CancelSubscriptions(r.preview.id.clone()))
                        }
                        _ => None,
                    });
                }
            }
            Dialog::RuleReport { scroll, .. } => match k.code {
                K::Down => *scroll = scroll.saturating_add(1),
                K::Up => *scroll = scroll.saturating_sub(1),
                K::PageDown => *scroll = scroll.saturating_add(8),
                K::PageUp => *scroll = scroll.saturating_sub(8),
                _ => {}
            },
            Dialog::InlineGroup { editor, parent } => match editor.key(k) {
                Ok(Some(change)) => {
                    let mut parent = parent.clone();
                    if let Some(old) = &parent.group {
                        parent
                            .choices
                            .retain(|(tag, _)| tag != native::tag(&old.value));
                    }
                    parent
                        .choices
                        .push((native::tag(&change.value).into(), change.name.clone()));
                    parent.target = parent.choices.len() - 1;
                    parent.group = Some(change);
                    self.dialog = Some(Dialog::RuleImport(parent));
                    return Ok(None);
                }
                Ok(None) => {}
                Err(e) => {
                    self.error = true;
                    self.notice = e.to_string();
                }
            },
            Dialog::RuleImport(import) => {
                if import.preview.is_none() {
                    if save
                        || (k.code == K::Enter && import.form.selected == import.form.fields.len())
                    {
                        let action = import.prepare();
                        self.retry = Some(d);
                        return Ok(Some(action));
                    }
                    let f = &mut import.form;
                    if matches!(k.code, K::Tab | K::Down) {
                        f.selected = (f.selected + 1) % (f.fields.len() + 2);
                    } else if matches!(k.code, K::BackTab | K::Up) {
                        f.selected = (f.selected + f.fields.len() + 1) % (f.fields.len() + 2);
                    } else if let Some(field) = f.fields.get_mut(f.selected) {
                        if let Kind::Choice(choices) = &field.kind {
                            if matches!(k.code, K::Left | K::Right | K::Char(' ')) {
                                let i = choices
                                    .iter()
                                    .position(|v| v == &field.input.value)
                                    .unwrap_or(0);
                                field.input = Input::new(
                                    choices[(i + if k.code == K::Left {
                                        choices.len() - 1
                                    } else {
                                        1
                                    }) % choices.len()]
                                    .clone(),
                                );
                            }
                        } else {
                            field.input.key(k, false);
                        }
                    }
                } else {
                    match k.code {
                        K::Tab | K::Down => import.focus = (import.focus + 1) % 7,
                        K::BackTab | K::Up => import.focus = (import.focus + 6) % 7,
                        K::Left | K::Right | K::Char(' ') => {
                            let forward = k.code != K::Left;
                            if import.focus == 0 && !import.choices.is_empty() {
                                let n = import.choices.len();
                                import.target =
                                    (import.target + if forward { 1 } else { n - 1 }) % n;
                            }
                            if import.focus == 1 {
                                import.position = if forward {
                                    (import.position + 1).min(import.rules.len())
                                } else {
                                    import.position.saturating_sub(1)
                                };
                            }
                        }
                        K::Enter if import.focus == 2 => {
                            let edit = Edit {
                                revision: import.revision.clone(),
                                pointer: "/outbounds/-".into(),
                                value: Value::Null,
                            };
                            self.dialog = Some(Dialog::InlineGroup {
                                editor: Box::new(group::GroupEditor::new(edit, self)?),
                                parent: import.clone(),
                            });
                            return Ok(None);
                        }
                        K::Enter if import.focus == 3 => {
                            import.warnings_reviewed = true;
                            self.dialog = Some(Dialog::RuleReport {
                                parent: import.clone(),
                                scroll: 0,
                            });
                            return Ok(None);
                        }
                        K::Enter if import.focus == 4 => {
                            self.intent = Some(Intent::RuleContext(import.clone()));
                            self.retry = Some(d);
                            return Ok(Some(Action::ReadNative("/route".into())));
                        }
                        K::Enter if import.focus == 5 => match import.commit() {
                            Ok(action) => {
                                self.retry = Some(d);
                                return Ok(Some(action));
                            }
                            Err(e) => {
                                self.error = true;
                                self.notice = e.to_string();
                                import.focus = 3;
                            }
                        },
                        _ => {}
                    }
                }
            }
            Dialog::Group(g) => match g.key(k) {
                Ok(Some(change)) => {
                    self.retry = Some(d);
                    return Ok(Some(Action::WriteGroup(change)));
                }
                Ok(None) => {}
                Err(error) => {
                    self.error = true;
                    self.notice = error.to_string();
                }
            },
            Dialog::Commands {
                choices,
                query,
                selected,
            } => {
                let filtered: Vec<_> = choices
                    .iter()
                    .filter(|item| {
                        item.label
                            .to_lowercase()
                            .contains(&query.value.to_lowercase())
                    })
                    .collect();
                match k.code {
                    K::Down => *selected = (*selected + 1).min(filtered.len().saturating_sub(1)),
                    K::Up => *selected = selected.saturating_sub(1),
                    K::Enter => {
                        if let Some(item) = filtered.get(*selected) {
                            if self.unavailable(item.command).is_none() {
                                return self.activate(item.command);
                            }
                        }
                    }
                    _ => {
                        query.key(k, false);
                        *selected = 0;
                    }
                }
            }
            Dialog::Text { scroll, action, .. } => match k.code {
                K::Enter if action.is_some() => {
                    let next = action.clone();
                    self.retry = Some(d);
                    return Ok(next);
                }
                K::Down | K::Char('j') => *scroll = scroll.saturating_add(1),
                K::Up | K::Char('k') => *scroll = scroll.saturating_sub(1),
                K::PageDown => *scroll = scroll.saturating_add(12),
                K::PageUp => *scroll = scroll.saturating_sub(12),
                _ => {}
            },
            Dialog::Json { edit, input } => {
                if save {
                    match serde_json::from_str(&input.value) {
                        Ok(v) => {
                            let mut e = edit.clone();
                            e.value = v;
                            self.retry = Some(d);
                            return Ok(Some(Action::WriteNative(e)));
                        }
                        Err(e) => {
                            self.notice = format!("Invalid JSON: {e}");
                            self.error = true;
                        }
                    }
                } else {
                    input.key(k, true);
                }
            }
            Dialog::Form(f) => {
                if k.code == K::Enter && f.selected == f.submit_focus() + 1 {
                    return Ok(None);
                }
                if matches!(k.code, K::Enter | K::Char(' ')) && f.toggle_advanced() {
                    self.dialog = Some(d);
                    return Ok(None);
                }
                if save || (k.code == K::Enter && f.selected == f.submit_focus()) {
                    match f.submit() {
                        Ok(action) => {
                            self.retry = Some(d);
                            return Ok(Some(action));
                        }
                        Err(e) => {
                            self.notice = e.to_string();
                            self.error = true;
                        }
                    }
                } else if k.code == K::Tab || k.code == K::Down {
                    f.selected = (f.selected + 1) % (f.submit_focus() + 2);
                } else if k.code == K::BackTab || k.code == K::Up {
                    f.selected = (f.selected + f.submit_focus() + 1) % (f.submit_focus() + 2);
                } else if f.selected < f.visible_fields() {
                    let field = &mut f.fields[f.selected];
                    match &field.kind {
                        Kind::Members(choices) if k.code == K::Char(' ') || k.code == K::Enter => {
                            let mut choices = choices.clone();
                            let existing = serde_json::from_str::<Vec<String>>(&field.input.value)
                                .unwrap_or_default();
                            for tag in &existing {
                                if !choices.contains(tag) {
                                    choices.push(tag.clone());
                                }
                            }
                            let chosen = existing.into_iter().collect();
                            self.dialog = Some(Dialog::Members {
                                form: Box::new(f.clone()),
                                choices,
                                chosen,
                                selected: 0,
                            });
                            return Ok(None);
                        }
                        Kind::Choice(choices)
                            if matches!(k.code, K::Left | K::Right | K::Char(' '))
                                && !choices.is_empty() =>
                        {
                            let i = choices
                                .iter()
                                .position(|s| s == &field.input.value)
                                .unwrap_or(0);
                            let n = if k.code == K::Left {
                                (i + choices.len() - 1) % choices.len()
                            } else {
                                (i + 1) % choices.len()
                            };
                            field.input = Input::new(choices[n].clone());
                        }
                        Kind::Bool if matches!(k.code, K::Left | K::Right | K::Char(' ')) => {
                            field.input = Input::new(
                                if field.input.value == "true" {
                                    "false"
                                } else {
                                    "true"
                                }
                                .into(),
                            )
                        }
                        _ => field.input.key(k, false),
                    }
                }
            }
            Dialog::Add { choices, selected } => match k.code {
                K::Down | K::Char('j') => *selected = (*selected + 1).min(choices.len() - 1),
                K::Up | K::Char('k') => *selected = selected.saturating_sub(1),
                K::Enter => {
                    return Ok(Some(self.open(
                        self.path().into(),
                        Intent::Add(choices[*selected].1.clone()),
                    )))
                }
                _ => {}
            },
            Dialog::Members {
                form,
                choices,
                chosen,
                selected,
            } => {
                if save {
                    let mut f = *form.clone();
                    let mut members =
                        serde_json::from_str::<Vec<String>>(&f.fields[f.selected].input.value)
                            .unwrap_or_default();
                    members.retain(|s| chosen.contains(s));
                    for tag in choices {
                        if chosen.contains(tag) && !members.contains(tag) {
                            members.push(tag.clone());
                        }
                    }
                    f.fields[f.selected].input = Input::new(json!(members).to_string());
                    self.dialog = Some(Dialog::Form(f));
                    return Ok(None);
                }
                match k.code {
                    K::Down | K::Char('j') => {
                        *selected = (*selected + 1).min(choices.len().saturating_sub(1))
                    }
                    K::Up | K::Char('k') => *selected = selected.saturating_sub(1),
                    K::Char(' ') => {
                        if let Some(s) = choices.get(*selected) {
                            if !chosen.remove(s) {
                                chosen.insert(s.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
            Dialog::Select {
                group,
                choices,
                selected,
            } => match k.code {
                K::Down | K::Char('j') => {
                    *selected = (*selected + 1).min(choices.len().saturating_sub(1))
                }
                K::Up | K::Char('k') => *selected = selected.saturating_sub(1),
                K::Enter => {
                    let next = choices.get(*selected).map(|s| Action::SelectNative {
                        group: group.clone(),
                        member: s.clone(),
                    });
                    self.retry = Some(d);
                    return Ok(next);
                }
                _ => {}
            },
            Dialog::Auth { .. } => {}
        }
        self.dialog = Some(d);
        Ok(None)
    }
    fn key(&mut self, k: KeyEvent) -> Result<Option<Action>> {
        if self.busy {
            return Ok(None);
        }
        if let Some(d) = self.dialog.take() {
            return self.dialog_key(k, d);
        }
        if self.searching {
            if matches!(k.code, K::Esc | K::Enter) {
                self.searching = false;
            } else {
                self.filter.key(k, false);
                self.selected[self.page] = 0;
            }
            return Ok(None);
        }
        if let Some(action) = self.observation_key(k) {
            return Ok(action);
        }
        if let Some(action) = self.navigation_key(k)? {
            return Ok(action);
        }
        let s = &self.snapshot.store;
        match k.code{
            K::Char('I')=>self.import(false),
            K::Char(':')=>return self.activate(Command::Actions),
            K::Char('q')=>self.quit=true,K::Char('?')=>self.note("Help",view::help(),None),
            K::Char('c') if !self.snapshot.connected=>return Ok(Some(if s.native.is_none(){Action::ReviewMigration}else{Action::ReviewApply})),
            K::Char('A')=>return Ok(Some(if s.native.is_none(){Action::ReviewMigration}else{Action::ReviewApply})),
            K::Char('d')=>self.note("Stop and restore?","Restore sing-owned system proxy settings, then stop the core. If recovery fails, keep the core running. q closes only the interface.".into(),Some(Action::Disconnect)),
            K::Char('M')=>self.settings_form(false,true),
            K::Char('i') if self.page==0||self.page==10=>self.note("Install verified core?",format!("Download sing-box {} from SagerNet; no system installation.",runtime::CORE_VERSION),Some(Action::InstallCore)),
            K::Char('u') if s.native.is_none()=>return Ok(Some(Action::ReviewMigration)),
            K::Char('v') if self.page==0=>{ensure!(self.snapshot.connected,"Start the core before testing a connection");self.note("Check HTTPS connectivity?","Send an explicit HTTPS request through the local mixed proxy to www.gstatic.com/generate_204. Not a speed test.".into(),Some(Action::Probe));},
            K::Char('R')=>self.note("Restore system proxy?","Restore sing-owned settings without stopping the core.".into(),Some(Action::RestoreProxy)),
            K::Char('p')=>return Ok(Some(Action::Preview)),K::Char('V')=>return Ok(Some(Action::Check)),
            K::Char('b') if self.page==6=>self.note("Restore previous applied state?","Restore the previous complete state and restart the core.".into(),Some(Action::Rollback)),
            K::Char('/')=>self.searching=true,K::Esc=>self.filter=Input::new(String::new()),
            K::Down|K::Char('j')=>self.selected[self.page]=(self.selected[self.page]+1).min(self.rows().len().saturating_sub(1)),K::Up|K::Char('k')=>self.selected[self.page]=self.selected[self.page].saturating_sub(1),
            K::Char('a') if self.page==0||(self.page==5&&self.tabs[5]==0)=>self.import(false),K::Char('C') if self.page==3||(self.page==5&&self.tabs[5]==1)=>return Ok(Some(self.open("/route".into(),Intent::RuleImport))),
            K::Char('g') if self.page==2=>{ensure!(s.native.is_some(),"Initialize native configuration on Overview (u)");return Ok(Some(self.open("/outbounds/-".into(),Intent::Group)));},
            K::Char('a') if [1,2,3,4,5].contains(&self.page)=>{ensure!(s.native.is_some(),"Initialize native configuration on Overview (u)");let choices=templates(self.path());ensure!(!choices.is_empty(),"Edit Options instead");self.dialog=Some(Dialog::Add{choices,selected:0});},
            K::Char('s') if self.page==1||self.page==11=>self.settings_form(true,false),K::Char('e') if self.page==10=>self.settings_form(false,false),
            K::Char('e'|'E') if [1,2,3,4,5,6].contains(&self.page)=>{if self.page==5&&self.tabs[5]==0{return Ok(None);}let p=if self.page==6{if k.code==K::Char('E'){String::new()}else{let(_,n,_)=self.current().context("Select an object")?;format!("/{}",n.replace('~',"~0").replace('/',"~1"))}}else if(self.page==3&&self.tabs[3]==1)||(self.page==4&&self.tabs[4]==2){self.path().into()}else{let(i,_,_)=self.current().context("Select an object")?;format!("{}/{i}",self.path())};return Ok(Some(self.open(p,if k.code==K::Char('E')||self.page==6{Intent::Raw}else{Intent::Form})));},
            K::Enter if self.page==0||self.page==2=>{
                let(_,_,v)=self.current().context("Select an outbound")?;
                if v["type"]=="selector" {
                    let choices:Vec<String>=native::array(&v,"/outbounds").iter().filter_map(|s|s.as_str().map(str::to_string)).collect();
                    let current=self.snapshot.groups.group.iter().find(|g|g.tag==native::tag(&v)).map(|g|g.selected.as_str()).or_else(||v["default"].as_str()).unwrap_or("");
                    let selected=choices.iter().position(|s|s==current).unwrap_or(0);
                    self.dialog=Some(Dialog::Select{group:native::tag(&v).into(),choices,selected});
                }else{self.note("Outbound",view::details(self,&v),None);}
            },
            K::Enter=>{if let Some((_,n,v))=self.current(){self.note(&n,view::details(self,&v),None);}},
            K::Char('x') if [1,2,3,4,5].contains(&self.page)=>{if self.page==5&&self.tabs[5]==0{let(i,_,_)=self.current().context("Select a subscription")?;let id=s.subscriptions[i].id.clone();self.note("Remove subscription?","Remove its nodes from the draft; group references are not silently repaired.".into(),Some(Action::Delete(id)));}else{let(i,_,value)=self.current().context("Select an object")?;return Ok(Some(self.open(self.path().into(),Intent::Delete(i,value))));}},
            K::Char('J'|'K') if self.page==3||self.page==4=>{let(i,_,_)=self.current().context("Select a rule")?;return Ok(Some(self.open(self.path().into(),Intent::Move(i,if k.code==K::Char('J'){1}else{-1}))));},
            K::Char('r') if self.page==5=>{let(i,_,v)=self.current().context("Select a resource")?;return Ok(if self.tabs[5]==0{Some(Action::Refresh(s.subscriptions[i].id.clone()))}else{s.rule_resources.iter().find(|r|r.tag()==native::tag(&v)).map(|r|Action::RefreshRules(r.id.clone()))});},
            K::Char('t') if self.page==2=>{let(_,_,v)=self.current().context("Select an outbound")?;return Ok(Some(Action::Test(native::tag(&v).into())));},
            K::Char('r') if self.page==7=>return Ok(Some(Action::Connections)),K::Char('h') if self.page==7=>self.show_closed = !self.show_closed,
            K::Char('x') if self.page==7=>{let(_,_,v)=self.current().context("Select a connection")?;let c:api::Connection=serde_json::from_value(v)?;let detail=format!("{} → {}",c.source,c.destination);self.note("Close connection?",detail,Some(Action::CloseConnection(c.id)));},
            K::Char('r') if self.page==8=>return Ok(Some(Action::Logs)),K::Char('r') if self.page==9=>return Ok(Some(Action::Diagnostics)),_=>{}
        }
        Ok(None)
    }
}
fn enter() -> Result<()> {
    terminal::enable_raw_mode()?;
    execute!(
        io::stdout(),
        EnterAlternateScreen,
        event::EnableBracketedPaste
    )?;
    Ok(())
}
fn leave() -> Result<()> {
    terminal::disable_raw_mode()?;
    execute!(
        io::stdout(),
        event::DisableBracketedPaste,
        LeaveAlternateScreen
    )?;
    Ok(())
}
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = leave();
    }
}
pub fn run(dir: PathBuf, demo: bool) -> Result<()> {
    ensure!(
        io::stdin().is_terminal(),
        "Interactive terminal required; --preview renders a static preview"
    );
    let snap = if demo {
        sample()?
    } else {
        runtime::ensure_daemon(&dir)?;
        runtime::request(&dir, Action::Snapshot)?
            .snapshot
            .context("No manager snapshot")?
    };
    ensure!(demo||snap.manager_protocol>=runtime::PROTOCOL,"Old manager detected. Close old interfaces, run ./sing --shutdown, then reopen ./sing. Shutdown stops the old core; saved data remains intact.");
    let mut a = App::new(snap, demo);
    let (tx, rx) = mpsc::channel::<(Action, bool)>();
    let (out, results) = mpsc::channel::<(Result<Reply>, bool)>();
    let worker = dir.clone();
    std::thread::spawn(move || {
        for (action, background) in rx {
            let snapshot_only = matches!(action, Action::Snapshot);
            let mut r = runtime::request(&worker, action);
            if !snapshot_only {
                if let Ok(r) = &mut r {
                    if let Ok(s) = runtime::request(&worker, Action::Snapshot) {
                        r.snapshot = s.snapshot;
                    }
                }
            }
            if out.send((r, background)).is_err() {
                break;
            }
        }
    });
    let old = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |i| {
        let _ = leave();
        old(i);
    }));
    enter()?;
    let _guard = Guard;
    let mut term = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut last = Instant::now() - Duration::from_secs(3);
    let mut polling = false;
    let mut redraw = true;
    let mut painted_second = model::now();
    while !a.quit {
        if painted_second != model::now() {
            painted_second = model::now();
            redraw = true;
        }
        let mut action = None;
        while let Ok((r, background)) = results.try_recv() {
            redraw = true;
            if background {
                polling = false;
                a.receive_poll(r);
                continue;
            }
            match r {
                Ok(r) => match a.receive(r) {
                    Ok(next) => action = next,
                    Err(e) => {
                        a.notice = e.to_string();
                        a.error = true;
                    }
                },
                Err(e) => {
                    a.busy = false;
                    a.failed(e.to_string());
                }
            }
        }
        if redraw {
            term.draw(|f| view::draw(f, &a))?;
            redraw = false;
        }
        if event::poll(Duration::from_millis(100))? {
            redraw = true;
            match event::read()? {
                Event::Key(k) if k.kind != event::KeyEventKind::Release => {
                    if k.code == K::Enter && matches!(a.dialog, Some(Dialog::Auth { .. })) {
                        let Some(Dialog::Auth { kind, after }) = a.dialog.take() else {
                            unreachable!()
                        };
                        leave()?;
                        let r = if kind == "system" {
                            crate::system_proxy::helper::authorize(&dir)
                        } else {
                            runtime::authorize_tun(&dir, &a.snapshot.core)
                        };
                        enter()?;
                        term.clear()?;
                        match r {
                            Ok(()) => action = Some(after),
                            Err(e) => {
                                a.notice = e.to_string();
                                a.error = true;
                            }
                        }
                    } else {
                        match a.key(k) {
                            Ok(next) => {
                                if next.is_some() {
                                    action = next;
                                }
                            }
                            Err(e) => {
                                a.notice = e.to_string();
                                a.error = true;
                            }
                        }
                    }
                }
                Event::Paste(s) => match &mut a.dialog {
                    Some(Dialog::Group(g)) => g.paste(&s),
                    Some(Dialog::InlineGroup { editor, .. }) => editor.paste(&s),
                    Some(Dialog::RuleImport(r)) if r.preview.is_none() => {
                        if let Some(field) = r.form.fields.get_mut(r.form.selected) {
                            field.input.insert(&s);
                        }
                    }
                    Some(Dialog::Form(f)) if f.selected < f.visible_fields() => {
                        if let Some(field) = f.fields.get_mut(f.selected) {
                            field.input.insert(&s);
                        }
                    }
                    Some(Dialog::Json { input, .. }) => input.insert(&s),
                    _ => {}
                },
                _ => {}
            }
        }
        if let Some(action) = action {
            redraw = true;
            if demo {
                demo_action(&mut a, action)?;
            } else {
                a.busy = true;
                tx.send((action, false))?;
                last = Instant::now();
            }
        } else if !demo
            && !a.busy
            && !polling
            && a.dialog.is_none()
            && (a.refresh_requested || last.elapsed() > Duration::from_secs(2))
        {
            polling = true;
            a.refresh_requested = false;
            tx.send((
                if a.page == 7 || a.page == 0 {
                    Action::Connections
                } else {
                    Action::Snapshot
                },
                true,
            ))?;
            last = Instant::now();
        }
    }
    drop(term);
    drop(_guard);
    println!("Interface closed; a running core stays active.");
    Ok(())
}
fn demo_action(a: &mut App, action: Action) -> Result<()> {
    match action {
        Action::Connections => {}
        Action::ConnectionSetupInfo => demo_action(a, Action::ReadNative(String::new()))?,
        Action::ReviewConnectionSetup(change) => {
            let s = native::connection_setup(
                &a.snapshot.store,
                &change,
                a.snapshot.ssh,
                cfg!(target_os = "macos"),
            )?;
            let mut r: Reply = serde_json::from_value(
                json!({"ok":true,"message":"Demo setup preview only","needs_auth":false}),
            )?;
            r.config = Some(native::connection_setup_review(&a.snapshot.store, &s)?);
            r.confirm = Some(Action::SaveConnectionSetup(change));
            a.receive(r)?;
        }
        Action::SaveConnectionSetup(change) => {
            a.snapshot.store = native::connection_setup(
                &a.snapshot.store,
                &change,
                a.snapshot.ssh,
                cfg!(target_os = "macos"),
            )?;
            a.snapshot.dirty = true;
            a.retry = None;
            a.notice = "Demo setup saved in memory; no network changes".into();
        }
        Action::ReadNative(p) => {
            let e = native::read(&a.snapshot.store, p)?;
            let r = Reply {
                diff: None,
                edit: Some(e),
                confirm: None,
                connections: None,
                rules_preview: None,
                auth_kind: String::new(),
                after_auth: None,
                ok: true,
                message: String::new(),
                snapshot: None,
                preview: None,
                config: None,
                needs_auth: false,
                cores: None,
            };
            if let Some(next) = a.receive(r)? {
                demo_action(a, next)?;
            }
        }
        Action::WriteNative(e) => {
            native::write(&mut a.snapshot.store, e)?;
            a.retry = None;
            a.notice = "Demo draft saved in memory".into();
        }
        Action::WriteGroup(change) => {
            native::write_group(&mut a.snapshot.store, change)?;
            a.retry = None;
            a.snapshot.dirty = true;
            a.notice = "Demo group saved in memory; not applied".into();
        }
        Action::SaveSettings(s) => a.snapshot.store.settings = s,
        Action::Preview => a.note(
            "Native preview",
            serde_json::to_string_pretty(&config::redacted(&config::generate(&a.snapshot.store)?))?,
            None,
        ),
        Action::ReviewApply => {
            a.dialog = Some(Dialog::ApplyReview {
                summary: native::review(&a.snapshot.store, None)?,
                diff: native::review::detailed(&json!({}), &config::generate(&a.snapshot.store)?),
                expanded: false,
                focus: 0,
                scroll: 0,
                action: Action::ApplyNative {
                    revision: native::revision(&a.snapshot.store),
                },
            });
        }
        Action::ApplyNative { .. } => {
            a.retry = None;
            a.notice = "Offline demo: Apply does not start a core or change networking".into();
        }
        Action::Diagnostics => a.note(
            "Diagnostics",
            config::diagnostics(&a.snapshot.store, None),
            None,
        ),
        Action::SelectNative { group, member } => {
            if let Some(g) = a.snapshot.store.native.as_mut().unwrap()["outbounds"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|v| native::tag(v) == group)
            {
                g["default"] = json!(member);
            }
        }
        _ => a.notice = "Offline demo: no network or persistent changes".into(),
    }
    Ok(())
}
fn sample() -> Result<Snapshot> {
    let mut store = model::Store::new()?;
    store.nodes=crate::subscription::parse("trojan://fictional@jp.example.invalid:443#Tokyo\ntrojan://fictional@sg.example.invalid:443#Singapore","demo")?.nodes;
    let d = native::migration(&store)?;
    native::adopt(&mut store, d)?;
    Ok(Snapshot {
        selection_recovery: String::new(),
        manager_protocol: 9,
        system_proxy: Default::default(),
        connectivity: Default::default(),
        store,
        connected: false,
        api_ready: false,
        core: "sing-box".into(),
        version: "sing-box 1.14.0".into(),
        running_settings: None,
        running_tun: false,
        dirty: false,
        status: api::Status::default(),
        groups: api::Groups::default(),
        activity: vec!["Native workspace ready".into()],
        host: "localhost".into(),
        ssh: false,
    })
}
pub fn preview() -> Result<()> {
    let a = App::new(sample()?, true);
    let mut t = Terminal::new(TestBackend::new(110, 30))?;
    t.draw(|f| view::draw(f, &a))?;
    for row in t.backend().buffer().content.chunks(110) {
        println!("{}", row.iter().map(|c| c.symbol()).collect::<String>());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn top_shortcuts_and_group_entry_preserve_document() {
        let mut a = App::new(sample().unwrap(), true);
        let before = a.doc();
        a.key(KeyEvent::new(K::Char('2'), M::NONE)).unwrap();
        assert_eq!(a.page, 2);
        let action = a
            .key(KeyEvent::new(K::Char('g'), M::NONE))
            .unwrap()
            .unwrap();
        demo_action(&mut a, action).unwrap();
        let Some(Dialog::Group(group)) = &a.dialog else {
            panic!()
        };
        assert!(group.name.value.is_empty());
        assert!(!group.automatic);
        a.key(KeyEvent::new(K::Esc, M::NONE)).unwrap();
        a.key(KeyEvent::new(K::Char('3'), M::NONE)).unwrap();
        assert_eq!(a.page, 3);
        assert_eq!(a.focus, Focus::Content);
        assert_eq!(a.doc(), before);
    }
    #[test]
    fn advanced_excludes_dedicated_pages_but_full_edit_remains_available() {
        let mut a = App::new(sample().unwrap(), true);
        a.page = 6;
        let before = a.doc();
        let rows = a.rows();
        assert!(rows.iter().all(
            |(_, name, _)| !["dns", "route", "inbounds", "outbounds"].contains(&name.as_str())
        ));
        assert!(rows.iter().any(|(_, name, _)| name == "services"));
        assert!(
            matches!(a.key(KeyEvent::new(K::Char('E'), M::NONE)).unwrap(), Some(Action::ReadNative(p)) if p.is_empty())
        );
        assert_eq!(a.doc(), before);
    }
    #[test]
    fn top_navigation_and_import_hints_visible_at_all_sizes() {
        for (w, h) in [(120, 35), (80, 24), (54, 18)] {
            let mut a = App::new(sample().unwrap(), true);
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            for (page, tab, hint) in [
                (0, 0, "v Test Connection"),
                (2, 0, "g New Group"),
                (5, 0, "I Import Subscription"),
                (5, 1, "C Import Rule Set"),
            ] {
                a.page = page;
                a.tabs[page] = tab;
                t.draw(|f| view::draw(f, &a)).unwrap();
                let lines: Vec<_> = t
                    .backend()
                    .buffer()
                    .content
                    .chunks(w as usize)
                    .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
                    .collect();
                let rendered = lines.join("\n");
                assert!(rendered.contains(hint), "{w}x{h}: {hint}");
                assert!(lines[..5].join("\n").contains("Overview"));
                assert!(!lines[..5].join("\n").contains(", Settings"));
                assert!(lines[lines.len() - 2..].join("\n").contains(", Settings"));
                assert!(rendered.contains("Quit"));
            }
        }
    }
    #[test]
    fn native_form_preserves_hidden_fields() {
        let s = sample().unwrap().store;
        let mut e = native::read(&s, "/dns/servers/1".into()).unwrap();
        e.value["tls"] = json!({"server_name":"dns.invalid","future":true});
        let before = e.value.clone();
        let mut f = object_form(e, s.native.as_ref().unwrap()).unwrap();
        let i = f.fields.iter().position(|f| f.key == "server").unwrap();
        f.fields[i].input = Input::new("resolver.invalid".into());
        let Action::WriteNative(e) = f.submit().unwrap() else {
            panic!()
        };
        assert_eq!(e.value["tls"], before["tls"]);
        assert_eq!(e.value["server"], "resolver.invalid");
    }
    #[test]
    fn member_picker_preserves_order_and_cancel_returns_to_form() {
        let mut a = App::new(sample().unwrap(), true);
        let e = Edit {
            pointer: "/outbounds/-".into(),
            revision: "test".into(),
            value: json!({"type":"selector","tag":"ordered","outbounds":["proxy","direct"]}),
        };
        let mut f = object_form(e, &a.doc()).unwrap();
        f.selected = f.fields.iter().position(|f| f.key == "outbounds").unwrap();
        a.dialog = Some(Dialog::Form(f));
        a.key(KeyEvent::new(K::Char(' '), M::NONE)).unwrap();
        assert!(matches!(a.dialog, Some(Dialog::Members { .. })));
        a.key(KeyEvent::new(K::Esc, M::NONE)).unwrap();
        assert!(matches!(a.dialog, Some(Dialog::Form(_))));
        a.key(KeyEvent::new(K::Char(' '), M::NONE)).unwrap();
        a.key(KeyEvent::new(K::F(2), M::NONE)).unwrap();
        let Some(Dialog::Form(f)) = a.dialog else {
            panic!()
        };
        let Action::WriteNative(e) = f.submit().unwrap() else {
            panic!()
        };
        assert_eq!(e.value["outbounds"], json!(["proxy", "direct"]));
    }
    #[test]
    fn overview_uses_running_tun_not_unapplied_draft() {
        let mut a = App::new(sample().unwrap(), true);
        a.snapshot.connected = true;
        a.snapshot.store.native.as_mut().unwrap()["inbounds"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type":"tun"}));
        let mut t = Terminal::new(TestBackend::new(120, 35)).unwrap();
        t.draw(|f| view::draw(f, &a)).unwrap();
        let rendered = t
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(rendered.contains("TUN             Off"));
        a.snapshot.running_tun = true;
        t.draw(|f| view::draw(f, &a)).unwrap();
        let rendered = t
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(rendered.contains("TUN             Running"));
    }
    #[test]
    fn apply_review_has_visible_diff_back_and_explicit_apply() {
        let mut a = App::new(sample().unwrap(), true);
        let mut reply:Reply=serde_json::from_value(json!({"ok":true,"message":"Review","needs_auth":false,"config":"DNS unchanged. Apply restarts the core.","diff":"/route/final\n- direct\n+ proxy","confirm":{"action":"apply_native","data":{"revision":"authoritative"}}})).unwrap();
        a.receive(reply.clone()).unwrap();
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
                text.contains("[Native Diff]")
                    && text.contains("[Back]")
                    && text.contains("[Apply]")
            );
        }
        a.key(KeyEvent::new(K::Tab, M::NONE)).unwrap();
        assert!(a.key(KeyEvent::new(K::Enter, M::NONE)).unwrap().is_none());
        assert!(matches!(
            a.dialog,
            Some(Dialog::ApplyReview { expanded: true, .. })
        ));
        a.key(KeyEvent::new(K::BackTab, M::NONE)).unwrap();
        assert!(
            matches!(a.key(KeyEvent::new(K::Enter,M::NONE)).unwrap(),Some(Action::ApplyNative{revision}) if revision=="authoritative")
        );
        reply.ok = false;
        reply.message = "Draft changed".into();
        reply.config = None;
        reply.confirm = None;
        reply.diff = None;
        a.receive(reply).unwrap();
        a.key(KeyEvent::new(K::Esc, M::NONE)).unwrap();
        assert!(matches!(
            a.dialog,
            Some(Dialog::ApplyReview { expanded: true, .. })
        ));
    }
    #[test]
    fn english_workspace_and_small_terminal() {
        for (w, h) in [(120, 35), (80, 24), (54, 18)] {
            let mut a = App::new(sample().unwrap(), true);
            a.snapshot.store.settings.language = "zh".into();
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            for p in 0..12 {
                a.page = p;
                t.draw(|f| view::draw(f, &a)).unwrap();
                let rendered = t
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|c| c.symbol())
                    .collect::<String>();
                assert!(!rendered
                    .chars()
                    .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)));
                assert!(rendered.contains("Quit"));
            }
        }
    }
    #[test]
    fn start_apply_and_navigation_are_distinct() {
        let mut a = App::new(sample().unwrap(), true);
        assert!(matches!(
            a.key(KeyEvent::new(K::Char('c'), M::NONE)).unwrap(),
            Some(Action::ReviewApply)
        ));
        a.snapshot.connected = true;
        assert!(a
            .key(KeyEvent::new(K::Char('c'), M::NONE))
            .unwrap()
            .is_none());
        assert!(matches!(
            a.key(KeyEvent::new(K::Char('A'), M::NONE)).unwrap(),
            Some(Action::ReviewApply)
        ));
        a.key(KeyEvent::new(K::Tab, M::NONE)).unwrap();
        a.key(KeyEvent::new(K::Tab, M::NONE)).unwrap();
        a.key(KeyEvent::new(K::Tab, M::NONE)).unwrap();
        a.key(KeyEvent::new(K::Down, M::NONE)).unwrap();
        assert_eq!(a.page, 0);
        a.key(KeyEvent::new(K::Char('2'), M::NONE)).unwrap();
        assert_eq!(a.page, 2);
    }
    #[test]
    fn unicode_input_and_json_error_keep_editor() {
        let mut i = Input::new("A中B".into());
        i.key(KeyEvent::new(K::Left, M::NONE), true);
        i.key(KeyEvent::new(K::Backspace, M::NONE), true);
        assert_eq!(i.value, "AB");
        let mut a = App::new(sample().unwrap(), true);
        a.dialog = Some(Dialog::Json {
            edit: native::read(&a.snapshot.store, "".into()).unwrap(),
            input: Input::new("{".into()),
        });
        assert!(a.key(KeyEvent::new(K::F(2), M::NONE)).unwrap().is_none());
        assert!(a.dialog.is_some());
        assert!(a.error);
    }
    #[test]
    fn all_native_forms_render_without_hiding_save() {
        let a = App::new(sample().unwrap(), true);
        for p in [
            "/inbounds",
            "/outbounds",
            "/route/rules",
            "/dns/servers",
            "/dns/rules",
            "/route/rule_set",
        ] {
            for (_, v) in templates(p) {
                let form = object_form(
                    Edit {
                        pointer: format!("{p}/-"),
                        revision: "demo".into(),
                        value: v,
                    },
                    &a.doc(),
                )
                .unwrap();
                let mut t = Terminal::new(TestBackend::new(54, 18)).unwrap();
                t.draw(|f| {
                    view::draw_dialog(
                        f,
                        &Dialog::Form(form.clone()),
                        ratatui::layout::Rect::new(0, 0, 54, 18),
                        None,
                    )
                })
                .unwrap();
                let rendered = t
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|c| c.symbol())
                    .collect::<String>();
                assert!(rendered.contains("F2 Save"));
            }
        }
    }
}
