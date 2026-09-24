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
fn dns_recovery_uses_the_normal_interface_once_per_failure() {
    let mut app = demo_app();
    app.snap.connected = false;
    app.snap.capture_recovery = "Previous network settings need to be restored".into();
    app.recover_capture_if_needed();
    app.recover_capture_if_needed();
    assert_eq!(
        app.outbox.len(),
        1,
        "do not repeatedly ask for authorization"
    );
    assert!(matches!(
        app.outbox.front().unwrap().0,
        Action::RecoverCapture
    ));
    let view = screen(&app, 110, 32);
    assert!(view.contains("Network settings need attention"));
    assert!(!view.contains("--restore-dns"));
    app.outbox.clear();
    chrome::start_stop(&mut app);
    assert!(
        matches!(app.outbox.front().unwrap().0, Action::Connect),
        "Start resumes automatically after recovery"
    );
    app.outbox.clear();
    app.snap.capture_recovery.clear();
    app.recover_capture_if_needed();
    app.snap.capture_recovery = "A later recovery".into();
    app.recover_capture_if_needed();
    assert_eq!(app.outbox.len(), 1);
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
fn alt_arrows_move_the_same_rule_in_both_workspaces_without_applying() {
    for in_config in [false, true] {
        let mut app = demo_app();
        if in_config {
            app.key(key(KeyCode::Char(':')));
            app.paste("route");
            app.key(key(KeyCode::Enter));
        } else {
            app.go(Tab::Proxies);
            app.key(key(KeyCode::Right));
        }
        app.drain();
        assert!(screen(&app, 80, 24).contains("Alt+↑↓ reorder"));
        let initial = app.doc().clone();
        let original = native::array(&initial, "/route/rules").to_vec();
        let up = KeyEvent::new(KeyCode::Up, KeyModifiers::ALT);
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::ALT);
        app.key(up); // The first rule cannot move upwards.
        assert!(app.outbox.is_empty() && app.busy.is_none());
        app.key(down);
        assert!(app.busy.is_some());
        assert_eq!(app.doc(), &initial);
        app.drain();
        let mut repeated = down;
        repeated.kind = event::KeyEventKind::Repeat;
        app.key(repeated);
        app.drain();
        assert_eq!(app.doc()["route"]["rules"][2], original[0]);
        assert_eq!(app.doc()["route"]["rules"][0], original[1]);
        assert_eq!(app.doc()["outbounds"], initial["outbounds"]);
        assert_eq!(app.doc()["dns"], initial["dns"]);
        assert!(app.snap.connected && app.snap.dirty);
        app.key(up);
        app.drain();
        assert_eq!(app.doc()["route"]["rules"][1], original[0]);
        let reordered = app.doc().clone();
        // Releasing Alt restores navigation; J/K no longer reorder.
        for k in [
            key(KeyCode::Down),
            key(KeyCode::Up),
            key(KeyCode::Char('J')),
            key(KeyCode::Char('K')),
        ] {
            app.key(k);
        }
        let mut released = down;
        released.kind = event::KeyEventKind::Release;
        app.key(released);
        assert_eq!(app.doc(), &reordered);
        assert!(app.outbox.is_empty());
        for _ in 0..original.len() {
            app.key(key(KeyCode::Down));
        }
        app.key(down); // No wraparound at the bottom.
        assert!(app.outbox.is_empty() && app.busy.is_none());
    }
}

fn finish_demo_job(app: &mut App) {
    let (action, then, _) = app.outbox.pop_front().unwrap();
    let mut demo = app.demo.take().unwrap();
    let reply = demo.handle(app, action);
    app.demo = Some(demo);
    app.finish(reply, then);
}

#[test]
fn reorder_serializes_pending_moves_and_rejects_concurrent_edits() {
    for before_read in [true, false] {
        let mut app = demo_app();
        app.go(Tab::Proxies);
        app.key(key(KeyCode::Right));
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::ALT);
        app.key(down);
        // A previously queued, unrelated reply must not unlock the reorder operation.
        let unrelated: Reply =
            serde_json::from_value(json!({"ok":true,"message":"","needs_auth":false})).unwrap();
        app.finish(unrelated, Box::new(|_, _| {}));
        app.key(down); // A pending move cannot target a second, stale index.
        assert_eq!(app.outbox.len(), 1);
        assert_eq!(app.proxies.selected[1], 0);
        if !before_read {
            finish_demo_job(&mut app);
        }
        let mut demo = app.demo.take().unwrap();
        let mut edit = demo
            .handle(&app, Action::ReadNative("/route/rules".into()))
            .edit
            .unwrap();
        edit.value
            .as_array_mut()
            .unwrap()
            .insert(0, json!({"domain":["new.invalid"],"outbound":"direct"}));
        assert!(demo.handle(&app, Action::WriteNative(edit)).ok);
        let other_edit = demo.snapshot().store.native.unwrap();
        app.demo = Some(demo);
        finish_demo_job(&mut app);
        assert!(app.busy.is_none() && !app.moving_rule && app.outbox.is_empty());
        assert_eq!(app.proxies.selected[1], 0);
        assert_eq!(app.doc(), &other_edit);
        assert!(app
            .toast
            .as_ref()
            .is_some_and(|t| t.error && t.text.contains("changed")));
    }
}

