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
    assert!(dns.contains("Config / dns"));
    assert!(dns.contains("Servers"));
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
fn browsing_holds_view_but_collects_every_observed_snapshot() {
    let mut app = demo_app();
    app.go(Tab::Activity);
    app.key(key(KeyCode::Down));
    let id = app.observation_view().entries[1].c.id.clone();
    let mut c = app.history.entries[0].c.clone();
    c.id = "new-connection".into();
    c.created_at += 20000;
    app.observe_connections(runtime::ConnectionReport {
        observed_at: 123,
        total: 1,
        items: vec![c.clone()],
    });
    assert_eq!(app.observation_view().entries[1].c.id, id);
    assert!(app.frozen_history.is_some());
    assert!(app
        .history
        .entries
        .iter()
        .any(|e| e.c.id == "new-connection"));
    c.id = "later-connection".into();
    c.created_at += 20000;
    app.observe_connections(runtime::ConnectionReport {
        observed_at: 124,
        total: 1,
        items: vec![c],
    });
    assert!(app
        .history
        .entries
        .iter()
        .any(|e| e.c.id == "new-connection" && !e.open));
    assert_eq!(app.observation_view().entries[1].c.id, id);
    app.key(key(KeyCode::Char(' ')));
    assert_eq!(app.observation_view().entries[0].c.id, "later-connection");
    assert!(app.frozen_history.is_none());
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
fn section_navigation_has_no_bracket_hints() {
    let mut app = demo_app();
    for tab in [Tab::Proxies, Tab::Activity] {
        app.go(tab);
        for _ in 0..3 {
            for (w, h) in [(54, 18), (80, 24), (140, 32)] {
                let view = screen(&app, w, h);
                assert!(!view.contains("[/]"), "{view}");
                assert_eq!(view.matches("← →").count(), 1, "{view}");
            }
            app.key(key(KeyCode::Right));
        }
    }
    config::open_menu(&mut app);
    app.paste("dns");
    app.key(key(KeyCode::Enter));
    app.drain();
    for _ in 0..3 {
        assert!(!screen(&app, 100, 28).contains("[/]"));
        assert_eq!(screen(&app, 100, 28).matches("← →").count(), 1);
        app.key(key(KeyCode::Right));
    }
}

#[test]
fn workspace_subsections_sit_directly_below_the_navigation_underline() {
    let mut app = demo_app();
    for (tab, title) in [(Tab::Proxies, "Groups"), (Tab::Activity, "Connections")] {
        app.go(tab);
        for (w, h) in [(54, 18), (80, 24), (140, 32)] {
            let view = screen(&app, w, h);
            assert!(view.lines().nth(1).unwrap().contains('━'), "{view}");
            assert!(view.lines().nth(2).unwrap().contains(title), "{view}");
        }
    }
}

#[test]
fn overview_content_sits_directly_below_the_navigation_underline() {
    let app = demo_app();
    for (w, h) in [(54, 18), (80, 24), (140, 32)] {
        let view = screen(&app, w, h);
        assert!(view.lines().nth(1).unwrap().contains('━'), "{view}");
        assert!(view.lines().nth(2).unwrap().contains("Capture"), "{view}");
    }
}

#[test]
fn traffic_arrows_keep_a_single_space_before_the_value() {
    let mut app = demo_app();
    for available in [true, false] {
        app.snap.status.traffic_available = available;
        for (w, h) in [(80, 24), (140, 32)] {
            let view = screen(&app, w, h);
            let header = view.lines().take(2).collect::<Vec<_>>().join("\n");
            assert!(header.contains("↓ ") && header.contains("↑ "), "{view}");
            assert!(!header.contains("↓  ") && !header.contains("↑  "), "{view}");
            if !available {
                assert!(header.contains("↓ —  ↑ —"), "{view}");
            }
        }
    }
}

#[test]
fn unmatched_traffic_is_editable_through_native_route_options() {
    let mut app = demo_app();
    app.key(key(KeyCode::Char(':')));
    app.paste("route");
    app.key(key(KeyCode::Enter));
    app.drain();
    app.key(key(KeyCode::Left));
    assert!(screen(&app, 100, 28).contains("Options"));
    app.key(key(KeyCode::Enter));
    app.drain();
    let view = screen(&app, 100, 28);
    assert!(view.contains("Unmatched traffic"), "{view}");
    app.key(key(KeyCode::Enter));
    assert!(screen(&app, 100, 28).contains("Proxy"));
}

#[test]
fn overview_has_wordmark_and_compact_honest_rates_without_chart() {
    let mut app = demo_app();
    for (w, h) in [(54, 18), (80, 24), (140, 32)] {
        let view = screen(&app, w, h);
        assert!(view.contains("SING"));
        assert!(!view.contains('◆'));
        assert!(!view.contains("Traffic"));
        assert!(!view.contains("sampled / combined"));
        if w >= 80 {
            assert!(view.lines().take(2).any(|l| l.contains("KB/s")), "{view}");
        }
    }
    app.snap.status.traffic_available = false;
    let view = screen(&app, 140, 32);
    assert!(!view.lines().take(2).any(|l| l.contains("KB/s")));
    assert!(view.lines().take(2).any(|l| l.contains('—')));
}

#[test]
fn activity_top_level_entry_and_section_switch_clear_app_scope() {
    let mut app = demo_app();
    app.key(key(KeyCode::Char('3')));
    app.key(key(KeyCode::Right));
    app.key(key(KeyCode::Enter));
    assert!(app.activity.app_filter.is_some());
    let held = screen(&app, 54, 18);
    assert!(held.contains("Esc back"), "{held}");
    assert!(held.contains("Held"), "{held}");
    app.key(key(KeyCode::Esc));
    assert_eq!(app.activity.section, activity::Section::Apps);
    app.key(key(KeyCode::Enter));
    app.key(key(KeyCode::Char('1')));
    app.key(key(KeyCode::Char('c')));
    assert!(app.activity.app_filter.is_none());
    assert!(!app.activity.paused);
    assert_eq!(app.activity.section, activity::Section::Requests);
    assert!(screen(&app, 100, 28).contains("All connections"));
    app.key(key(KeyCode::Right));
    app.key(key(KeyCode::Enter));
    app.key(key(KeyCode::Right));
    app.key(key(KeyCode::Left));
    assert!(app.activity.app_filter.is_none());
    app.activity.query = "no-match".into();
    app.key(key(KeyCode::Char('3')));
    assert!(app.activity.query.is_empty());
}

#[test]
fn activity_detail_pane_is_opt_in_and_does_not_hide_list_by_default() {
    let mut app = demo_app();
    app.go(Tab::Activity);
    assert!(!screen(&app, 140, 32).contains("Matched rule"));
    app.key(key(KeyCode::Char('d')));
    assert!(screen(&app, 140, 32).contains("Matched rule"));
    app.key(key(KeyCode::Enter));
    assert!(screen(&app, 80, 24).contains("Connection evidence"));
}

#[test]
fn rule_labels_are_readable_without_mutating_native_document() {
    let mut app = demo_app();
    app.snap.store.native.as_mut().unwrap()["route"]["rules"] = json!([
        {"action":"sniff"}, {"protocol":"dns","action":"hijack-dns"}
    ]);
    let original = app.doc().clone();
    app.go(Tab::Proxies);
    app.key(key(KeyCode::Right));
    let view = screen(&app, 140, 32);
    assert!(view.contains("All connections"));
    assert!(view.contains("Detect protocol"));
    assert!(view.contains("DNS traffic"));
    assert!(view.contains("Handle DNS"));
    assert!(view.contains("(sniff)"));
    assert_eq!(&original, app.doc());
    assert!(app.outbox.is_empty());
}

#[test]
#[ignore = "Writes fictional renderer snapshots to .build/violet-polish"]
fn render_polish_review() {
    use ratatui::style::{Color, Modifier};
    fn color(c: Color) -> String {
        match c {
            Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
            Color::Indexed(i) if i >= 232 => {
                let v = 8 + (i - 232) as u16 * 10;
                format!("#{v:02x}{v:02x}{v:02x}")
            }
            Color::Indexed(i) if i >= 16 => {
                let i = i - 16;
                let levels = [0, 95, 135, 175, 215, 255];
                let (r, g, b) = (
                    levels[(i / 36) as usize],
                    levels[((i / 6) % 6) as usize],
                    levels[(i % 6) as usize],
                );
                format!("#{r:02x}{g:02x}{b:02x}")
            }
            _ => "#dee4eb".into(),
        }
    }
    fn xml(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        if theme::background() == Color::Rgb(23, 19, 32) {
            ".build/violet-polish/truecolor"
        } else {
            ".build/violet-polish/indexed"
        },
    );
    std::fs::create_dir_all(&dir).unwrap();
    for (name, w, h) in [
        ("overview", 120, 32),
        ("overview80", 80, 24),
        ("overview54", 54, 18),
        ("config", 80, 24),
        ("dns", 100, 28),
        ("apps", 110, 30),
        ("connections", 80, 24),
        ("connections-wide", 140, 32),
        ("connections-detail", 140, 32),
        ("connections-held", 54, 18),
        ("rules", 110, 30),
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
            "connections" | "connections-wide" => app.go(Tab::Activity),
            "connections-detail" => {
                app.go(Tab::Activity);
                app.activity.show_details = true;
            }
            "connections-held" => {
                app.go(Tab::Activity);
                app.key(key(KeyCode::Right));
                app.key(key(KeyCode::Enter));
            }
            "rules" => {
                app.snap.store.native.as_mut().unwrap()["route"]["rules"] = json!([
                    {"action":"sniff"},
                    {"protocol":"dns","action":"hijack-dns"},
                    {"domain_suffix":["example.com"],"outbound":"proxy"}
                ]);
                app.go(Tab::Proxies);
                app.key(key(KeyCode::Right));
            }
            "policies" => app.go(Tab::Proxies),
            _ => {}
        }
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, &app)).unwrap();
        let mut svg=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\"><rect width=\"100%\" height=\"100%\" fill=\"{}\"/><g font-family=\"Menlo,monospace\" font-size=\"16\" xml:space=\"preserve\">", w as usize*10+32,h as usize*21+32, color(theme::background()));
        for (i, c) in term.backend().buffer().content.iter().enumerate() {
            let x = 16 + i % w as usize * 10;
            let y = 16 + i / w as usize * 21;
            let bg = if c.bg == Color::Reset {
                color(theme::background())
            } else {
                color(c.bg)
            };
            svg.push_str(&format!("<rect x=\"{x}\" y=\"{y}\" width=\"10\" height=\"21\" fill=\"{bg}\"/><text x=\"{x}\" y=\"{}\" fill=\"{}\" font-weight=\"{}\">{}</text>", y+16,color(c.fg),if c.modifier.contains(Modifier::BOLD){"bold"}else{"normal"},xml(c.symbol())));
        }
        svg.push_str("</g></svg>");
        std::fs::write(dir.join(format!("{name}.svg")), svg).unwrap();
    }
}
