use super::App;
use crossterm::event::KeyEvent;
use ratatui::{layout::Rect, Frame};
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum Section { #[default] Requests, Apps, Logs }
#[derive(Default)]
pub struct State { pub section: Section }
pub fn capturing(_: &App) -> bool { false }
pub fn key(_: &mut App, _: KeyEvent) {}
pub fn paste(_: &mut App, _: &str) {}
pub fn draw(_: &mut Frame, _: Rect, _: &App) {}
pub fn hints(_: &App) -> Vec<(&'static str, &'static str)> { vec![] }
