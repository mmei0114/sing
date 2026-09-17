//! Explicit loopback-only integration tests. No real subscription or TUN.
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

fn rpc(dir: &Path, action: Value) -> Value {
    let mut s = UnixStream::connect(dir.join("manager.sock")).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    writeln!(s, "{action}").unwrap();
    let mut result = String::new();
    BufReader::new(s).read_line(&mut result).unwrap();
    serde_json::from_str(&result).unwrap()
}
struct Daemon {
    dir: PathBuf,
    child: Child,
}
impl Drop for Daemon {
    fn drop(&mut self) {
        if UnixStream::connect(self.dir.join("manager.sock")).is_ok() {
            let _ = rpc(&self.dir, json!({"action":"shutdown"}));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
#[ignore = "Isolated Unix socket regression; no core or network changes"]
fn manager_accepts_fragmented_request_frames() {
    let dir = tempfile::tempdir().unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_sing"))
        .args(["--daemon", "--data-dir"])
        .arg(dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut daemon = Daemon {
        dir: dir.path().into(),
        child,
    };
    for _ in 0..100 {
        if UnixStream::connect(dir.path().join("manager.sock")).is_ok() {
            break;
        }
        assert!(daemon.child.try_wait().unwrap().is_none());
        std::thread::sleep(Duration::from_millis(30));
    }
    // On macOS accepted sockets inherit O_NONBLOCK from the listener. A
    // short idle period/partial frame must not be mistaken for client EOF.
    for delay_before_write in [false, true] {
        let mut s = UnixStream::connect(dir.path().join("manager.sock")).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        if delay_before_write {
            std::thread::sleep(Duration::from_millis(120));
        }
        s.write_all(b"{\"action\":").unwrap();
        std::thread::sleep(Duration::from_millis(120));
        s.write_all(b"\"snapshot\"}\n").unwrap();
        let mut line = String::new();
        BufReader::new(s).read_line(&mut line).unwrap();
        let reply: Value =
            serde_json::from_str(&line).expect("complete response to fragmented request");
        assert_eq!(reply["ok"], true);
        assert_eq!(reply["snapshot"]["connected"], false);
    }
    // Activity's `f` saves a native route edit before offering Apply. Exercise
    // that sequence in the isolated profile; never start a core for this test.
    let migration = rpc(dir.path(), json!({"action":"review_migration"}));
    assert_eq!(migration["ok"], true, "{migration}");
    let adopted = rpc(dir.path(), migration["confirm"].clone());
    assert_eq!(adopted["ok"], true, "{adopted}");
    let read_route = json!({"action":"read_native","data":"/route"});
    let initial = rpc(dir.path(), read_route.clone());
    assert_eq!(initial["ok"], true);
    let mut edit = initial["edit"].clone();
    edit["value"]["find_process"] = json!(false);
    assert_eq!(
        rpc(dir.path(), json!({"action":"write_native","data":edit}))["ok"],
        true
    );
    let mut edit = rpc(dir.path(), read_route.clone())["edit"].clone();
    edit["value"]["find_process"] = json!(true);
    assert_eq!(
        rpc(dir.path(), json!({"action":"write_native","data":edit}))["ok"],
        true
    );
    assert_eq!(
        rpc(dir.path(), read_route)["edit"]["value"]["find_process"],
        true
    );
    assert_eq!(
        rpc(dir.path(), json!({"action":"snapshot"}))["snapshot"]["connected"],
        false
    );
    // Also use the production client transport, not only the test RPC helper.
    let status = Command::new(env!("CARGO_BIN_EXE_sing"))
        .args(["--status", "--data-dir"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["ok"], true);
    assert_eq!(status["snapshot"]["connected"], false);
}

#[test]
#[ignore = "Requires loopback socket access and SING_TEST_CORE"]
fn subscription_http_preview_persistence_and_background_core() {
    let core = std::env::var("SING_TEST_CORE").expect("Set SING_TEST_CORE");
    let dir = tempfile::tempdir().unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_sing"))
        .arg("--daemon")
        .arg("--data-dir")
        .arg(dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut daemon = Daemon {
        dir: dir.path().into(),
        child,
    };
    for _ in 0..100 {
        if UnixStream::connect(dir.path().join("manager.sock")).is_ok() {
            break;
        }
        assert!(daemon.child.try_wait().unwrap().is_none());
        std::thread::sleep(Duration::from_millis(30));
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for body in [
            "trojan://fake@127.0.0.1:9#Fixture",
            "trojan://fake@127.0.0.1:9#Fixture",
            "<html>expired</html>",
        ] {
            let (mut s, _) = listener.accept().unwrap();
            let mut req = [0; 4096];
            assert!(s.read(&mut req).unwrap() > 0);
            write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
    });
    let import = json!({"action":"import","data":{"source":format!("http://{address}/subscription?token=fixture-private"),"name":"Local test","user_agent":""}});
    let preview = rpc(dir.path(), import.clone());
    assert_eq!(preview["preview"]["count"], 1);
    assert_eq!(
        rpc(dir.path(), json!({"action":"snapshot"}))["snapshot"]["store"]["nodes"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        rpc(dir.path(), json!({"action":"cancel_import"}))["ok"],
        true
    );
    assert_eq!(rpc(dir.path(), import)["ok"], true);
    assert_eq!(
        rpc(dir.path(), json!({"action":"commit_import"}))["ok"],
        true
    );
    let snapshot = rpc(dir.path(), json!({"action":"snapshot"}));
    assert!(!snapshot.to_string().contains("fixture-private"));
    assert!(!snapshot.to_string().contains("\"fake\""));
    let id = snapshot["snapshot"]["store"]["subscriptions"][0]["id"].clone();
    assert_eq!(
        rpc(dir.path(), json!({"action":"refresh","data":id}))["ok"],
        false
    );
    assert_eq!(
        rpc(dir.path(), json!({"action":"snapshot"}))["snapshot"]["store"]["nodes"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let mut settings = snapshot["snapshot"]["store"]["settings"].clone();
    let free = || {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    };
    settings["core"] = json!(core);
    settings["port"] = json!(free());
    settings["api_port"] = json!(free());
    assert_eq!(
        rpc(
            dir.path(),
            json!({"action":"save_settings","data":settings})
        )["ok"],
        true
    );
    let connect = rpc(dir.path(), json!({"action":"connect"}));
    assert_eq!(connect["ok"], true, "{connect}");
    // Every RPC client closes; a fresh client must see the same live daemon/core.
    std::thread::sleep(Duration::from_millis(200));
    let fresh = rpc(dir.path(), json!({"action":"snapshot"}));
    assert_eq!(fresh["snapshot"]["connected"], true);
    assert_eq!(fresh["snapshot"]["api_ready"], true);
    assert_eq!(rpc(dir.path(), json!({"action":"logs"}))["ok"], true);
    assert_eq!(rpc(dir.path(), json!({"action":"disconnect"}))["ok"], true);
    server.join().unwrap();
    println!("PASS: HTTP subscription, staged import/cancel/commit, redaction, failed refresh, background core, native API, diagnostics and cleanup");
}

#[cfg(target_os = "macos")]
mod proxy_lifecycle {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    };
    #[derive(Default)]
    struct State {
        active: bool,
        port: u16,
        fail_enable: bool,
        fail_restore: bool,
        events: Vec<(String, bool)>,
    }
    struct FakeHelper {
        state: Arc<Mutex<State>>,
        done: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }
    impl FakeHelper {
        fn start(dir: &Path) -> Self {
            let listener =
                std::os::unix::net::UnixListener::bind(dir.join("system-proxy.sock")).unwrap();
            listener.set_nonblocking(true).unwrap();
            let state = Arc::new(Mutex::new(State::default()));
            let done = Arc::new(AtomicBool::new(false));
            let st = state.clone();
            let stop = done.clone();
            let thread = std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let (mut socket, _) = match listener.accept() {
                        Ok(s) => s,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(10));
                            continue;
                        }
                        Err(e) => panic!("{e}"),
                    };
                    socket
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut line = String::new();
                    BufReader::new(&socket).read_line(&mut line).unwrap();
                    let req: Value = serde_json::from_str(&line).unwrap();
                    let mut s = st.lock().unwrap();
                    let mut ok = true;
                    match req["action"].as_str().unwrap() {
                        "enable" => {
                            s.port = req["port"].as_u64().unwrap() as u16;
                            let ready = std::net::TcpStream::connect(("127.0.0.1", s.port)).is_ok();
                            s.events.push(("enable".into(), ready));
                            s.active = true;
                            ok = !s.fail_enable;
                        }
                        "restore" | "exit" => {
                            if s.active {
                                let ready =
                                    std::net::TcpStream::connect(("127.0.0.1", s.port)).is_ok();
                                s.events.push(("restore".into(), ready));
                            }
                            ok = !s.fail_restore;
                            if ok {
                                s.active = false;
                            }
                        }
                        "status" | "heartbeat" => {}
                        other => panic!("Unexpected helper action: {other}"),
                    }
                    writeln!(socket, "{}", json!({
                        "ok":ok,"error":if ok { "" } else { "Injected proxy transaction failure" },
                        "status":{"supported":true,"helper_ready":true,"configured":s.active,
                            "effective":s.active,"pending_restore":s.active,"safe_to_stop":!s.active,
                            "services":["MOCK Wi-Fi"],"detail":"Simulated proxy settings; no OS changes"}
                    })).unwrap();
                }
            });
            Self {
                state,
                done,
                thread: Some(thread),
            }
        }
    }
    impl Drop for FakeHelper {
        fn drop(&mut self) {
            self.done.store(true, Ordering::Relaxed);
            self.thread.take().unwrap().join().unwrap();
        }
    }

    #[test]
    #[ignore = "Real loopback core + MOCK system proxy helper; never writes OS proxy settings"]
    fn system_proxy_ordering_failure_recovery_and_shutdown() {
        let core = std::env::var("SING_TEST_CORE").expect("Set SING_TEST_CORE");
        let dir = tempfile::tempdir().unwrap();
        let helper = FakeHelper::start(dir.path());
        let child = Command::new(env!("CARGO_BIN_EXE_sing"))
            .args(["--daemon", "--data-dir"])
            .arg(dir.path())
            .env_remove("SSH_CONNECTION")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut daemon = Daemon {
            dir: dir.path().into(),
            child,
        };
        for _ in 0..100 {
            if UnixStream::connect(dir.path().join("manager.sock")).is_ok() {
                break;
            }
            assert!(daemon.child.try_wait().unwrap().is_none());
            std::thread::sleep(Duration::from_millis(30));
        }
        let source = json!({"action":"import","data":{"source":"trojan://fake@127.0.0.1:9#Fixture","name":"Mock","user_agent":""}});
        assert_eq!(rpc(dir.path(), source)["ok"], true);
        assert_eq!(
            rpc(dir.path(), json!({"action":"commit_import"}))["ok"],
            true
        );
        let mut settings =
            rpc(dir.path(), json!({"action":"snapshot"}))["snapshot"]["store"]["settings"].clone();
        let free = || {
            std::net::TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .port()
        };
        let port = free();
        settings["port"] = json!(port);
        settings["api_port"] = json!(free());
        settings["core"] = json!(core);
        settings["mode"] = json!("system");
        assert_eq!(
            rpc(
                dir.path(),
                json!({"action":"save_settings","data":settings})
            )["ok"],
            true
        );
        let connect = rpc(dir.path(), json!({"action":"connect"}));
        assert_eq!(connect["ok"], true, "{connect}");
        let snapshot = rpc(dir.path(), json!({"action":"snapshot"}));
        assert_eq!(snapshot["snapshot"]["system_proxy"]["configured"], true);
        assert_eq!(snapshot["snapshot"]["connectivity"]["state"], "not_checked");
        helper.state.lock().unwrap().fail_restore = true;
        let refused = Command::new(env!("CARGO_BIN_EXE_sing"))
            .arg("--data-dir")
            .arg(dir.path())
            .arg("--shutdown")
            .output()
            .unwrap();
        assert!(!refused.status.success());
        for action in ["disconnect", "shutdown"] {
            assert_eq!(rpc(dir.path(), json!({"action":action}))["ok"], false);
            assert!(daemon.child.try_wait().unwrap().is_none());
            assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_ok());
            assert!(dir.path().join("system-proxy.pending.json").exists());
        }
        helper.state.lock().unwrap().fail_restore = false;
        assert_eq!(
            rpc(dir.path(), json!({"action":"restore_proxy"}))["ok"],
            true
        );
        assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_ok());
        assert_eq!(
            rpc(dir.path(), json!({"action":"snapshot"}))["snapshot"]["dirty"],
            true
        );
        assert_eq!(rpc(dir.path(), json!({"action":"connect"}))["ok"], true);
        assert_eq!(rpc(dir.path(), json!({"action":"disconnect"}))["ok"], true);
        assert!(!dir.path().join("system-proxy.pending.json").exists());
        assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());
        helper.state.lock().unwrap().fail_enable = true;
        assert_eq!(rpc(dir.path(), json!({"action":"connect"}))["ok"], false);
        assert!(!helper.state.lock().unwrap().active);
        assert!(!dir.path().join("system-proxy.pending.json").exists());
        assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());
        assert!(helper
            .state
            .lock()
            .unwrap()
            .events
            .iter()
            .all(|(_, ready)| *ready));
        assert_eq!(rpc(dir.path(), json!({"action":"shutdown"}))["ok"], true);
        daemon.child.wait().unwrap();
        println!("PASS: proxy enable after core readiness, restore before stop, failed disconnect/shutdown keeps core and manager alive, restore-only, partial-enable rollback. OS settings untouched.");
    }
}
