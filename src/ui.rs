//! Native-object workspace; English UI, independent of names in user data.
mod forms;
mod view;
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
use ratatui::{
    backend::{CrosstermBackend, TestBackend},
    Terminal,
};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    io::{self, IsTerminal},
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};
const PAGES: [&str; 11] = [
    "Overview",
    "Inbounds",
    "Outbounds",
    "Routing",
    "DNS",
    "Resources",
    "Advanced",
    "Connections",
    "Logs",
    "Diagnostics",
    "Settings",
];
const PAGE_KEYS: [char; 11] = ['1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '-'];
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
    Add(Value),
    Delete(usize),
    Move(usize, isize),
}
#[derive(Clone)]
enum Dialog {
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
    snapshot: Snapshot,
    page: usize,
    nav: bool,
    selected: [usize; 11],
    tabs: [usize; 11],
    filter: Input,
    searching: bool,
    dialog: Option<Dialog>,
    intent: Option<Intent>,
    retry: Option<Dialog>,
    notice: String,
    error: bool,
    busy: bool,
    connections: runtime::ConnectionReport,
    show_closed: bool,
    quit: bool,
    demo: bool,
}
impl App {
    fn new(snapshot: Snapshot, demo: bool) -> Self {
        Self {
            snapshot,
            page: 0,
            nav: false,
            selected: [0; 11],
            tabs: [0; 11],
            filter: Input::new(String::new()),
            searching: false,
            dialog: None,
            intent: None,
            retry: None,
            notice: String::new(),
            error: false,
            busy: false,
            connections: Default::default(),
            show_closed: false,
            quit: false,
            demo,
        }
    }
    fn doc(&self) -> Value {
        self.snapshot.store.native.clone().unwrap_or(json!({}))
    }
    fn label(&self, tag: &str) -> String {
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
    fn tab_names(&self) -> Vec<&str> {
        match self.page {
            3 => vec!["Rules", "Options"],
            4 => vec!["Resolvers", "Rules", "Options"],
            5 => vec!["Subscriptions", "Rule sets"],
            _ => vec![],
        }
    }
    fn rows(&self) -> Vec<(usize, String, Value)> {
        let doc = self.doc();
        let all:Vec<_>=match self.page{
        0=>native::array(&doc,"/outbounds").iter().enumerate().filter(|(_,v)|v["type"]=="selector"||v["type"]=="urltest").map(|(i,v)|(i,self.label(native::tag(v)),v.clone())).collect(),
        5 if self.tabs[5]==0=>self.snapshot.store.subscriptions.iter().enumerate().map(|(i,s)|(i,format!("{} · {}",s.name,s.format),json!({"name":s.name,"source":s.source,"updated_at":s.updated_at,"warnings":s.warnings}))).collect(),
        7=>self.connections.items.iter().enumerate().filter(|(_,c)|self.show_closed||c.closed_at==0).map(|(i,c)|(i,format!("{} → {} {}",if c.domain.is_empty(){&c.destination}else{&c.domain},c.outbound,if c.closed_at==0{""}else{"[closed]"}),serde_json::to_value(c).unwrap())).collect(),
        3 if self.tabs[3]==1=>vec![(0,"Routing options".into(),doc["route"].clone())],4 if self.tabs[4]==2=>vec![(0,"DNS options".into(),doc["dns"].clone())],
        6=>doc.as_object().map(|m|m.iter().filter(|(k,_)|!["inbounds","outbounds","route","dns"].contains(&k.as_str())).enumerate().map(|(i,(k,v))|(i,k.clone(),v.clone())).collect()).unwrap_or_default(),
        _=>native::array(&doc,self.path()).iter().enumerate().map(|(i,v)|{let label=if self.page==2{self.snapshot.store.nodes.iter().find(|n|n.tag()==native::tag(v)).map(|n|format!("{} · {} · {}",n.name,n.kind(),n.server())).unwrap_or_else(||format!("{} · {}",self.label(native::tag(v)),text(&v["type"])))}else{title(v)};let delay=if self.page==2{self.snapshot.groups.group.iter().flat_map(|g|g.items.iter()).filter(|m|m.tag==native::tag(v)&&m.delay>0).max_by_key(|m|m.time).map(|m|format!(" · {} ms",m.delay)).unwrap_or_default()}else{String::new()};(i,format!("{label}{delay}"),v.clone())}).collect()};
        let q = self.filter.value.to_lowercase();
        all.into_iter()
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
                FormAction::Import
            },
        }));
    }
    fn receive(&mut self, r: Reply) -> Result<Option<Action>> {
        self.busy = false;
        if let Some(s) = r.snapshot {
            self.snapshot = s;
            if !self.snapshot.connected {
                self.connections = Default::default();
            }
        }
        if !r.ok {
            self.error = true;
            self.notice = r.message;
            if let Some(d) = self.retry.take() {
                self.dialog = Some(d);
            }
            return Ok(None);
        }
        self.retry = None;
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
            self.connections = c;
        }
        if let Some(edit) = r.edit {
            match self.intent.take().unwrap_or(Intent::Raw) {
                Intent::Raw => {
                    self.dialog = Some(Dialog::Json {
                        input: Input::new(serde_json::to_string_pretty(&edit.value)?),
                        edit,
                    })
                }
                Intent::Form => self.dialog = Some(Dialog::Form(object_form(edit, &self.doc())?)),
                Intent::Add(value) => {
                    self.dialog = Some(Dialog::Form(object_form(
                        Edit {
                            pointer: format!("{}/-", edit.pointer),
                            value,
                            revision: edit.revision,
                        },
                        &self.doc(),
                    )?))
                }
                Intent::Delete(i) => {
                    let mut e = edit;
                    e.value.as_array_mut().context("Not a list")?.remove(i);
                    self.note(
                        "Remove from draft?",
                        "Referenced objects must be repaired before Apply.".into(),
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
            self.note(
                "Review node subscription",
                format!(
                    "{} · {}\n{} nodes: +{} / -{}\n\n{}\n\n{}",
                    p.name,
                    p.format,
                    p.count,
                    p.added,
                    p.removed,
                    p.names.join("\n"),
                    p.warnings.join("\n")
                ),
                Some(Action::CommitImport),
            );
        }
        if let Some(p) = r.rules_preview {
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
        if k.code == K::Esc {
            return Ok(match d {
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
            Dialog::Text { scroll, action, .. } => match k.code {
                K::Enter if action.is_some() => return Ok(action.take()),
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
                if save {
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
                    f.selected = (f.selected + 1) % f.fields.len();
                } else if k.code == K::BackTab || k.code == K::Up {
                    f.selected = (f.selected + f.fields.len() - 1) % f.fields.len();
                } else {
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
                    return Ok(choices.get(*selected).map(|s| Action::SelectNative {
                        group: group.clone(),
                        member: s.clone(),
                    }))
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
        if k.code == K::Tab || k.code == K::BackTab {
            self.nav = !self.nav;
            return Ok(None);
        }
        if let K::Char(c) = k.code {
            if let Some(page) = PAGE_KEYS.iter().position(|key| *key == c) {
                self.page = page;
                self.nav = false;
                self.filter = Input::new(String::new());
                return Ok((page == 7).then_some(Action::Connections));
            }
        }
        if self.nav {
            if matches!(k.code, K::Enter | K::Esc) {
                self.nav = false;
                return Ok(None);
            }
            match k.code {
                K::Left | K::Up | K::Char('k') => self.page = self.page.saturating_sub(1),
                K::Right | K::Down | K::Char('j') => self.page = (self.page + 1).min(10),
                _ => {}
            }
            self.filter = Input::new(String::new());
            if self.page == 7
                && matches!(
                    k.code,
                    K::Left | K::Right | K::Up | K::Down | K::Char('j' | 'k')
                )
            {
                return Ok(Some(Action::Connections));
            }
        }
        let s = &self.snapshot.store;
        match k.code{
            K::Char('q')=>self.quit=true,K::Char('?')=>self.note("Help",view::help(),None),
            K::Char('c') if !self.snapshot.connected=>return Ok(Some(if s.native.is_none(){Action::ReviewMigration}else{Action::ReviewApply})),
            K::Char('A')=>return Ok(Some(if s.native.is_none(){Action::ReviewMigration}else{Action::ReviewApply})),
            K::Char('d')=>self.note("Stop and restore?","Restore sing-owned system proxy settings, then stop the core. If recovery fails, keep the core running. q closes only the interface.".into(),Some(Action::Disconnect)),
            K::Char('M')=>self.settings_form(false,true),
            K::Char('i') if self.page==0||self.page==10=>self.note("Install verified core?",format!("Download sing-box {} from SagerNet; no system installation.",runtime::CORE_VERSION),Some(Action::InstallCore)),
            K::Char('u') if s.native.is_none()=>return Ok(Some(Action::ReviewMigration)),
            K::Char('v') if self.page==0=>self.note("Check HTTPS connectivity?","Send an explicit HTTPS request through the local mixed proxy to gstatic. Not a speed test.".into(),Some(Action::Probe)),
            K::Char('R')=>self.note("Restore system proxy?","Restore sing-owned settings without stopping the core.".into(),Some(Action::RestoreProxy)),
            K::Char('p')=>return Ok(Some(Action::Preview)),K::Char('V')=>return Ok(Some(Action::Check)),
            K::Char('b') if self.page==6=>self.note("Restore previous applied state?","Restore the previous complete state and restart the core.".into(),Some(Action::Rollback)),
            _ if self.nav=>{},K::Char('/')=>self.searching=true,K::Esc=>self.filter=Input::new(String::new()),
            K::Down|K::Char('j')=>self.selected[self.page]=(self.selected[self.page]+1).min(self.rows().len().saturating_sub(1)),K::Up|K::Char('k')=>self.selected[self.page]=self.selected[self.page].saturating_sub(1),
            K::Char('['|']') if !self.tab_names().is_empty()=>{let n=self.tab_names().len();self.tabs[self.page]=if k.code==K::Char(']'){(self.tabs[self.page]+1)%n}else{(self.tabs[self.page]+n-1)%n};self.selected[self.page]=0;self.filter=Input::new(String::new());},
            K::Char('a') if self.page==0||(self.page==5&&self.tabs[5]==0)=>self.import(false),K::Char('C') if self.page==5&&self.tabs[5]==1=>self.import(true),
            K::Char('g') if self.page==2=>{ensure!(s.native.is_some(),"Initialize native configuration on Overview (u)");self.dialog=Some(Dialog::Add{choices:templates("/outbounds").into_iter().take(2).collect(),selected:0});},
            K::Char('a') if [1,2,3,4,5].contains(&self.page)=>{ensure!(s.native.is_some(),"Initialize native configuration on Overview (u)");let choices=templates(self.path());ensure!(!choices.is_empty(),"Edit Options instead");self.dialog=Some(Dialog::Add{choices,selected:0});},
            K::Char('s') if self.page==1=>self.settings_form(true,false),K::Char('e') if self.page==10=>self.settings_form(false,false),
            K::Char('e'|'E') if [1,2,3,4,5,6].contains(&self.page)=>{if self.page==5&&self.tabs[5]==0{return Ok(None);}let p=if self.page==6{if k.code==K::Char('E'){String::new()}else{let(_,n,_)=self.current().context("Select an object")?;format!("/{}",n.replace('~',"~0").replace('/',"~1"))}}else if(self.page==3&&self.tabs[3]==1)||(self.page==4&&self.tabs[4]==2){self.path().into()}else{let(i,_,_)=self.current().context("Select an object")?;format!("{}/{i}",self.path())};return Ok(Some(self.open(p,if k.code==K::Char('E')||self.page==6{Intent::Raw}else{Intent::Form})));},
            K::Enter if self.page==0||self.page==2=>{let(_,_,v)=self.current().context("Select an outbound")?;if v["type"]=="selector"{self.dialog=Some(Dialog::Select{group:native::tag(&v).into(),choices:native::array(&v,"/outbounds").iter().filter_map(|s|s.as_str().map(str::to_string)).collect(),selected:0});}else{self.note("Outbound",serde_json::to_string_pretty(&v)?,None);}},
            K::Enter=>{if let Some((_,n,v))=self.current(){self.note(&n,serde_json::to_string_pretty(&v)?,None);}},
            K::Char('x') if [1,2,3,4,5].contains(&self.page)=>{if self.page==5&&self.tabs[5]==0{let(i,_,_)=self.current().context("Select a subscription")?;let id=s.subscriptions[i].id.clone();self.note("Remove subscription?","Remove its nodes from the draft; group references are not silently repaired.".into(),Some(Action::Delete(id)));}else{let(i,_,_)=self.current().context("Select an object")?;return Ok(Some(self.open(self.path().into(),Intent::Delete(i))));}},
            K::Char('J'|'K') if self.page==3||self.page==4=>{let(i,_,_)=self.current().context("Select a rule")?;return Ok(Some(self.open(self.path().into(),Intent::Move(i,if k.code==K::Char('J'){1}else{-1}))));},
            K::Char('r') if self.page==5=>{let(i,_,v)=self.current().context("Select a resource")?;return Ok(if self.tabs[5]==0{Some(Action::Refresh(s.subscriptions[i].id.clone()))}else{s.rule_resources.iter().find(|r|r.tag()==native::tag(&v)).map(|r|Action::RefreshRules(r.id.clone()))});},
            K::Char('t') if self.page==2=>{let(_,_,v)=self.current().context("Select an outbound")?;return Ok(Some(Action::Test(native::tag(&v).into())));},
            K::Char('r') if self.page==7=>return Ok(Some(Action::Connections)),K::Char('h') if self.page==7=>self.show_closed = !self.show_closed,
            K::Char('x') if self.page==7=>{let(i,_,_)=self.current().context("Select a connection")?;let c=&self.connections.items[i];let id=c.id.clone();let detail=format!("{} → {}",c.source,c.destination);self.note("Close connection?",detail,Some(Action::CloseConnection(id)));},
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
    ensure!(demo||snap.manager_protocol>=6,"Old manager detected. Close old interfaces, run ./sing --shutdown, then reopen ./sing. Shutdown stops the old core; saved data remains intact.");
    let mut a = App::new(snap, demo);
    let (tx, rx) = mpsc::channel::<Action>();
    let (out, results) = mpsc::channel::<Result<Reply>>();
    let worker = dir.clone();
    std::thread::spawn(move || {
        for action in rx {
            let polling = matches!(action, Action::Snapshot);
            let mut r = runtime::request(&worker, action);
            if !polling {
                if let Ok(r) = &mut r {
                    if let Ok(s) = runtime::request(&worker, Action::Snapshot) {
                        r.snapshot = s.snapshot;
                    }
                }
            }
            if out.send(r).is_err() {
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
    let mut last = Instant::now();
    let mut redraw = true;
    while !a.quit {
        let mut action = None;
        while let Ok(r) = results.try_recv() {
            redraw = true;
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
                    a.notice = e.to_string();
                    a.error = true;
                    if let Some(d) = a.retry.take() {
                        a.dialog = Some(d);
                    }
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
                    Some(Dialog::Form(f)) => f.fields[f.selected].input.insert(&s),
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
                tx.send(action)?;
                last = Instant::now();
            }
        } else if !demo && !a.busy && a.dialog.is_none() && last.elapsed() > Duration::from_secs(2)
        {
            a.busy = true;
            tx.send(if a.page == 7 {
                Action::Connections
            } else {
                Action::Snapshot
            })?;
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
        Action::ReadNative(p) => {
            let e = native::read(&a.snapshot.store, p)?;
            let r = Reply {
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
        Action::SaveSettings(s) => a.snapshot.store.settings = s,
        Action::Preview => a.note(
            "Native preview",
            serde_json::to_string_pretty(&config::redacted(&config::generate(&a.snapshot.store)?))?,
            None,
        ),
        Action::ReviewApply => a.note("Review", native::review(&a.snapshot.store, None)?, None),
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
        manager_protocol: 6,
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
        a.key(KeyEvent::new(K::Char('3'), M::NONE)).unwrap();
        assert_eq!(a.page, 2);
        a.key(KeyEvent::new(K::Char('g'), M::NONE)).unwrap();
        let Some(Dialog::Add { choices, .. }) = &a.dialog else {
            panic!()
        };
        assert_eq!(choices.len(), 2);
        assert!(choices[0].0.contains("Manual group"));
        a.key(KeyEvent::new(K::Esc, M::NONE)).unwrap();
        a.key(KeyEvent::new(K::Tab, M::NONE)).unwrap();
        a.key(KeyEvent::new(K::Right, M::NONE)).unwrap();
        assert_eq!(a.page, 3);
        assert!(a.nav);
        a.key(KeyEvent::new(K::Enter, M::NONE)).unwrap();
        assert!(!a.nav);
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
                (0, 0, "a Import subscription"),
                (2, 0, "g New group"),
                (5, 0, "a Import subscription"),
                (5, 1, "C Import QX/Clash rules"),
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
                assert!(lines[1].contains("1 Overview"));
                assert!(lines[..5].join("\n").contains("- Settings"));
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
        assert!(rendered.contains("TUN  Off"));
        a.snapshot.running_tun = true;
        t.draw(|f| view::draw(f, &a)).unwrap();
        let rendered = t
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(rendered.contains("TUN  Running"));
    }
    #[test]
    fn english_workspace_and_small_terminal() {
        for (w, h) in [(120, 35), (80, 24), (54, 18)] {
            let mut a = App::new(sample().unwrap(), true);
            a.snapshot.store.settings.language = "zh".into();
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            for p in 0..11 {
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
        a.key(KeyEvent::new(K::Down, M::NONE)).unwrap();
        assert_eq!(a.page, 1);
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
