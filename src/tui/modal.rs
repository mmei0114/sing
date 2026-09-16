//! Stacked dialogs. Every dialog speaks the same keys: ↑↓ move, Enter choose,
//! Esc back, / filter where lists are long.
use super::{input::Input, text, theme, App};
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};
use ratatui::{
    layout::{Constraint, Flex, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
    Frame,
};
use std::any::Any;

pub enum Outcome {
    Stay,
    Close,
    Push(Box<dyn Modal>),
    Replace(Box<dyn Modal>),
}
pub trait Modal: Any {
    fn key(&mut self, app: &mut App, k: KeyEvent) -> Outcome;
    fn draw(&self, f: &mut Frame, area: Rect, app: &App);
    fn as_any(&mut self) -> &mut dyn Any;
    fn paste(&mut self, _s: &str) {}
    fn hints(&self) -> Vec<(&'static str, &'static str)> {
        vec![("↑↓", "move"), ("enter", "choose"), ("esc", "back")]
    }
}
macro_rules! any {
    () => {
        fn as_any(&mut self) -> &mut dyn std::any::Any {
            self
        }
    };
}
pub(crate) use any;

pub fn is_save(k: &KeyEvent) -> bool {
    k.code == K::F(2) || (k.code == K::Char('s') && k.modifiers.contains(M::CONTROL))
}

/// Centered popup of at most `w`×`h`, clamped to the body.
pub fn popup(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width.saturating_sub(2)).max(area.width.min(20));
    let h = h.min(area.height);
    let [r] = Layout::vertical([Constraint::Length(h)])
        .flex(Flex::Center)
        .areas(area);
    let [r] = Layout::horizontal([Constraint::Length(w)])
        .flex(Flex::Center)
        .areas(r);
    r
}
/// Clear, frame and return the inner area.
pub fn frame(f: &mut Frame, r: Rect, title: &str, right: &str) -> Rect {
    f.render_widget(Clear, r);
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::s(theme::faint()))
        .style(Style::default().bg(theme::panel()))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(title.to_string(), theme::bold(theme::text())),
            Span::raw(" "),
        ]));
    if !right.is_empty() {
        block = block.title(
            Line::from(Span::styled(format!(" {right} "), theme::s(theme::dim()))).right_aligned(),
        );
    }
    let inner = block.inner(r);
    f.render_widget(block, r);
    Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(2),
        ..inner
    }
}
/// A row with a left accent bar when selected.
pub fn row(selected: bool, spans: Vec<Span<'static>>) -> Line<'static> {
    let mut all = vec![Span::styled(
        if selected { "▌" } else { " " },
        theme::s(theme::accent()),
    )];
    all.extend(spans);
    let line = Line::from(all);
    if selected {
        line.style(theme::selected_row())
    } else {
        line
    }
}
/// First visible index so `selected` stays on screen.
pub fn scroll(selected: usize, height: usize, len: usize) -> usize {
    if height == 0 || len <= height {
        return 0;
    }
    selected
        .saturating_sub(height / 2)
        .min(len.saturating_sub(height))
}
pub fn buttons(labels: &[&str], focus: Option<usize>) -> Line<'static> {
    let mut spans = vec![];
    for (i, l) in labels.iter().enumerate() {
        let on = focus == Some(i);
        spans.push(Span::styled(
            format!(" {l} "),
            if on {
                Style::default()
                    .fg(theme::panel())
                    .bg(theme::accent())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::text()).bg(theme::selection())
            },
        ));
        spans.push(Span::raw("  "));
    }
    Line::from(spans)
}

