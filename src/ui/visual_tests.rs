//! Deterministic screenshots of the real renderer, using fictional data only.
use super::*;
use ratatui::style::{Color, Modifier};

fn color(color: Color, fallback: &str) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Yellow => "#ffff00".into(),
        Color::LightRed => "#ff8080".into(),
        Color::Gray => "#c0c0c0".into(),
        _ => fallback.into(),
    }
}
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn screen(a: &App, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| view::draw(f, a)).unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .chunks(width as usize)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect()
}

fn live_overview(a: &mut App) {
    a.snapshot.connected = true;
    a.snapshot.api_ready = true;
    a.snapshot.running_settings = Some(a.snapshot.store.settings.clone());
    a.snapshot.system_proxy.effective = true;
    a.snapshot.status.traffic_available = true;
    a.snapshot.status.uplink = 24576;
    a.snapshot.status.downlink = 2_621_440;
    a.snapshot.groups.group.push(api::Group {
        tag: "proxy".into(),
        selected: "Tokyo".into(),
        ..Default::default()
    });
    for name in ["HK", "US", "JP"] {
        a.snapshot.store.native.as_mut().unwrap()["outbounds"].as_array_mut().unwrap().push(json!({"type":"selector","tag":name,"outbounds":["Tokyo","Singapore"],"default":"Tokyo"}));
        a.snapshot.groups.group.push(api::Group {
            tag: name.into(),
            selected: if name == "US" {
                "Singapore".into()
            } else {
                "Tokyo".into()
            },
            ..Default::default()
        });
    }
    a.observe_connections(runtime::ConnectionReport {
        observed_at: model::now(),
        total: 3,
        items: [
            ("video.example.invalid", vec!["HK", "Tokyo"]),
            ("api.example.invalid", vec!["US", "Singapore"]),
            ("192.168.1.1", vec!["direct"]),
        ]
        .iter()
        .enumerate()
        .map(|(i, (domain, chain))| api::Connection {
            id: format!("fictional-{i}"),
            domain: (*domain).into(),
            chain: chain.iter().map(|s| (*s).into()).collect(),
            outbound: chain[0].into(),
            rule: "Fictional demo rule".into(),
            ..Default::default()
        })
        .collect(),
    });
}

#[test]
fn overview_has_no_empty_navigation_row_or_outlet_summary_and_no_more_alias() {
    for (width, height) in [(54, 18), (80, 24), (120, 35)] {
        let mut a = App::new(sample().unwrap(), true);
        live_overview(&mut a);
        let lines = screen(&a, width, height);
        assert!(lines[2].contains("Test Connection"));
        let rendered = lines.join("\n");
        for text in [
            "Proxy Groups",
            "Connections",
            "l Focus",
            "Traffic",
            "2.5 MiB/s",
        ] {
            assert!(rendered.contains(text), "{width}x{height}: {text}");
        }
        for absent in [": More", "Fallback", "Global target", "sing-box 1.14.0"] {
            assert!(!rendered.contains(absent));
        }
        a.focus = Focus::Connections;
        assert!(screen(&a, width, height)
            .join("\n")
            .contains("example.invalid"));
    }
}

#[test]
fn traffic_requires_fresh_available_data_and_check_displays_age_and_target() {
    let mut a = App::new(sample().unwrap(), true);
    live_overview(&mut a);
    a.snapshot.connectivity = runtime::ProbeStatus {
        state: "passed".into(),
        checked_at: model::now() - 25,
        detail: String::new(),
    };
    let rendered = screen(&a, 80, 24).join("\n");
    assert!(rendered.contains("25s ago") && rendered.contains("www.gstatic.com"));
    a.snapshot.status.traffic_available = false;
    let unavailable = screen(&a, 80, 24).join("\n");
    assert!(unavailable.contains("Unavailable"));
    assert!(!unavailable.contains("2.5 MiB/s"));
    a.snapshot.status.traffic_available = true;
    a.snapshot_at = model::now() - 20;
    assert!(screen(&a, 80, 24).join("\n").contains("stale sample"));
}

#[test]
fn dock_stays_anchored_and_status_remains_honest() {
    for (width, height) in [(54, 18), (80, 24), (120, 35)] {
        let mut app = App::new(sample().unwrap(), true);
        let baseline = screen(&app, width, height);
        let dock_start = baseline
            .iter()
            .position(|line| line.contains("c Start"))
            .unwrap();
        assert!(dock_start >= height as usize - 2);
        app.snapshot.dirty = true;
        app.error = true;
        app.notice = "Draft changed. Your inputs are preserved; review before retrying.".into();
        let changed = screen(&app, width, height);
        assert_eq!(&baseline[dock_start..], &changed[dock_start..]);
        assert!(changed[0].contains("Draft"));
        assert!(changed.join("\n").contains("Draft changed."));
        app.snapshot.system_proxy.pending_restore = true;
        assert!(screen(&app, width, height)[0].contains("RECOVERY REQUIRED"));
        app.snapshot.system_proxy.pending_restore = false;
        app.snapshot.connected = true;
        app.snapshot.api_ready = false;
        let unavailable = screen(&app, width, height).join("\n");
        assert!(unavailable.contains("API UNAVAILABLE"));
        assert!(!unavailable.contains("RUNNING"));
        assert!(unavailable.contains("d Stop"));
    }
}

