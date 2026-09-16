use super::{App, modal::{any, Modal, Outcome, TextView}};
use crate::runtime::Action;
use crossterm::event::{KeyCode as K, KeyEvent};
use ratatui::{layout::Rect, Frame};
pub struct Auth { kind: String, after: Action, message: String }
impl Auth { pub fn new(kind: String, after: Action, message: String) -> Self { Self { kind, after, message } } }
impl Modal for Auth {
    any!();
    fn key(&mut self, app: &mut App, k: KeyEvent) -> Outcome {
        match k.code {
            K::Enter => { app.pending_auth = Some((self.kind.clone(), self.after.clone())); Outcome::Close }
            K::Esc => Outcome::Close,
            _ => Outcome::Stay,
        }
    }
    fn draw(&self, _: &mut Frame, _: Rect, _: &App) { let _ = &self.message; }
}
pub fn help(_: &App) -> TextView { TextView::plain("Help", "") }
pub fn review_apply(_: &mut App) {}
pub fn import_subscription(_: &mut App) {}