#[test]
fn moving_rules_preserves_unredacted_fields_and_empty_lists_are_safe() {
    let mut app = demo_app();
    let mut demo = app.demo.take().unwrap();
    let mut edit = demo
        .handle(&app, Action::ReadNative("/route/rules".into()))
        .edit
        .unwrap();
    edit.value[0]["future"] = json!({"secret":"fictional-test-value","other":17});
    assert!(demo.handle(&app, Action::WriteNative(edit)).ok);
    app.observe(demo.snapshot());
    app.demo = Some(demo);
    app.go(Tab::Proxies);
    app.key(key(KeyCode::Right));
    app.key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
    app.drain();
    let mut demo = app.demo.take().unwrap();
    let mut edit = demo
        .handle(&app, Action::ReadNative("/route/rules".into()))
        .edit
        .unwrap();
    assert_eq!(
        edit.value[1]["future"],
        json!({"secret":"fictional-test-value","other":17})
    );
    edit.value = json!([]);
    assert!(demo.handle(&app, Action::WriteNative(edit)).ok);
    app.observe(demo.snapshot());
    app.demo = Some(demo);
    for code in [KeyCode::Up, KeyCode::Down] {
        app.key(KeyEvent::new(code, KeyModifiers::ALT));
    }
    assert!(app.outbox.is_empty() && app.busy.is_none());
}

#[test]
fn nested_rule_lists_use_alt_arrows_and_keep_edits_local_until_saved() {
    use std::{cell::RefCell, rc::Rc};
    let mut app = demo_app();
    let original = app.doc().clone();
    let values = vec![
        json!({"domain":["first.invalid"]}),
        json!({"domain":["second.invalid"]}),
    ];
    let saved = Rc::new(RefCell::new(Vec::new()));
    let output = saved.clone();
    app.push(editor::ObjectList::new(
        schema::Object::RouteRule,
        "Rules",
        values.clone(),
        Box::new(move |_, v| {
            *output.borrow_mut() = v;
        }),
    ));
    for code in [KeyCode::Char('J'), KeyCode::Char('K')] {
        app.key(key(code));
    }
    app.key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
    assert_eq!(app.doc(), &original);
    assert!(saved.borrow().is_empty());
    app.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    assert_eq!(*saved.borrow(), vec![values[1].clone(), values[0].clone()]);
    assert!(app.modals.is_empty() && app.outbox.is_empty());
}

#[test]
fn node_source_removal_checks_references_confirms_and_leaves_core_running() {
    let mut app = demo_app();
    app.go(Tab::Proxies);
    app.key(key(KeyCode::Left));
    app.key(key(KeyCode::Char('x')));
    assert!(screen(&app, 100, 28).contains("Source is in use"));
    assert!(app.outbox.is_empty());
    app.key(key(KeyCode::Esc));

    // Move the source's users to another member before trying to delete it.
    let mut demo = app.demo.take().unwrap();
    let mut edit = demo
        .handle(&app, Action::ReadNative("/outbounds".into()))
        .edit
        .unwrap();
    for v in edit.value.as_array_mut().unwrap() {
        if labels::is_group(v) {
            v["outbounds"] = json!(["direct"]);
            if v.get("default").is_some() {
                v["default"] = json!("direct");
            }
        }
    }
    assert!(demo.handle(&app, Action::WriteNative(edit)).ok);
    app.observe(demo.snapshot());
    app.demo = Some(demo);
    let before = serde_json::to_value(&app.snap.store).unwrap();
    let policies = app.doc()["route"].clone();
    app.key(key(KeyCode::Char('x')));
    assert!(screen(&app, 100, 28).contains("Remove node source?"));
    app.key(key(KeyCode::Enter)); // Dangerous confirmations default to Cancel.
    app.drain();
    assert_eq!(serde_json::to_value(&app.snap.store).unwrap(), before);
    app.key(key(KeyCode::Char('x')));
    app.key(key(KeyCode::Char('y')));
    app.drain();
    assert!(app.snap.store.subscriptions.is_empty());
    assert!(app.snap.store.nodes.is_empty());
    assert_eq!(app.doc()["route"], policies);
    assert!(app.snap.connected && app.snap.dirty);
}

