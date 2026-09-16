use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn demo_app() -> App {
    let demo = demo::Demo::running().unwrap();
    let mut app = App::new(demo.snapshot(), Some(demo));
    for _ in 0..8 {
        let mut backend = app.demo.take().unwrap();
        backend.tick(&mut app);
        app.demo = Some(backend);
    }
    app
}

fn screen(app: &App, width: u16, height: u16) -> String {
    render(app, width, height).join("\n")
}

#[test]
fn eighty_column_shell_keeps_three_workspaces_and_three_core_controls() {
    let app = demo_app();
    let view = screen(&app, 80, 24);
    assert!(view.contains("1 Overview"));
    assert!(view.contains("2 Policies"));
    assert!(view.contains("3 Activity"));
    assert!(view.contains("s Stop"));
    assert!(view.contains("m Mode Rule"));
    assert!(view.contains("t TUN Off"));
    assert!(!view.contains("Proxies & Rules"));
    assert!(!view.lines().last().unwrap_or("").contains("System Proxy"));
}

#[test]
fn policies_and_activity_are_real_keyboard_workspaces() {
    let mut app = demo_app();
    app.key(key(KeyCode::Char('2')));
    let policies = screen(&app, 100, 28);
    assert!(policies.contains("Groups"));
    assert!(policies.contains("Rules"));
    assert!(policies.contains("Sources"));
    assert!(policies.contains("Streaming"));

    app.key(key(KeyCode::Char('3')));
    app.key(key(KeyCode::Char(']')));
    let activity = screen(&app, 110, 28);
    assert!(activity.contains("Connections"));
    assert!(activity.contains("Apps"));
    assert!(activity.contains("OBSERVED LINKS"));
    assert!(activity.contains("Google Chrome"));
}

#[test]
fn connection_can_open_a_prefilled_rule_in_the_shared_editor() {
    let mut app = demo_app();
    app.key(key(KeyCode::Char('3')));
    app.key(key(KeyCode::Char('r')));
    assert_eq!(app.modals.len(), 1);
    app.key(key(KeyCode::Enter));
    assert_eq!(app.modals.len(), 1);
    app.key(key(KeyCode::Enter));
    app.drain();
    let view = screen(&app, 110, 30);
    assert!(view.contains("New Rule · priority"));
    assert!(view.contains("Domain"));
    assert!(view.contains("Send to"));
}

#[test]
fn config_follows_native_sections_and_dns_uses_the_shared_editor() {
    let mut app = demo_app();
    app.key(key(KeyCode::Char(',')));
    app.drain();
    app.key(key(KeyCode::Char(']')));
    app.key(key(KeyCode::Char(']')));
    let dns = screen(&app, 110, 30);
    assert!(dns.contains("dns › servers"));
    assert!(dns.contains("dns-proxy"), "{dns}");
    app.key(key(KeyCode::Enter));
    app.drain();
    app.key(key(KeyCode::Char('a')));
    let editor = screen(&app, 110, 30);
    assert!(editor.contains("Edit DNS Server"));
    assert!(editor.contains("Via outbound"), "{editor}");
}