#[test]
fn long_group_lists_keep_selected_unicode_item_visible_and_fields_private() {
    let mut app = App::new(sample().unwrap(), true);
    let outbounds = app.snapshot.store.native.as_mut().unwrap()["outbounds"]
        .as_array_mut()
        .unwrap();
    for index in 0..250 {
        outbounds.push(
            json!({"type":"selector","tag":format!("Tokyo 東京 {index}"),"outbounds":["direct"]}),
        );
    }
    app.go(2, 0);
    app.selected[2] = app.rows().len() - 1;
    for (width, height) in [(54, 18), (80, 24), (120, 35)] {
        let rendered = screen(&app, width, height).join("\n");
        assert!(rendered.contains("249"));
        assert!(rendered.contains("Current") || rendered.contains("Default"));
        assert!(!rendered.contains("fictional@"));
    }
}

#[test]
fn dialogs_and_search_do_not_advertise_active_global_keys() {
    let mut app = App::new(sample().unwrap(), true);
    app.import(false);
    for (width, height) in [(54, 18), (80, 24), (120, 35)] {
        let rendered = screen(&app, width, height).join("\n");
        assert!(rendered.contains("global shortcuts paused"));
        assert!(rendered.contains("Review Import") && rendered.contains("Cancel"));
        assert!(!rendered.contains("c Start"));
    }
    app.dialog = None;
    app.searching = true;
    assert!(screen(&app, 80, 24).join("\n").contains("Filtering"));
}

#[test]
#[ignore = "writes fictional UI review SVGs to .build/ui-review; no manager or network"]
fn render_visual_review() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".build/ui-review");
    std::fs::create_dir_all(&directory).unwrap();
    for (name, width, height, page, tab) in [
        ("overview-80", 80, 24, 0, 0),
        ("overview-live-80", 80, 24, 0, 0),
        ("overview-live-120", 120, 35, 0, 0),
        ("overview-live-54", 54, 18, 0, 0),
        ("actions-80", 80, 24, 0, 0),
        ("proxies-80", 80, 24, 2, 0),
        ("proxies-120", 120, 35, 2, 0),
        ("routing-80", 80, 24, 3, 0),
        ("dns-80", 80, 24, 4, 0),
        ("import-80", 80, 24, 0, 0),
        ("narrow-54", 54, 18, 2, 0),
    ] {
        let mut a = App::new(sample().unwrap(), true);
        a.go(page, tab);
        if name.starts_with("overview-live") {
            live_overview(&mut a);
        }
        if name == "actions-80" {
            a.activate(Command::Actions).unwrap();
        }
        if name.starts_with("proxies") || name.starts_with("narrow") {
            let outbounds = a.snapshot.store.native.as_mut().unwrap()["outbounds"]
                .as_array_mut()
                .unwrap();
            let members =
                outbounds.iter().find(|v| v["type"] == "selector").unwrap()["outbounds"].clone();
            for group in ["Streaming", "Work"] {
                outbounds.push(json!({"type":"selector","tag":group,"outbounds":members}));
            }
        }
        if name.starts_with("import") {
            a.import(false);
        }
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| view::draw(f, &a)).unwrap();
        let mut svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\"><rect width=\"100%\" height=\"100%\" fill=\"#11171e\"/><g font-family=\"Menlo,DejaVu Sans Mono,monospace\" font-size=\"16\" xml:space=\"preserve\">", width as usize * 10 + 32, height as usize * 21 + 32);
        for (index, cell) in terminal.backend().buffer().content.iter().enumerate() {
            let x = 16 + index % width as usize * 10;
            let y = 16 + index / width as usize * 21;
            let bg = color(cell.bg, "#11171e");
            let fg = color(cell.fg, "#dae2e9");
            let opacity = if cell.modifier.contains(Modifier::DIM) {
                "0.5"
            } else {
                "1"
            };
            let weight = if cell.modifier.contains(Modifier::BOLD) {
                "bold"
            } else {
                "normal"
            };
            let decoration = if cell.modifier.contains(Modifier::UNDERLINED) {
                "underline"
            } else {
                "none"
            };
            svg.push_str(&format!("<rect x=\"{x}\" y=\"{y}\" width=\"10\" height=\"21\" fill=\"{bg}\"/><text x=\"{x}\" y=\"{}\" fill=\"{fg}\" opacity=\"{opacity}\" font-weight=\"{weight}\" text-decoration=\"{decoration}\">{}</text>", y+16, xml(cell.symbol())));
        }
        svg.push_str("</g></svg>");
        std::fs::write(directory.join(format!("{name}.svg")), svg).unwrap();
    }
}
