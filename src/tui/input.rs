//! Single and multi-line text input with a byte cursor on char boundaries.
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};
use ratatui::text::{Line, Span};
use ratatui::style::{Modifier, Style};
use super::{text, theme};

#[derive(Clone)]
pub struct Input {
    pub value: String,
    pub cursor: usize,
}
impl Input {
    pub fn new(value: String) -> Self {
        let cursor = value.len();
        Self { value, cursor }
    }
    pub fn insert(&mut self, s: &str) {
        let s: String = s
            .chars()
            .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
            .collect();
        self.value.insert_str(self.cursor, &s);
        self.cursor += s.len();
    }
    pub fn key(&mut self, k: KeyEvent, multiline: bool) {
        match k.code {
            K::Left => {
                self.cursor = self.value[..self.cursor]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(i, _)| i)
            }
            K::Right => {
                if let Some(c) = self.value[self.cursor..].chars().next() {
                    self.cursor += c.len_utf8();
                }
            }
            K::Home => self.cursor = self.value[..self.cursor].rfind('\n').map_or(0, |i| i + 1),
            K::End => {
                self.cursor += self.value[self.cursor..]
                    .find('\n')
                    .unwrap_or(self.value.len() - self.cursor)
            }
            K::Backspace if self.cursor > 0 => {
                let i = self.value[..self.cursor]
                    .char_indices()
                    .next_back()
                    .unwrap()
                    .0;
                self.value.replace_range(i..self.cursor, "");
                self.cursor = i;
            }
            K::Delete if self.cursor < self.value.len() => {
                let n = self.value[self.cursor..].chars().next().unwrap().len_utf8();
                self.value.replace_range(self.cursor..self.cursor + n, "");
            }
            K::Enter if multiline => self.insert("\n"),
            K::Up | K::Down if multiline => {
                let start = self.value[..self.cursor].rfind('\n').map_or(0, |i| i + 1);
                let col = self.value[start..self.cursor].chars().count();
                let target = if k.code == K::Up {
                    if start == 0 {
                        return;
                    }
                    self.value[..start - 1].rfind('\n').map_or(0, |i| i + 1)
                } else {
                    let Some(n) = self.value[self.cursor..].find('\n') else {
                        return;
                    };
                    self.cursor + n + 1
                };
                let line = self.value[target..].split('\n').next().unwrap_or("");
                self.cursor = target + line.char_indices().nth(col).map_or(line.len(), |(i, _)| i);
            }
            K::Char('u') if k.modifiers.contains(M::CONTROL) => {
                self.value.clear();
                self.cursor = 0;
            }
            K::Char(c) if !k.modifiers.intersects(M::CONTROL | M::ALT) => {
                self.insert(&c.to_string())
            }
            _ => {}
        }
    }
}

impl Input {
    pub fn empty() -> Self {
        Self::new(String::new())
    }
    pub fn set(&mut self, value: impl Into<String>) {
        *self = Self::new(value.into());
    }
    /// Render one line with a visible block cursor, scrolled so the cursor stays
    /// inside `max` columns. `secret` masks the value.
    pub fn line(&self, max: usize, focused: bool, secret: bool) -> Line<'static> {
        let value: String = if secret {
            "•".repeat(self.value.chars().count().min(24))
        } else {
            self.value.replace('\n', "⏎")
        };
        let cursor_chars = if secret {
            self.value[..self.cursor].chars().count().min(24)
        } else {
            self.value[..self.cursor].chars().count()
        };
        let chars: Vec<char> = value.chars().collect();
        let before: String = chars[..cursor_chars.min(chars.len())].iter().collect();
        let at: String = chars.get(cursor_chars).map(|c| c.to_string()).unwrap_or(" ".into());
        let after: String = chars.iter().skip(cursor_chars + 1).collect();
        if !focused {
            return Line::from(Span::styled(text::fit(&value, max), theme::s(theme::text())));
        }
        // Keep the tail of `before` visible.
        let room = max.saturating_sub(text::width(&at) + 1);
        let mut shown = before.clone();
        while text::width(&shown) > room {
            shown.remove(0);
        }
        let rest = max.saturating_sub(text::width(&shown) + text::width(&at));
        Line::from(vec![
            Span::styled(shown, theme::s(theme::text())),
            Span::styled(at, Style::default().fg(theme::panel()).bg(theme::accent()).add_modifier(Modifier::BOLD)),
            Span::styled(text::fit(&after, rest), theme::s(theme::text())),
        ])
    }
}
