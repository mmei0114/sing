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
    app.paste("dns");
    app.key(key(KeyCode::Enter));
    app.drain();
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

#[test]
fn module_picker_preserves_workspace_and_has_one_dns_entry() {
    let mut app = demo_app();
    app.go(Tab::Activity);
    app.key(key(KeyCode::Char(':')));
    assert_eq!(app.tab, Tab::Activity);
    let popup = screen(&app, 80, 24);
    assert!(popup.contains("choose a module"));
    app.paste("dns");
    app.key(key(KeyCode::Enter));
    assert_eq!(app.tab, Tab::Config);
    let config = screen(&app, 110, 30);
    assert!(!config.contains("certificate"));
    app.key(key(KeyCode::Esc));
    assert_eq!(app.tab, Tab::Activity);
}

#[test]
fn overview_scales_and_keeps_controls_and_actual_connections() {
    let mut app = demo_app();
    for (w, h) in [(54, 18), (80, 24), (120, 32)] {
        let view = screen(&app, w, h);
        assert!(view.contains("Connections"), "{view}");
        assert!(view.contains(": Config"), "{view}");
        assert!(!view.contains("else "));
        assert!(!view.contains("Global target"));
    }
    app.key(key(KeyCode::Tab));
    app.key(key(KeyCode::Down));
    assert!(app.activity.paused);
    app.key(key(KeyCode::Enter));
    assert!(screen(&app, 80, 24).contains("Connection evidence"));
    app.key(key(KeyCode::Esc));
    app.key(key(KeyCode::Esc));
    assert!(!app.activity.paused);
}

#[test]
fn observation_pause_holds_identity_until_explicit_resume() {
    let mut app = demo_app();
    app.go(Tab::Activity);
    app.key(key(KeyCode::Down));
    let id = app.history.entries[1].c.id.clone();
    let mut c = app.history.entries[0].c.clone();
    c.id = "new-connection".into();
    c.created_at += 20000;
    app.observe_connections(runtime::ConnectionReport {
        observed_at: 123,
        total: 1,
        items: vec![c],
    });
    assert_eq!(app.history.entries[1].c.id, id);
    assert!(app.pending_connections.is_some());
    app.key(key(KeyCode::Char(' ')));
    assert_eq!(app.history.entries[0].c.id, "new-connection");
    assert!(app.pending_connections.is_none());
}

#[test]
fn missing_identity_has_actionable_explanation_and_does_not_invent_app() {
    let mut app = demo_app();
    app.snap.store.native.as_mut().unwrap()["route"]["find_process"] = json!(false);
    app.snap.store.native.as_mut().unwrap()["route"]["rules"] = json!([]);
    let c = api::Connection::default();
    app.push(flows::connection_details(&app, &c));
    let view = screen(&app, 110, 32);
    assert!(view.contains("Lookup off"));
    assert!(view.contains("Enable it with f"));
    assert!(!view.contains("Application  Unavailable"));
}

#[test]
fn active_system_proxy_recovery_record_is_not_an_error() {
    let mut app = demo_app();
    app.snap.system_proxy.pending_restore = true;
    app.snap.system_proxy.configured = true;
    app.snap.system_proxy.helper_ready = true;
    app.snap.system_proxy.effective = true;
    let view = screen(&app, 110, 32);
    assert!(view.contains("System proxy verified"));
    assert!(!view.contains("needs recovery"));
    assert!(!view.contains("Recovery  "));
}

#[test]
fn activity_search_captures_letters_instead_of_toggling_core() {
    let mut app = demo_app();
    app.go(Tab::Activity);
    app.key(key(KeyCode::Char('/')));
    app.key(key(KeyCode::Char('s')));
    assert!(app.snap.connected);
    assert!(app.outbox.is_empty());
    app.key(key(KeyCode::Enter));
    assert_eq!(app.activity.query, "s");
    app.key(key(KeyCode::Esc));
    assert!(app.activity.query.is_empty());
}

