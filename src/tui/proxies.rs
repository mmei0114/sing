use super::{notify, theme, App, labels, modal::{Choice, Picker}};
use crate::{native, runtime::Action};
use crossterm::event::KeyEvent;
use ratatui::{layout::Rect, style::Color, Frame};
use serde_json::Value;
#[derive(Default)]
pub struct State {}
pub fn capturing(_: &App) -> bool { false }
pub fn key(_: &mut App, _: KeyEvent) {}
pub fn paste(_: &mut App, _: &str) {}
pub fn draw(_: &mut Frame, _: Rect, _: &App) {}
pub fn hints(_: &App) -> Vec<(&'static str, &'static str)> { vec![] }

/// (member in use, its latency). Live API values while running; the saved
/// default otherwise.
pub fn current_member(app: &App, tag: &str, v: &Value) -> (String, i32) {
    if let Some(g) = app.live_group(tag) {
        let delay = g.items.iter().find(|i| i.tag == g.selected).map(|i| i.delay).unwrap_or(0);
        return (g.selected.clone(), delay);
    }
    let member = v["default"]
        .as_str()
        .or_else(|| v["outbounds"][0].as_str())
        .unwrap_or("")
        .to_string();
    (member, 0)
}
pub fn member_delay(app: &App, group: &str, member: &str) -> i32 {
    app.snap
        .groups
        .group
        .iter()
        .filter(|g| g.tag == group || group.is_empty())
        .flat_map(|g| g.items.iter())
        .filter(|i| i.tag == member)
        .map(|i| i.delay)
        .max()
        .unwrap_or(0)
}
pub fn delay_text(delay: i32) -> String {
    match delay {
        d if d <= 0 => String::new(),
        65535.. => "timeout".into(),
        d => format!("{d} ms"),
    }
}
pub fn delay_color(delay: i32) -> Color {
    match delay {
        d if d <= 0 => theme::dim(),
        1..=150 => theme::good(),
        151..=400 => theme::warn(),
        _ => theme::bad(),
    }
}
pub fn test_latency(app: &mut App, tag: String) {
    if !app.snap.connected {
        return app.error("Start the core to test latency");
    }
    app.request_busy(Action::Test(tag), "Testing", Box::new(notify));
}
/// Pick the member a group uses. Live selection while running (manual groups
/// only); otherwise it becomes the group's saved default.
pub fn choose_member(app: &mut App, tag: String) {
    let Some(v) = native::array(app.doc(), "/outbounds").iter().find(|v| native::tag(v) == tag).cloned() else {
        return;
    };
    if v["type"] == "urltest" {
        return app.error("Auto groups pick the fastest member themselves. l tests latency; e edits the group.");
    }
    let (current, _) = current_member(app, &tag, &v);
    let choices: Vec<Choice> = native::array(&v, "/outbounds")
        .iter()
        .filter_map(Value::as_str)
        .map(|m| {
            let kind = native::array(app.doc(), "/outbounds")
                .iter()
                .chain(native::array(app.doc(), "/endpoints"))
                .find(|o| native::tag(o) == m)
                .map(|o| labels::protocol(o["type"].as_str().unwrap_or("")).to_string())
                .unwrap_or_default();
            let delay = delay_text(member_delay(app, &tag, m));
            Choice::new(m, app.label(m), format!("{kind}  {delay}"))
        })
        .collect();
    let running = app.snap.connected && app.live_group(&tag).is_some();
    let title = format!("{} · choose proxy", app.label(&tag));
    app.push(Picker::single(
        &title,
        choices,
        &current,
        Box::new(move |app, picked| {
            let Some(member) = picked.into_iter().next() else { return };
            if running {
                app.request(Action::SelectNative { group: tag, member }, Box::new(notify));
            } else {
                set_default(app, tag, member);
            }
        }),
    ));
}
fn set_default(app: &mut App, tag: String, member: String) {
    let Some(i) = native::array(app.doc(), "/outbounds").iter().position(|v| native::tag(v) == tag) else {
        return;
    };
    app.request(
        Action::ReadNative(format!("/outbounds/{i}")),
        Box::new(move |app, r| {
            let Some(mut edit) = r.edit.filter(|e| r.ok && native::tag(&e.value) == tag) else {
                return app.error("Group changed; try again");
            };
            edit.value["default"] = serde_json::json!(member);
            app.request(Action::WriteNative(edit), Box::new(|app, r| {
                if r.ok { app.toast("Default saved; used when the core starts") } else { notify(app, r) }
            }));
        }),
    );
}