// ---- Confirm ---------------------------------------------------------------------
pub type Act = Box<dyn FnOnce(&mut App)>;
pub struct Confirm {
    title: String,
    body: String,
    yes: String,
    danger: bool,
    focus: usize,
    on_yes: Option<Act>,
}
impl Confirm {
    pub fn new(title: &str, body: impl Into<String>, yes: &str, on_yes: Act) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            yes: yes.into(),
            danger: false,
            focus: 0,
            on_yes: Some(on_yes),
        }
    }
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self.focus = 1;
        self
    }
}
impl Modal for Confirm {
    any!();
    fn key(&mut self, app: &mut App, k: KeyEvent) -> Outcome {
        match k.code {
            K::Left | K::Right | K::Tab | K::BackTab => self.focus = 1 - self.focus,
            K::Char('y') => {
                (self.on_yes.take().unwrap())(app);
                return Outcome::Close;
            }
            K::Enter if self.focus == 0 => {
                (self.on_yes.take().unwrap())(app);
                return Outcome::Close;
            }
            K::Enter | K::Esc | K::Char('n') => return Outcome::Close,
            _ => {}
        }
        Outcome::Stay
    }
    fn draw(&self, f: &mut Frame, area: Rect, _: &App) {
        let lines = textwrap_height(&self.body, 56);
        let r = popup(area, 62, lines as u16 + 6);
        let inner = frame(f, r, &self.title, "");
        let [body, _, buttons_area] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(inner);
        f.render_widget(
            Paragraph::new(self.body.clone())
                .style(theme::s(theme::text()))
                .wrap(Wrap { trim: false }),
            body,
        );
        f.render_widget(
            buttons(&[&self.yes, "Cancel"], Some(self.focus)),
            buttons_area,
        );
        if self.danger {
            let _ = theme::bad();
        }
    }
    fn hints(&self) -> Vec<(&'static str, &'static str)> {
        vec![("←→", "choose"), ("enter", "confirm"), ("esc", "cancel")]
    }
}
pub fn textwrap_height(s: &str, w: usize) -> usize {
    s.lines()
        .map(|l| (text::width(l).max(1) + w - 1) / w)
        .sum::<usize>()
        .max(1)
}

// ---- Text viewer --------------------------------------------------------------------
pub struct TextView {
    pub title: String,
    pub body: Vec<Line<'static>>,
    pub scroll: u16,
}
impl TextView {
    pub fn plain(title: &str, body: &str) -> Self {
        Self {
            title: title.into(),
            body: body
                .lines()
                .map(|l| Line::styled(l.to_string(), theme::s(theme::text())))
                .collect(),
            scroll: 0,
        }
    }
}
impl Modal for TextView {
    any!();
    fn key(&mut self, _: &mut App, k: KeyEvent) -> Outcome {
        match k.code {
            K::Esc | K::Enter | K::Char('q') => return Outcome::Close,
            K::Down | K::Char('j') => self.scroll = self.scroll.saturating_add(1),
            K::Up | K::Char('k') => self.scroll = self.scroll.saturating_sub(1),
            K::PageDown | K::Char(' ') => self.scroll = self.scroll.saturating_add(10),
            K::PageUp => self.scroll = self.scroll.saturating_sub(10),
            _ => {}
        }
        Outcome::Stay
    }
    fn draw(&self, f: &mut Frame, area: Rect, _: &App) {
        let r = popup(area, 96, area.height);
        let inner = frame(f, r, &self.title, "");
        f.render_widget(
            Paragraph::new(self.body.clone())
                .wrap(Wrap { trim: false })
                .scroll((self.scroll, 0)),
            inner,
        );
    }
    fn hints(&self) -> Vec<(&'static str, &'static str)> {
        vec![("↑↓", "scroll"), ("esc", "close")]
    }
}

