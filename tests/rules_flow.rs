//! Isolated manager/core contract test. Fictional nodes; no OS network changes.
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};
fn rpc(dir: &Path, action: &str, data: Value) -> Value {
    let mut socket = UnixStream::connect(dir.join("manager.sock")).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    writeln!(socket, "{}", json!({"action":action,"data":data})).unwrap();
    let mut line = String::new();
    BufReader::new(socket).read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}
fn ok(dir: &Path, action: &str, data: Value) -> Value {
    let r = rpc(dir, action, data);
    assert_eq!(r["ok"], true, "{action}: {r}");
    r
}
struct Manager<'a>(&'a Path, Child);
fn fake_http_node(label: &'static str) -> (u16, std::thread::JoinHandle<()>) {
    fake_http_node_for(label, "media.example.invalid")
}
fn fake_http_node_for(
    label: &'static str,
    expected: &'static str,
) -> (u16, std::thread::JoinHandle<()>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(25);
        let mut socket = loop {
            if let Ok((socket, _)) = listener.accept() {
                break socket;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "No request reached node {label}"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        socket.set_nonblocking(false).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::new(socket.try_clone().unwrap());
        let mut header = String::new();
        loop {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).unwrap() > 0);
            header.push_str(&line);
            if line == "\r\n" {
                break;
            }
        }
        assert!(header.contains(expected), "{header}");
        if header.starts_with("CONNECT ") {
            write!(socket, "HTTP/1.1 200 Connection Established\r\n\r\n").unwrap();
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                if line == "\r\n" {
                    break;
                }
            }
        }
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{label}",
            label.len()
        )
        .unwrap();
    });
    (port, server)
}

