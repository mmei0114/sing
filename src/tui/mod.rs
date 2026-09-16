//! sing terminal interface.
//!
//! Three workspaces sit on top (Overview · Proxies & Rules · Activity); the
//! always-needed controls (Start, Mode, TUN, System Proxy, Config) sit at the
//! bottom. Everything else lives in Config, laid out like sing-box's own
//! configuration and edited with one shared object editor.
mod activity;
mod chrome;
mod config;
mod demo;
mod editor;
mod flows;
mod history;
mod input;
mod labels;
mod modal;
mod overview;
mod proxies;
mod quick_rule;
mod schema;
#[cfg(test)]
mod tests;
mod text;
mod theme;

use crate::{
    api, native,
    runtime::{self, Action, CoreReport, Reply, Snapshot},
};
use anyhow::{ensure, Context, Result};
use crossterm::{
    event::{self, Event, KeyCode as K, KeyEvent, KeyModifiers as M},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use modal::{Modal, Outcome};
use ratatui::{
    backend::{CrosstermBackend, TestBackend},
    Frame, Terminal,
};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    io::{self, IsTerminal},
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

pub type Then = Box<dyn FnOnce(&mut App, Reply)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Proxies,
    Activity,
    Config,
}
impl Tab {
    pub const MAIN: [Tab; 3] = [Tab::Overview, Tab::Proxies, Tab::Activity];
    pub fn name(self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Proxies => "Policies",
            Tab::Activity => "Activity",
            Tab::Config => "Config",
        }
    }
}

pub struct Toast {
    pub text: String,
    pub error: bool,
    pub at: Instant,
}

pub struct App {
    pub snap: Snapshot,
    pub tab: Tab,
    pub last_tab: Tab,
    pub modals: Vec<Box<dyn Modal>>,
    pub toast: Option<Toast>,
    pub busy: Option<(String, Instant)>,
    pub demo: Option<demo::Demo>,
    pub quit: bool,
    pub overview: overview::State,
    pub proxies: proxies::State,
    pub activity: activity::State,
    pub config: config::State,
    pub history: history::History,
    /// Recent (downlink, uplink) rates for the traffic sparkline.
    pub traffic: VecDeque<(i64, i64)>,
    pub logs: Vec<String>,
    pub cores: Option<CoreReport>,
    pub poll_error: String,
    outbox: VecDeque<(Action, Then, Option<String>)>,
    pub(crate) pending_auth: Option<(String, Action)>,
}

pub fn notify(app: &mut App, r: Reply) {
    if r.ok {
        if !r.message.is_empty() {
            app.toast(r.message);
        }
    } else {
        app.error(r.message);
    }
}

impl App {
    pub fn new(snap: Snapshot, demo: Option<demo::Demo>) -> Self {
        Self {
            snap,
            tab: Tab::Overview,
            last_tab: Tab::Overview,
            modals: vec![],
            toast: None,
            busy: None,
            demo,
            quit: false,
            overview: Default::default(),
            proxies: Default::default(),
            activity: Default::default(),
            config: Default::default(),
            history: Default::default(),
            traffic: VecDeque::new(),
            logs: vec![],
            cores: None,
            poll_error: String::new(),
            outbox: VecDeque::new(),
            pending_auth: None,
        }
    }
    // ---- data access -------------------------------------------------------
    pub fn doc(&self) -> &Value {
        static EMPTY: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
        self.snap
            .store
            .native
            .as_ref()
            .unwrap_or_else(|| EMPTY.get_or_init(|| json!({})))
    }
    pub fn label(&self, tag: &str) -> String {
        labels::label(&self.snap.store, tag)
    }
    pub fn tun_configured(&self) -> bool {
        native::array(self.doc(), "/inbounds")
            .iter()
            .any(|i| i["type"] == "tun")
    }
    pub fn system_proxy_on(&self) -> bool {
        self.snap.store.settings.mode == "system"
    }
    pub fn live_group(&self, tag: &str) -> Option<&api::Group> {
        self.snap.groups.group.iter().find(|g| g.tag == tag)
    }

