//! Obsidian: near-black surfaces, soft white text and focused violet accents.
//! Truecolor where supported;
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
    c(pick((182, 154, 247), 147))
}
pub fn text() -> Color {
    c(pick((222, 222, 231), 253))
}
pub fn dim() -> Color {
    c(pick((161, 161, 181), 248))
}
pub fn faint() -> Color {
    c(pick((147, 147, 166), 248))
}
pub fn good() -> Color {
    c(pick((159, 196, 168), 151))
}
pub fn warn() -> Color {
    c(pick((223, 188, 131), 180))
}
pub fn bad() -> Color {
    c(pick((234, 146, 166), 181))
}
pub fn info() -> Color {
    c(pick((169, 181, 214), 146))
}
pub fn violet() -> Color {
    c(pick((177, 167, 201), 146))
}
pub fn selection() -> Color {
    c(pick((48, 42, 67), 237))
}
pub fn panel() -> Color {
    c(pick((30, 30, 39), 235))
}
pub fn background() -> Color {
    c(pick((21, 21, 27), 234))
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
