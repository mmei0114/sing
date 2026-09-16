//! One quiet palette. Truecolor where the terminal supports it, the nearest
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
    c(pick((114, 216, 191), 115))
}
pub fn text() -> Color {
    c(pick((222, 228, 235), 254))
}
pub fn dim() -> Color {
    c(pick((134, 146, 160), 245))
}
pub fn faint() -> Color {
    c(pick((72, 82, 96), 239))
}
pub fn good() -> Color {
    c(pick((126, 220, 150), 114))
}
pub fn warn() -> Color {
    c(pick((240, 200, 110), 221))
}
pub fn bad() -> Color {
    c(pick((246, 124, 132), 210))
}
pub fn info() -> Color {
    c(pick((128, 176, 255), 111))
}
pub fn violet() -> Color {
    c(pick((186, 156, 255), 141))
}
pub fn selection() -> Color {
    c(pick((40, 52, 66), 236))
}
pub fn panel() -> Color {
    c(pick((24, 30, 38), 234))
}

pub fn s(fg: Color) -> Style {
    Style::default().fg(fg)
}
pub fn bold(fg: Color) -> Style {
    Style::default().fg(fg).add_modifier(Modifier::BOLD)
}
pub fn key() -> Style {
    Style::default().fg(accent()).add_modifier(Modifier::BOLD)
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
            let h = tag.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
            palette[(h % palette.len() as u32) as usize]
        }
    }
}