    // ---- feedback ----------------------------------------------------------
    pub fn toast(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast {
            text: crate::model::clean(&text.into()),
            error: false,
            at: Instant::now(),
        });
    }
    pub fn error(&mut self, text: impl Into<String>) {
        let text = runtime::redact_error(&text.into(), &self.snap.store);
        self.toast = Some(Toast {
            text: crate::model::clean(&text.replace('\n', " · ")),
            error: true,
            at: Instant::now(),
        });
    }

    // ---- requests ----------------------------------------------------------
    /// Queue a manager request; `then` runs with the reply (also on failure).
    pub fn request(&mut self, action: Action, then: Then) {
        self.outbox.push_back((action, then, None));
    }
    pub fn request_busy(&mut self, action: Action, busy: &str, then: Then) {
        self.outbox.push_back((action, then, Some(busy.into())));
    }
    pub fn send(&mut self, action: Action) {
        self.request(action, Box::new(notify));
    }
    pub fn push(&mut self, m: impl Modal + 'static) {
        self.modals.push(Box::new(m));
    }
    pub fn top<T: 'static>(&mut self) -> Option<&mut T> {
        self.modals.last_mut()?.as_any().downcast_mut::<T>()
    }
    fn finish(&mut self, mut reply: Reply, then: Then) {
        self.busy = None;
        if let Some(s) = reply.snapshot.take() {
            self.observe(s);
        }
        if let Some(c) = reply.connections.take() {
            self.history.observe(c, self.snap.connected);
        }
        if let Some(c) = reply.cores.take() {
            self.cores = Some(c);
        }
        if reply.needs_auth {
            let kind = if reply.auth_kind.is_empty() {
                "tun".to_string()
            } else {
                reply.auth_kind.clone()
            };
            let after = reply.after_auth.clone().unwrap_or(Action::Connect);
            self.push(flows::Auth::new(kind, after, reply.message.clone()));
            return;
        }
        then(self, reply);
    }
    pub fn observe(&mut self, s: Snapshot) {
        if s.connected && s.api_ready {
            self.traffic.push_back((s.status.downlink, s.status.uplink));
            while self.traffic.len() > 240 {
                self.traffic.pop_front();
            }
        } else if !s.connected {
            self.traffic.clear();
        }
        self.snap = s;
    }
    fn receive_poll(&mut self, r: Result<Reply>) {
        match r {
            Ok(mut r) => {
                if let Some(s) = r.snapshot.take() {
                    self.observe(s);
                }
                if r.ok {
                    self.poll_error.clear();
                    if let Some(c) = r.connections.take() {
                        self.history.observe(c, self.snap.connected);
                    }
                    if let Some(text) = r.config.take() {
                        self.logs = text.lines().rev().take(2000).map(str::to_string).collect();
                        self.logs.reverse();
                    }
                } else {
                    self.poll_error = crate::model::clean(&r.message);
                }
            }
            Err(e) => self.poll_error = crate::model::clean(&e.to_string()),
        }
    }
    /// Offline demo and tests: resolve queued requests synchronously.
    pub fn drain(&mut self) {
        while let Some((action, then, _)) = self.outbox.pop_front() {
            let Some(mut demo) = self.demo.take() else {
                self.outbox.push_front((action, then, None));
                return;
            };
            let reply = demo.handle(self, action);
            self.demo = Some(demo);
            self.finish(reply, then);
        }
    }

    // ---- keys --------------------------------------------------------------
    pub fn key(&mut self, k: KeyEvent) {
        if k.code == K::Char('c') && k.modifiers.contains(M::CONTROL) {
            self.quit = true;
            return;
        }
        if self.busy.is_some() {
            return;
        }
        if let Some(mut m) = self.modals.pop() {
            let outcome = m.key(self, k);
            self.apply(m, outcome);
            return;
        }
        let capturing = match self.tab {
            Tab::Overview => false,
            Tab::Proxies => proxies::capturing(self),
            Tab::Activity => activity::capturing(self),
            Tab::Config => config::capturing(self),
        };
        if !capturing && self.global_key(k) {
            return;
        }
        match self.tab {
            Tab::Overview => overview::key(self, k),
            Tab::Proxies => proxies::key(self, k),
            Tab::Activity => activity::key(self, k),
            Tab::Config => config::key(self, k),
        }
    }
    pub fn paste(&mut self, s: &str) {
        if let Some(m) = self.modals.last_mut() {
            m.paste(s);
        } else {
            match self.tab {
                Tab::Proxies => proxies::paste(self, s),
                Tab::Activity => activity::paste(self, s),
                Tab::Config => config::paste(self, s),
                Tab::Overview => {}
            }
        }
    }
    pub fn apply(&mut self, m: Box<dyn Modal>, outcome: Outcome) {
        match outcome {
            Outcome::Stay => self.modals.push(m),
            Outcome::Close => {}
            Outcome::Push(next) => {
                self.modals.push(m);
                self.modals.push(next);
            }
            Outcome::Replace(next) => self.modals.push(next),
        }
    }
    pub fn go(&mut self, tab: Tab) {
        if tab != self.tab {
            if self.tab != Tab::Config {
                self.last_tab = self.tab;
            }
            self.tab = tab;
            if tab == Tab::Config {
                config::entered(self);
            }
        }
    }
    fn global_key(&mut self, k: KeyEvent) -> bool {
        match k.code {
            K::Char('q') => self.quit = true,
            K::Char('1') => self.go(Tab::Overview),
            K::Char('2') => self.go(Tab::Proxies),
            K::Char('3') => self.go(Tab::Activity),
            K::Char(',') => {
                if self.tab == Tab::Config {
                    self.go(self.last_tab)
                } else {
                    self.go(Tab::Config)
                }
            }
            K::Char('?') => self.push(flows::help(self)),
            K::Char('s') => chrome::start_stop(self),
            K::Char('m') => self.push(chrome::ModeMenu::new(self)),
            K::Char('t') => chrome::toggle_tun(self),
            K::Char('p') => chrome::toggle_system_proxy(self),
            K::Char('A') => flows::review_apply(self),
            _ => return false,
        }
        true
    }
}