fn rule_source_app() -> App {
    let mut app = demo_app();
    app.demo = None;
    app.snap
        .store
        .rule_resources
        .push(crate::model::RuleResource {
            id: "video".into(),
            name: "Video rules".into(),
            source: "fixture".into(),
            format: "qx".into(),
            updated_at: 0,
            digest: String::new(),
            input_count: 1,
            rules: vec![],
            native_document: None,
            warnings: vec![],
        });
    app.snap.store.native.as_mut().unwrap()["route"]["rule_set"] = json!([
        {"type":"inline","tag":"rs-video","rules":[{"domain_suffix":["video.invalid"]}]}
    ]);
    app.go(Tab::Proxies);
    app.key(key(KeyCode::Left));
    app.proxies.selected[2] = app.snap.store.subscriptions.len();
    app
}

#[test]
fn rule_source_removal_checks_route_dns_and_tun_references() {
    for (pointer, value) in [
        (
            "/route/rules",
            json!([{"rule_set":["rs-video"],"outbound":"proxy"}]),
        ),
        (
            "/dns/rules",
            json!([{"rule_set":["rs-video"],"server":"bootstrap"}]),
        ),
        (
            "/inbounds",
            json!([{"type":"tun","tag":"tun","route_address_set":["rs-video"]}]),
        ),
    ] {
        let mut app = rule_source_app();
        native::set(app.snap.store.native.as_mut().unwrap(), pointer, value).unwrap();
        app.key(key(KeyCode::Char('x')));
        let view = screen(&app, 100, 28);
        assert!(view.contains("Source is in use"), "{view}");
        assert!(view.contains(pointer), "{view}");
        assert!(app.outbox.is_empty());
    }
}

#[test]
fn rule_source_removal_resolves_its_tag_after_confirmation_and_preserves_other_sets() {
    let mut app = rule_source_app();
    app.key(key(KeyCode::Char('x')));
    app.key(key(KeyCode::Enter));
    assert!(app.outbox.is_empty());
    app.key(key(KeyCode::Char('x')));
    app.key(key(KeyCode::Char('y')));
    // Simulate a second interface inserting a rule set while the dialog was open.
    let other = json!({"type":"inline","tag":"other","rules":[]});
    app.snap.store.native.as_mut().unwrap()["route"]["rule_set"]
        .as_array_mut()
        .unwrap()
        .insert(0, other.clone());
    let (action, then, _) = app.outbox.pop_front().unwrap();
    let Action::ReadNative(pointer) = action else {
        panic!("expected scoped read")
    };
    let mut reply: Reply =
        serde_json::from_value(json!({"ok":true,"message":"","needs_auth":false})).unwrap();
    reply.edit = Some(native::read(&app.snap.store, pointer).unwrap());
    app.finish(reply, then);
    let (action, _, _) = app.outbox.pop_front().unwrap();
    let Action::WriteNative(edit) = action else {
        panic!("expected draft save")
    };
    assert_eq!(edit.pointer, "/route/rule_set");
    assert_eq!(edit.value, json!([other]));
    let original = app.snap.store.clone();
    let mut changed = original.clone();
    changed.native.as_mut().unwrap()["route"]["rules"] = json!([
        {"rule_set":["rs-video"],"outbound":"proxy"}
    ]);
    let before = serde_json::to_value(&changed).unwrap();
    assert!(native::write(&mut changed, edit.clone()).is_err());
    assert_eq!(serde_json::to_value(&changed).unwrap(), before);
    native::write(&mut app.snap.store, edit).unwrap();
    assert!(app.snap.store.rule_resources.is_empty());
    assert_eq!(app.doc()["route"]["rule_set"], json!([other]));
    assert_eq!(
        app.doc()["outbounds"],
        original.native.unwrap()["outbounds"]
    );
    assert!(app.outbox.is_empty());
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
#[ignore = "Writes fictional renderer snapshots to .build/theme-review"]
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
        if matches!(theme::background(), Color::Rgb(..)) {
            ".build/theme-review/truecolor"
        } else {
            ".build/theme-review/indexed"
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