#[test]
fn discovery_is_reviewed_and_never_applied_implicitly() {
    let mut app = demo_app();
    let mut demo = app.demo.take().unwrap();
    let mut edit = demo
        .handle(&app, Action::ReadNative("/route".into()))
        .edit
        .unwrap();
    edit.value["find_process"] = json!(false);
    assert!(demo.handle(&app, Action::WriteNative(edit)).ok);
    assert!(demo.handle(&app, Action::Connect).ok);
    app.observe(demo.snapshot());
    app.demo = Some(demo);
    app.go(Tab::Activity);
    app.key(key(KeyCode::Char('f')));
    assert!(app.outbox.is_empty());
    app.key(key(KeyCode::Esc));
    assert_eq!(app.doc()["route"]["find_process"], false);
    app.key(key(KeyCode::Char('f')));
    app.key(key(KeyCode::Enter));
    app.drain();
    assert_eq!(app.doc()["route"]["find_process"], true);
    assert!(screen(&app, 80, 24).contains("Review & Apply"));
    app.key(key(KeyCode::Esc));
    assert!(app.snap.dirty);
}

#[test]
fn unidentified_app_cannot_silently_become_a_domain_rule() {
    let mut app = demo_app();
    quick_rule::from_connection(
        &mut app,
        api::Connection {
            domain: "example.com".into(),
            ..Default::default()
        },
        true,
    );
    assert!(app.modals.is_empty());
    assert!(app
        .toast
        .as_ref()
        .is_some_and(|t| t.error && t.text.contains("No executable")));
}

#[test]
#[ignore = "Writes fictional renderer snapshots to .build/polish-review"]
fn render_polish_review() {
    use ratatui::style::{Color, Modifier};
    fn color(c: Color) -> String {
        match c {
            Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
            _ => "#dee4eb".into(),
        }
    }
    fn xml(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".build/polish-review");
    std::fs::create_dir_all(&dir).unwrap();
    for (name, w, h) in [
        ("overview", 120, 32),
        ("overview80", 80, 24),
        ("overview54", 54, 18),
        ("config", 80, 24),
        ("dns", 100, 28),
        ("apps", 110, 30),
        ("connections", 80, 24),
        ("policies", 110, 30),
        ("editor", 80, 24),
    ] {
        let mut app = demo_app();
        match name {
            "config" => config::open_menu(&mut app),
            "dns" | "editor" => {
                config::open_menu(&mut app);
                app.paste("dns");
                app.key(key(KeyCode::Enter));
                app.drain();
                if name == "editor" {
                    app.key(key(KeyCode::Enter));
                    app.drain();
                }
            }
            "apps" => {
                app.go(Tab::Activity);
                app.key(key(KeyCode::Char(']')));
            }
            "connections" => app.go(Tab::Activity),
            "policies" => app.go(Tab::Proxies),
            _ => {}
        }
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, &app)).unwrap();
        let mut svg=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\"><rect width=\"100%\" height=\"100%\" fill=\"#0f1722\"/><g font-family=\"Menlo,monospace\" font-size=\"16\" xml:space=\"preserve\">", w as usize*10+32,h as usize*21+32);
        for (i, c) in term.backend().buffer().content.iter().enumerate() {
            let x = 16 + i % w as usize * 10;
            let y = 16 + i / w as usize * 21;
            let bg = if c.bg == Color::Reset {
                "#0f1722".into()
            } else {
                color(c.bg)
            };
            svg.push_str(&format!("<rect x=\"{x}\" y=\"{y}\" width=\"10\" height=\"21\" fill=\"{bg}\"/><text x=\"{x}\" y=\"{}\" fill=\"{}\" font-weight=\"{}\">{}</text>", y+16,color(c.fg),if c.modifier.contains(Modifier::BOLD){"bold"}else{"normal"},xml(c.symbol())));
        }
        svg.push_str("</g></svg>");
        std::fs::write(dir.join(format!("{name}.svg")), svg).unwrap();
    }
}
