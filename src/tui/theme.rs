//! Ink violet, soft white and restrained lavender. Truecolor where supported;
//! 256-color entries elsewhere (Terminal.app, tmux without RGB).
use ratatui::style::{Color, Modifier, Style};
use std::sync::OnceLock;

fn truecolor() -> bool {
    static TRUE: OnceLock<bool> = OnceLock::new();
    *TRUE.get_or_init(|| {
        std::env::var("COLORTERM")
            .map(|v| v.contains("truecolor") || v.contains("24bit"))
            .unwrap_or(false)
    })
}
const fn pick(rgb: (u8, u8, u8), indexed: u8) -> (Color, Color) {
    (Color::Rgb(rgb.0, rgb.1, rgb.2), Color::Indexed(indexed))
}
fn c(pair: (Color, Color)) -> Color {
    if truecolor() {
        pair.0
    } else {
        pair.1
    }
}

pub fn accent() -> Color {
    c(pick((196, 173, 240), 183))
}
pub fn text() -> Color {
    c(pick((229, 225, 237), 254))
}
pub fn dim() -> Color {
    c(pick((158, 151, 174), 247))
}
pub fn faint() -> Color {
    c(pick((130, 120, 148), 244))
}
pub fn good() -> Color {
    c(pick((156, 202, 172), 150))
}
pub fn warn() -> Color {
    c(pick((231, 192, 130), 180))
}
pub fn bad() -> Color {
    c(pick((235, 145, 162), 211))
}
pub fn info() -> Color {
    c(pick((162, 180, 224), 146))
}
pub fn violet() -> Color {
    c(pick((186, 169, 208), 182))
}
pub fn selection() -> Color {
    c(pick((52, 43, 70), 237))
}
pub fn panel() -> Color {
    c(pick((31, 26, 43), 235))
}
pub fn background() -> Color {
    c(pick((23, 19, 32), 234))
}

pub fn s(fg: Color) -> Style {
    Style::default().fg(fg)
}
pub fn bold(fg: Color) -> Style {
    Style::default().fg(fg).add_modifier(Modifier::BOLD)
}
pub fn key() -> Style {
    Style::default().fg(text()).add_modifier(Modifier::BOLD)
}
pub fn selected_row() -> Style {
    Style::default().bg(selection())
}
/// Stable color for a routing target so the same group reads the same everywhere.
pub fn target(tag: &str) -> Color {
    match tag {
        "direct" | "sing-direct" => good(),
        "reject" | "block" => bad(),
        "" => dim(),
        _ => {
            let palette = [accent(), info(), violet()];
            let h = tag
                .bytes()
                .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
            palette[(h % palette.len() as u32) as usize]
        }
    }
}