// ---- Picker ---------------------------------------------------------------------------
#[derive(Clone)]
pub struct Choice {
    pub value: String,
    pub label: String,
    pub detail: String,
}
impl Choice {
    pub fn new(value: impl Into<String>, label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            detail: detail.into(),
        }
    }
}
pub type Picked = Box<dyn FnOnce(&mut App, Vec<String>)>;
/// Single or multi selection with type-to-filter.
pub struct Picker {
    title: String,
    choices: Vec<Choice>,
    chosen: Vec<String>,
    multi: bool,
    query: Input,
    selected: usize,
    on_pick: Option<Picked>,
}
impl Picker {
    pub fn single(title: &str, choices: Vec<Choice>, current: &str, on_pick: Picked) -> Self {
        let selected = choices.iter().position(|c| c.value == current).unwrap_or(0);
        Self {
            title: title.into(),
            chosen: vec![current.into()],
            choices,
            multi: false,
            query: Input::empty(),
            selected,
            on_pick: Some(on_pick),
        }
    }
    pub fn multi(title: &str, choices: Vec<Choice>, chosen: Vec<String>, on_pick: Picked) -> Self {
        Self {
            title: title.into(),
            choices,
            chosen,
            multi: true,
            query: Input::empty(),
            selected: 0,
            on_pick: Some(on_pick),
        }
    }
    fn visible(&self) -> Vec<&Choice> {
        let q = self.query.value.to_lowercase();
        self.choices
            .iter()
            .filter(|c| {
                q.is_empty()
                    || format!("{} {} {}", c.label, c.value, c.detail)
                        .to_lowercase()
                        .contains(&q)
            })
            .collect()
    }
}
impl Modal for Picker {
    any!();
    fn key(&mut self, app: &mut App, k: KeyEvent) -> Outcome {
        let n = self.visible().len();
        match k.code {
            K::Esc => return Outcome::Close,
            K::Down => self.selected = (self.selected + 1).min(n.saturating_sub(1)),
            K::Up => self.selected = self.selected.saturating_sub(1),
            K::PageDown => self.selected = (self.selected + 10).min(n.saturating_sub(1)),
            K::PageUp => self.selected = self.selected.saturating_sub(10),
            K::Char(' ') if self.multi => {
                if let Some(c) = self.visible().get(self.selected) {
                    let v = c.value.clone();
                    if let Some(i) = self.chosen.iter().position(|x| x == &v) {
                        self.chosen.remove(i);
                    } else {
                        self.chosen.push(v);
                    }
                }
            }
            K::Enter => {
                let result = if self.multi {
                    // Keep existing order, append new choices in list order.
                    let mut out: Vec<String> = self
                        .chosen
                        .iter()
                        .filter(|v| self.choices.iter().any(|c| &c.value == *v))
                        .cloned()
                        .collect();
                    if out.is_empty() {
                        if let Some(c) = self.visible().get(self.selected) {
                            out.push(c.value.clone());
                        }
                    }
                    out
                } else {
                    match self.visible().get(self.selected) {
                        Some(c) => vec![c.value.clone()],
                        None => return Outcome::Stay,
                    }
                };
                (self.on_pick.take().unwrap())(app, result);
                return Outcome::Close;
            }
            _ => {
                let before = self.query.value.clone();
                self.query.key(k, false);
                if before != self.query.value {
                    self.selected = 0;
                }
            }
        }
        Outcome::Stay
    }
    fn paste(&mut self, s: &str) {
        self.query.insert(s);
        self.selected = 0;
    }
    fn draw(&self, f: &mut Frame, area: Rect, _: &App) {
        let visible = self.visible();
        let h = (visible.len() as u16 + 5).clamp(8, area.height);
        let r = popup(area, 72, h);
        let right = if self.multi {
            format!("{} chosen", self.chosen.len())
        } else {
            String::new()
        };
        let inner = frame(f, r, &self.title, &right);
        let [search, list] =
            Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(inner);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("/ ", theme::s(theme::dim())),
                if self.query.value.is_empty() {
                    Span::styled("type to filter", theme::s(theme::faint()))
                } else {
                    Span::styled(self.query.value.clone(), theme::s(theme::text()))
                },
            ])),
            search,
        );
        let height = list.height as usize;
        let start = scroll(self.selected, height, visible.len());
        let w = list.width as usize;
        let lines: Vec<Line> = visible
            .iter()
            .enumerate()
            .skip(start)
            .take(height)
            .map(|(i, c)| {
                let mark = if self.multi {
                    if self.chosen.contains(&c.value) {
                        "◉ "
                    } else {
                        "○ "
                    }
                } else if self.chosen.first() == Some(&c.value) {
                    "● "
                } else {
                    "  "
                };
                let label_w = (w.saturating_sub(4)).min(34.max(w.saturating_sub(4) / 2));
                row(
                    i == self.selected,
                    vec![
                        Span::styled(mark, theme::s(theme::accent())),
                        Span::styled(text::cell(&c.label, label_w), theme::s(theme::text())),
                        Span::styled(
                            text::fit(&c.detail, w.saturating_sub(label_w + 4)),
                            theme::s(theme::dim()),
                        ),
                    ],
                )
            })
            .collect();
        if lines.is_empty() {
            f.render_widget(
                Paragraph::new(Span::styled("No matches", theme::s(theme::dim()))),
                list,
            );
        } else {
            f.render_widget(Paragraph::new(lines), list);
        }
    }
    fn hints(&self) -> Vec<(&'static str, &'static str)> {
        if self.multi {
            vec![("space", "toggle"), ("enter", "done"), ("type", "filter"), ("esc", "cancel")]
        } else {
            vec![("↑↓", "move"), ("enter", "choose"), ("type", "filter"), ("esc", "cancel")]
        }
    }
}