#[test]
#[ignore = "Real core, temporary loopback HTTP targets; no OS proxy, TUN or public traffic"]
fn routing_modes_and_observed_connection_close() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let core = std::env::var("SING_TEST_CORE").expect("Set SING_TEST_CORE");
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let child = Command::new(env!("CARGO_BIN_EXE_sing"))
        .args(["--daemon", "--data-dir"])
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut manager = Manager(dir, child);
    for _ in 0..100 {
        if UnixStream::connect(dir.join("manager.sock")).is_ok() {
            break;
        }
        assert!(manager.1.try_wait().unwrap().is_none());
        std::thread::sleep(Duration::from_millis(30));
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let done = Arc::new(AtomicBool::new(false));
    let stop = done.clone();
    let server = std::thread::spawn(move || {
        let mut workers = vec![];
        while !stop.load(Ordering::Relaxed) {
            let Ok((mut socket, _)) = listener.accept() else {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            };
            workers.push(std::thread::spawn(move || {
                socket.set_nonblocking(false).unwrap(); socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                let mut reader = BufReader::new(socket.try_clone().unwrap());
                loop {
                    loop { let mut line = String::new(); match reader.read_line(&mut line) {Ok(n) if n>0 => {}, _ => return}; if line == "\r\n" {break;} }
                    if write!(socket,"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: keep-alive\r\n\r\norigin").is_err() {return;}
                }
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }
    });
    let free = || {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    };
    let mut settings = ok(dir, "snapshot", Value::Null)["snapshot"]["store"]["settings"].clone();
    let port = free();
    settings["core"] = json!(core);
    settings["port"] = json!(port);
    settings["api_port"] = json!(free());
    settings["route_mode"] = json!("direct");
    settings["dns_policy"] = json!("legacy");
    settings["routing"] = json!("direct");
    settings["bypass_lan"] = json!(false);
    settings["rules"] = json!([{"kind":"ip_cidr","value":"127.0.0.1/32","target":"reject"}]);
    ok(dir, "save_settings", settings.clone());
    ok(dir, "connect", Value::Null);
    fn read_response(reader: &mut BufReader<std::net::TcpStream>) -> String {
        let mut length = 0;
        loop {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).unwrap() > 0);
            if let Some((key, value)) = line.split_once(':') {
                if key.eq_ignore_ascii_case("content-length") {
                    length = value.trim().parse::<usize>().unwrap();
                }
            }
            if line == "\r\n" {
                break;
            }
        }
        assert!(length < 1000);
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        String::from_utf8(body).unwrap()
    }
    let request = |path: &str| {
        let mut socket = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(socket,"GET http://{origin}/{path} HTTP/1.1\r\nHost: {origin}\r\nConnection: keep-alive\r\n\r\n").unwrap();
        BufReader::new(socket)
    };
    // CONNECT stays a tracked TCP tunnel; plain HTTP proxy requests may be
    // individually retired as soon as their response body completes.
    let tunnel = |path: &str| {
        let mut socket = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(
            socket,
            "CONNECT {origin} HTTP/1.1\r\nHost: {origin}\r\n\r\n"
        )
        .unwrap();
        let mut reader = BufReader::new(socket);
        let mut status = String::new();
        reader.read_line(&mut status).unwrap();
        assert!(status.contains("200"), "{status}");
        loop {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).unwrap() > 0);
            if line == "\r\n" {
                break;
            }
        }
        write!(
            reader.get_mut(),
            "GET /{path} HTTP/1.1\r\nHost: {origin}\r\nConnection: keep-alive\r\n\r\n"
        )
        .unwrap();
        reader
    };
    let mut first = tunnel("one");
    assert_eq!(read_response(&mut first), "origin");
    let mut second = tunnel("two");
    assert_eq!(read_response(&mut second), "origin");
    let source_port = first.get_ref().local_addr().unwrap().port();
    let report = ok(dir, "connections", Value::Null);
    let connection = report["connections"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| {
            c["source"]
                .as_str()
                .unwrap_or("")
                .ends_with(&format!(":{source_port}"))
                && c["closed_at"] == 0
        })
        .unwrap_or_else(|| panic!("Live connection must be observable: {report}"));
    assert_eq!(connection["outbound"], "direct");
    assert_eq!(connection["inbound"], "mixed-in");
    let id = connection["id"].clone();
    ok(dir, "close_connection", id.clone());
    let mut byte = [0];
    let closed = first.read(&mut byte);
    assert!(
        matches!(closed, Ok(0))
            || matches!(closed, Err(ref e) if e.kind() == std::io::ErrorKind::ConnectionReset),
        "Connection did not close: {closed:?}"
    );
    write!(
        second.get_mut(),
        "GET /still-open HTTP/1.1\r\nHost: {origin}\r\nConnection: keep-alive\r\n\r\n"
    )
    .unwrap();
    assert_eq!(read_response(&mut second), "origin");
    let report = ok(dir, "connections", Value::Null);
    assert!(!report["connections"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["id"] == id && c["closed_at"] == 0));
    drop(first);
    drop(second);
    // Saved route mode must not claim to be live before Apply.
    settings["route_mode"] = json!("rule");
    ok(dir, "save_settings", settings.clone());
    let s = ok(dir, "snapshot", Value::Null);
    assert_eq!(s["snapshot"]["running_settings"]["route_mode"], "direct");
    assert_eq!(s["snapshot"]["dirty"], true);
    let diag = ok(dir, "diagnostics", Value::Null);
    assert!(diag["config"].as_str().unwrap().contains("RUNNING"));
    assert!(diag["config"].as_str().unwrap().contains("SAVED"));
    // Add one fake node so rule/global mode can load. Global must override reject.
    let (node_port, node_server) = fake_http_node_for("node-global", "127.0.0.1");
    let source = json!({"outbounds":[{"type":"http","tag":"Fake","server":"127.0.0.1","server_port":node_port}]}).to_string();
    ok(
        dir,
        "import",
        json!({"source":source,"name":"Fixture","user_agent":""}),
    );
    ok(dir, "commit_import", Value::Null);
    let node = ok(dir, "snapshot", Value::Null)["snapshot"]["store"]["nodes"][0]["id"].clone();
    ok(
        dir,
        "save_group",
        json!({"id":"abcd","name":"Global group","kind":"selector","members":[node],"selected":node}),
    );
    // Running Direct does not load proxy selectors. Selections still save safely.
    ok(dir, "select", node.clone());
    ok(dir, "select_group", json!({"group":"abcd","node":node}));
    ok(dir, "connect", Value::Null);
    let mut rejected = request("rejected");
    let mut raw = String::new();
    if let Err(e) = rejected.read_to_string(&mut raw) {
        assert_eq!(
            e.kind(),
            std::io::ErrorKind::ConnectionReset,
            "Unexpected rejection result: {e}"
        );
    }
    assert!(!raw.contains("origin") && !raw.contains("node-global"));
    drop(rejected);
    settings["route_mode"] = json!("global");
    settings["global_target"] = json!("g-abcd");
    ok(dir, "save_settings", settings.clone());
    ok(dir, "connect", Value::Null);
    let mut proxied = request("global");
    assert_eq!(read_response(&mut proxied), "node-global");
    drop(proxied);
    node_server.join().unwrap();
    settings["bypass_lan"] = json!(true);
    ok(dir, "save_settings", settings.clone());
    ok(dir, "connect", Value::Null);
    let mut bypassed = request("lan-exception");
    assert_eq!(read_response(&mut bypassed), "origin");
    drop(bypassed);
    settings["route_mode"] = json!("direct");
    ok(dir, "save_settings", settings.clone());
    ok(dir, "connect", Value::Null);
    let s = ok(dir, "snapshot", Value::Null);
    assert_eq!(
        s["snapshot"]["store"]["settings"]["rules"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(s["snapshot"]["store"]["nodes"].as_array().unwrap().len(), 1);
    let mut renamed = s["snapshot"]["store"]["proxy_groups"][0].clone();
    renamed["name"] = json!("Pending rename");
    ok(dir, "save_group", renamed);
    ok(dir, "rollback", Value::Null);
    let restored = ok(dir, "snapshot", Value::Null);
    assert_eq!(
        restored["snapshot"]["running_settings"]["route_mode"],
        "global"
    );
    assert_eq!(
        restored["snapshot"]["store"]["proxy_groups"][0]["name"],
        "Global group"
    );
    ok(dir, "disconnect", Value::Null);
    done.store(true, Ordering::Relaxed);
    server.join().unwrap();
    println!("PASS: no-node direct, observed native connection metadata, close exactly one, saved/live distinction, rule reject, global override through named group, LAN exception, preserved policies, DNS generation, complete policy rollback");
}
impl Drop for Manager<'_> {
    fn drop(&mut self) {
        if self.0.join("manager.sock").exists() {
            let _ = rpc(self.0, "shutdown", Value::Null);
        }
        let _ = self.1.kill();
        let _ = self.1.wait();
    }
}
#[test]
#[ignore = "Loopback-only fake subscriptions and real SING_TEST_CORE; no system proxy or TUN"]
fn groups_rules_preview_refresh_dns_and_native_selection() {
    let core = std::env::var("SING_TEST_CORE").expect("Set SING_TEST_CORE");
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    let child = Command::new(env!("CARGO_BIN_EXE_sing"))
        .args(["--daemon", "--data-dir"])
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut manager = Manager(dir, child);
    for _ in 0..100 {
        if UnixStream::connect(dir.join("manager.sock")).is_ok() {
            break;
        }
        assert!(manager.1.try_wait().unwrap().is_none());
        std::thread::sleep(Duration::from_millis(30));
    }
    let (node_a, server_a) = fake_http_node("node-A");
    let (node_b, server_b) = fake_http_node("node-B");
    let node_source = json!({"outbounds":[{"type":"http","tag":"A","server":"127.0.0.1","server_port":node_a},{"type":"http","tag":"B","server":"127.0.0.1","server_port":node_b}]}).to_string();
    ok(
        dir,
        "import",
        json!({"source":node_source,"name":"Test","user_agent":""}),
    );
    ok(dir, "commit_import", Value::Null);
    let snap = ok(dir, "snapshot", Value::Null);
    let nodes = &snap["snapshot"]["store"]["nodes"];
    let group = json!({"id":"abcd","name":"Video","kind":"selector","members":[nodes[0]["id"],nodes[1]["id"]],"selected":nodes[0]["id"]});
    ok(dir, "save_group", group.clone());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for body in [
            "HOST-SUFFIX,example.com,OldPolicy\nUSER-AGENT,*youtube*,OldPolicy",
            "HOST-SUFFIX,example.com,OldPolicy\nUSER-AGENT,*youtube*,OldPolicy",
            "HOST-SUFFIX,example.invalid,NewPolicy",
            "<html>expired</html>",
        ] {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0; 4096];
            assert!(s.read(&mut buf).unwrap() > 0);
            write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
    let import = json!({"source":format!("http://{address}/rules?token=private-fixture"),"name":"YouTube","format":"auto","target":"g-abcd"});
    let p = ok(dir, "import_rules", import.clone());
    assert_eq!(p["rules_preview"]["count"], 1);
    assert_eq!(p["rules_preview"]["warnings"].as_array().unwrap().len(), 1);
    assert!(
        ok(dir, "snapshot", Value::Null)["snapshot"]["store"]["rule_resources"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    ok(dir, "cancel_rules", Value::Null);
    assert_eq!(rpc(dir, "commit_rules", Value::Null)["ok"], false);
    ok(dir, "import_rules", import);
    ok(dir, "commit_rules", Value::Null);
    let s = ok(dir, "snapshot", Value::Null);
    assert!(!s.to_string().contains("private-fixture"));
    let resource = s["snapshot"]["store"]["rule_resources"][0]["id"].clone();
    let mut binding = s["snapshot"]["store"]["rule_bindings"][0].clone();
    binding["target"] = json!("direct");
    ok(dir, "save_binding", binding.clone());
    let p = ok(dir, "refresh_rules", resource.clone());
    assert_eq!(p["rules_preview"]["added"], 1);
    assert_eq!(p["rules_preview"]["removed"], 1);
    assert_eq!(p["rules_preview"]["target"], "direct");
    ok(dir, "commit_rules", Value::Null);
    assert_eq!(rpc(dir, "refresh_rules", resource)["ok"], false);
    let s = ok(dir, "snapshot", Value::Null);
    assert_eq!(
        s["snapshot"]["store"]["rule_resources"][0]["rules"][0]["value"],
        "example.invalid"
    );
    binding["target"] = json!("g-abcd");
    ok(dir, "save_binding", binding.clone());
    assert_eq!(rpc(dir, "delete_group", json!("abcd"))["ok"], false);
    ok(
        dir,
        "import_rules",
        json!({"source":"DOMAIN-SUFFIX,blocked.invalid","name":"Reject fixture","format":"auto","target":"reject"}),
    );
    ok(dir, "commit_rules", Value::Null);
    let snap = ok(dir, "snapshot", Value::Null);
    let second = snap["snapshot"]["store"]["rule_bindings"][1].clone();
    ok(dir, "move_binding", json!({"id":second["id"],"delta":-1}));
    assert_eq!(
        ok(dir, "snapshot", Value::Null)["snapshot"]["store"]["rule_bindings"][0]["target"],
        "reject"
    );
    let free = || {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    };
    let mut settings = snap["snapshot"]["store"]["settings"].clone();
    settings["core"] = json!(core);
    settings["port"] = json!(free());
    settings["api_port"] = json!(free());
    settings["dns_policy"] = json!("paired");
    settings["routing"] = json!("direct"); // Only the imported rule may reach a test node.
    let proxy_port = settings["port"].as_u64().unwrap() as u16;
    ok(dir, "save_settings", settings);
    // Validate automatic groups without starting their public probe traffic.
    let mut auto = group.clone();
    auto["kind"] = json!("urltest");
    ok(dir, "save_group", auto);
    ok(dir, "check", Value::Null);
    ok(dir, "save_group", group.clone());
    ok(dir, "check", Value::Null);
    ok(dir, "connect", Value::Null);
    let request = || {
        let mut socket = std::net::TcpStream::connect(("127.0.0.1", proxy_port)).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(8)))
            .unwrap();
        write!(socket,"GET http://media.example.invalid/ HTTP/1.1\r\nHost: media.example.invalid\r\nConnection: close\r\n\r\n").unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).unwrap();
        response
    };
    assert!(request().ends_with("node-A"));
    ok(
        dir,
        "select_group",
        json!({"group":"abcd","node":nodes[1]["id"]}),
    );
    assert!(request().ends_with("node-B"));
    let snap = ok(dir, "snapshot", Value::Null);
    assert_eq!(snap["snapshot"]["connected"], true);
    assert_eq!(snap["snapshot"]["dirty"], false);
    let live = snap["snapshot"]["groups"]["group"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["tag"] == "g-abcd")
        .unwrap();
    assert_eq!(
        live["selected"],
        format!("n-{}", nodes[1]["id"].as_str().unwrap())
    );
    ok(dir, "disconnect", Value::Null);
    ok(dir, "delete_binding", binding["id"].clone());
    ok(dir, "delete_group", json!("abcd"));
    server.join().unwrap();
    server_a.join().unwrap();
    server_b.join().unwrap();
    println!("PASS: groups, private remote fetch, preview/cancel, policy override, update diff, failure preservation, target validation, ordering, paired DNS core check, urltest core check, native group switch and actual routed HTTP via nodes A then B");
}