// ---- drawing -----------------------------------------------------------------
pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let [header, body, hints, controls] = chrome::layout(area);
    chrome::header(f, header, app);
    match app.tab {
        Tab::Overview => overview::draw(f, body, app),
        Tab::Proxies => proxies::draw(f, body, app),
        Tab::Activity => activity::draw(f, body, app),
        Tab::Config => config::draw(f, body, app),
    }
    let page_hints = match app.modals.last() {
        Some(m) => m.hints(),
        None => match app.tab {
            Tab::Overview => overview::hints(app),
            Tab::Proxies => proxies::hints(app),
            Tab::Activity => activity::hints(app),
            Tab::Config => config::hints(app),
        },
    };
    for m in &app.modals {
        m.draw(f, body, app);
    }
    chrome::hint_line(f, hints, app, &page_hints);
    chrome::controls(f, controls, app);
}

// ---- terminal lifecycle ----------------------------------------------------------
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

enum Job {
    Foreground(u64, Action),
    Background(Action),
}

pub fn run(dir: PathBuf, demo: bool) -> Result<()> {
    ensure!(
        io::stdin().is_terminal(),
        "Interactive terminal required; --preview renders a static preview"
    );
    let mut app = if demo {
        let d = demo::Demo::new()?;
        App::new(d.snapshot(), Some(d))
    } else {
        runtime::ensure_daemon(&dir)?;
        let snap = runtime::request(&dir, Action::Snapshot)?
            .snapshot
            .context("No manager snapshot")?;
        ensure!(
            snap.manager_protocol >= runtime::PROTOCOL,
            "An older sing manager is still running. Run `sing --shutdown` (this stops its core; saved data stays), then open sing again."
        );
        App::new(snap, None)
    };
    let (tx, jobs) = mpsc::channel::<Job>();
    let (done, results) = mpsc::channel::<(Option<u64>, Result<Reply>)>();
    let worker_dir = dir.clone();
    std::thread::spawn(move || {
        for job in jobs {
            let (id, action) = match job {
                Job::Foreground(id, a) => (Some(id), a),
                Job::Background(a) => (None, a),
            };
            let snapshot_only = matches!(action, Action::Snapshot);
            let mut r = runtime::request(&worker_dir, action);
            if !snapshot_only {
                if let Ok(r) = &mut r {
                    if let Ok(s) = runtime::request(&worker_dir, Action::Snapshot) {
                        r.snapshot = s.snapshot;
                    }
                }
            }
            if done.send((id, r)).is_err() {
                break;
            }
        }
    });
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |i| {
        let _ = leave();
        hook(i);
    }));
    enter()?;
    let _guard = Guard;
    let mut term = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut waiting: HashMap<u64, Then> = HashMap::new();
    let mut next_id = 0u64;
    let mut polling = false;
    let mut last_poll = Instant::now() - Duration::from_secs(10);
    let mut last_logs = Instant::now() - Duration::from_secs(10);
    let mut last_demo_tick = Instant::now();
    while !app.quit {
        while let Ok((id, r)) = results.try_recv() {
            match id.and_then(|id| waiting.remove(&id)) {
                Some(then) => {
                    let reply = r.unwrap_or_else(|e| {
                        let mut r: Reply = serde_json::from_value(
                            json!({"ok":false,"message":"","needs_auth":false}),
                        )
                        .unwrap();
                        r.message = e.to_string();
                        r
                    });
                    app.finish(reply, then);
                }
                None => {
                    polling = false;
                    app.receive_poll(r);
                }
            }
        }
        if let Some(t) = &app.toast {
            if t.at.elapsed() > Duration::from_secs(if t.error { 12 } else { 5 }) {
                app.toast = None;
            }
        }
        term.draw(|f| draw(f, &app))?;
        if event::poll(Duration::from_millis(120))? {
            match event::read()? {
                Event::Key(k) if k.kind != event::KeyEventKind::Release => app.key(k),
                Event::Paste(s) => app.paste(&s),
                _ => {}
            }
        }
        if let Some((kind, after)) = app.pending_auth.take() {
            leave()?;
            let result = if kind == "system" {
                crate::system_proxy::helper::authorize(&dir)
            } else {
                runtime::authorize_tun(&dir, &app.snap.core)
            };
            enter()?;
            term.clear()?;
            match result {
                Ok(()) => app.request_busy(after, "Starting", Box::new(notify)),
                Err(e) => app.error(e.to_string()),
            }
        }
        if app.demo.is_some() {
            app.drain();
            if last_demo_tick.elapsed() > Duration::from_secs(1) {
                last_demo_tick = Instant::now();
                let mut d = app.demo.take().unwrap();
                d.tick(&mut app);
                app.demo = Some(d);
            }
            continue;
        }
        while let Some((action, then, busy)) = app.outbox.pop_front() {
            next_id += 1;
            waiting.insert(next_id, then);
            if let Some(label) = busy {
                app.busy = Some((label, Instant::now()));
            }
            tx.send(Job::Foreground(next_id, action))?;
        }
        let live = matches!(app.tab, Tab::Overview | Tab::Activity);
        let every = if live { 1 } else { 2 };
        if !polling && waiting.is_empty() && last_poll.elapsed() > Duration::from_secs(every) {
            polling = true;
            last_poll = Instant::now();
            tx.send(Job::Background(if live && app.snap.connected {
                Action::Connections
            } else {
                Action::Snapshot
            }))?;
        } else if !polling
            && waiting.is_empty()
            && app.tab == Tab::Activity
            && app.activity.section == activity::Section::Logs
            && last_logs.elapsed() > Duration::from_secs(2)
        {
            polling = true;
            last_logs = Instant::now();
            tx.send(Job::Background(Action::Logs))?;
        }
    }
    drop(term);
    drop(_guard);
    println!(
        "{}",
        if app.snap.connected {
            "sing closed. The core keeps running in the background; `sing --disconnect` stops it."
        } else {
            "sing closed."
        }
    );
    Ok(())
}

pub fn render(app: &App, w: u16, h: u16) -> Vec<String> {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| draw(f, app)).unwrap();
    t.backend()
        .buffer()
        .content
        .chunks(w as usize)
        .map(|row| {
            let mut line = String::new();
            let mut skip = 0;
            for c in row {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                line.push_str(c.symbol());
                skip = text::width(c.symbol()).saturating_sub(1);
            }
            line
        })
        .collect()
}

pub fn preview() -> Result<()> {
    let d = demo::Demo::running()?;
    let mut app = App::new(d.snapshot(), Some(d));
    for _ in 0..30 {
        let mut d = app.demo.take().unwrap();
        d.tick(&mut app);
        app.demo = Some(d);
    }
    for line in render(&app, 110, 32) {
        println!("{}", line.trim_end());
    }
    Ok(())
}