// ---- Prompt ----------------------------------------------------------------------------
pub type Entered = Box<dyn FnMut(&mut App, String) -> Result<(), String>>;
pub struct Prompt {
    title: String,
    help: String,
    input: Input,
    secret: bool,
    error: String,
    on_enter: Entered,
}
impl Prompt {
    pub fn new(title: &str, help: &str, value: &str, on_enter: Entered) -> Self {
        Self {
            title: title.into(),
            help: help.into(),
            input: Input::new(value.into()),
            secret: false,
            error: String::new(),
            on_enter,
        }
    }
    pub fn secret(mut self) -> Self {
        self.secret = true;
        self
    }
}
impl Modal for Prompt {
    any!();
    fn key(&mut self, app: &mut App, k: KeyEvent) -> Outcome {
        match k.code {
            K::Esc => return Outcome::Close,
            K::Enter => {
                return match (self.on_enter)(app, self.input.value.clone()) {
                    Ok(()) => Outcome::Close,
                    Err(e) => {
                        self.error = e;
                        Outcome::Stay
                    }
                }
            }
            _ => self.input.key(k, false),
        }
        Outcome::Stay
    }
    fn paste(&mut self, s: &str) {
        self.input.insert(s.trim_end_matches('\n'));
    }
    fn draw(&self, f: &mut Frame, area: Rect, _: &App) {
        let help_h = textwrap_height(&self.help, 62) as u16;
        let r = popup(area, 70, help_h + 7);
        let inner = frame(f, r, &self.title, "");
        let [help, _, field, error] = Layout::vertical([
            Constraint::Length(help_h),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .areas(inner);
        f.render_widget(
            Paragraph::new(self.help.clone())
                .style(theme::s(theme::dim()))
                .wrap(Wrap { trim: false }),
            help,
        );
        f.render_widget(
            Paragraph::new(self.input.line(field.width as usize, true, self.secret)),
            field,
        );
        f.render_widget(
            Paragraph::new(Span::styled(self.error.clone(), theme::s(theme::bad())))
                .wrap(Wrap { trim: false }),
            error,
        );
    }
    fn hints(&self) -> Vec<(&'static str, &'static str)> {
        vec![("enter", "ok"), ("^U", "clear"), ("esc", "cancel")]
    }
}

// ---- JSON / text area -------------------------------------------------------------------
pub type Submitted = Box<dyn FnMut(&mut App, String) -> Result<(), String>>;
pub struct TextArea {
    title: String,
    input: Input,
    error: String,
    on_save: Submitted,
}
impl TextArea {
    pub fn new(title: &str, value: String, on_save: Submitted) -> Self {
        let mut input = Input::new(value);
        input.cursor = 0;
        Self {
            title: title.into(),
            input,
            error: String::new(),
            on_save,
        }
    }
}
impl Modal for TextArea {
    any!();
    fn key(&mut self, app: &mut App, k: KeyEvent) -> Outcome {
        if is_save(&k) {
            return match (self.on_save)(app, self.input.value.clone()) {
                Ok(()) => Outcome::Close,
                Err(e) => {
                    self.error = e;
                    Outcome::Stay
                }
            };
        }
        match k.code {
            K::Esc => return Outcome::Close,
            K::Tab => self.input.insert("  "),
            _ => self.input.key(k, true),
        }
        Outcome::Stay
    }
    fn paste(&mut self, s: &str) {
        self.input.insert(s);
    }
    fn draw(&self, f: &mut Frame, area: Rect, _: &App) {
        let r = popup(area, area.width, area.height);
        let inner = frame(f, r, &self.title, "JSON");
        let [body, error] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(if self.error.is_empty() { 0 } else { 2 })])
                .areas(inner);
        let before = &self.input.value[..self.input.cursor];
        let line = before.matches('\n').count();
        let col = before.rsplit('\n').next().map(|s| s.chars().count()).unwrap_or(0);
        let height = body.height as usize;
        let top = line.saturating_sub(height.saturating_sub(1));
        let lines: Vec<Line> = self
            .input
            .value
            .split('\n')
            .enumerate()
            .skip(top)
            .take(height)
            .map(|(i, l)| {
                let number = Span::styled(format!("{:>4} ", i + 1), theme::s(theme::faint()));
                if i != line {
                    return Line::from(vec![number, Span::styled(l.to_string(), theme::s(theme::text()))]);
                }
                let chars: Vec<char> = l.chars().collect();
                let a: String = chars[..col.min(chars.len())].iter().collect();
                let c: String = chars.get(col).map(|c| c.to_string()).unwrap_or(" ".into());
                let b: String = chars.iter().skip(col + 1).collect();
                Line::from(vec![
                    number,
                    Span::styled(a, theme::s(theme::text())),
                    Span::styled(c, Style::default().bg(theme::accent()).fg(theme::panel())),
                    Span::styled(b, theme::s(theme::text())),
                ])
            })
            .collect();
        let horizontal = (col + 6).saturating_sub(body.width as usize) as u16;
        f.render_widget(Paragraph::new(lines).scroll((0, horizontal)), body);
        f.render_widget(
            Paragraph::new(Span::styled(self.error.clone(), theme::s(theme::bad())))
                .wrap(Wrap { trim: true }),
            error,
        );
    }
    fn hints(&self) -> Vec<(&'static str, &'static str)> {
        vec![("^S", "save"), ("esc", "cancel")]
    }
}
