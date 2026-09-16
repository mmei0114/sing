//! Width-aware text helpers. Node names are full of CJK and emoji.
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}
/// Truncate to a display width, ending with "…" when shortened.
pub fn fit(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in s.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w + 1 > max {
            break;
        }
        used += w;
        out.push(ch);
    }
    out.push('…');
    out
}
/// Truncate and pad with spaces to exactly `n` columns.
pub fn cell(s: &str, n: usize) -> String {
    let t = fit(s, n);
    let pad = n.saturating_sub(width(&t));
    format!("{t}{}", " ".repeat(pad))
}
pub fn right(s: &str, n: usize) -> String {
    let t = fit(s, n);
    let pad = n.saturating_sub(width(&t));
    format!("{}{t}", " ".repeat(pad))
}
pub fn bytes(n: i64) -> String {
    let n = n.max(0) as f64;
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{} B", n as i64)
    } else if v >= 100.0 {
        format!("{v:.0} {}", UNITS[i])
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}
pub fn rate(n: i64) -> String {
    format!("{}/s", bytes(n))
}
pub fn ago(seconds: u64) -> String {
    match seconds {
        0..=4 => "now".into(),
        5..=59 => format!("{seconds}s"),
        60..=3599 => format!("{}m", seconds / 60),
        3600..=86399 => format!("{}h", seconds / 3600),
        _ => format!("{}d", seconds / 86400),
    }
}
pub fn duration(seconds: u64) -> String {
    let (h, m) = (seconds / 3600, (seconds % 3600) / 60);
    if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {:02}s", seconds % 60)
    } else {
        format!("{seconds}s")
    }
}
/// Core timestamps arrive in milliseconds; older builds used seconds.
pub fn epoch_seconds(t: i64) -> u64 {
    if t > 100_000_000_000 {
        (t / 1000) as u64
    } else {
        t.max(0) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wide_characters_are_measured_by_columns() {
        assert_eq!(width("日本 🇯🇵"), 7);
        assert_eq!(fit("日本東京", 5), "日本…");
        assert_eq!(width(&cell("日本東京", 6)), 6);
        assert_eq!(bytes(1536), "1.5 KB");
        assert_eq!(ago(125), "2m");
    }
}
