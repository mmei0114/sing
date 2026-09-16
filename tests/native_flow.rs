//! Isolated native-draft lifecycle with the real core. No public network / TUN.
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
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
    let mut s = String::new();
    BufReader::new(socket).read_line(&mut s).unwrap();
    serde_json::from_str(&s).unwrap()
}
fn ok(dir: &Path, a: &str, d: Value) -> Value {
    let r = rpc(dir, a, d);
    assert_eq!(r["ok"], true, "{a}: {r}");
    r
}
struct Guard<'a>(&'a Path, Child);
impl Drop for Guard<'_> {
    fn drop(&mut self) {
        if UnixStream::connect(self.0.join("manager.sock")).is_ok() {
            let _ = rpc(self.0, "shutdown", Value::Null);
        }
        let _ = self.1.kill();
        let _ = self.1.wait();
    }
}
#[test]
#[ignore = "Explicit isolated first-connection flow; loopback core only, no system proxy or TUN"]
fn subscription_setup_save_then_explicit_start() {
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
    let mut guard = Guard(dir, child);
    for _ in 0..100 {
        if UnixStream::connect(dir.join("manager.sock")).is_ok() {
            break;
        }
        assert!(guard.1.try_wait().unwrap().is_none());
        std::thread::sleep(Duration::from_millis(30));
    }
    let port = || {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    };
    let mut settings = ok(dir, "snapshot", Value::Null)["snapshot"]["store"]["settings"].clone();
    settings["core"] = json!(core);
    settings["port"] = json!(port());
    settings["api_port"] = json!(port());
    ok(dir, "save_settings", settings);
    let p=ok(dir,"import",json!({"source":"trojan://fixture-private@127.0.0.1:9#Fixture","name":"Fixture","user_agent":""}))["preview"].clone();
    assert!(!p.to_string().contains("fixture-private"));
    ok(
        dir,
        "commit_subscriptions",
        json!({"id":p["id"],"revision":p["revision"]}),
    );
    let info = ok(dir, "connection_setup_info", Value::Null)["edit"].clone();
    let setup =
        json!({"revision":info["revision"],"target":"direct","mode":"rule","capture":"port"});
    let review = ok(dir, "review_connection_setup", setup);
    assert!(!review.to_string().contains("fixture-private"));
    assert!(!dir.join("pre-native-state.json").exists());
    assert!(ok(dir, "snapshot", Value::Null)["snapshot"]["store"]["native"].is_null());
    let ready = ok(
        dir,
        "save_connection_setup",
        review["confirm"]["data"].clone(),
    );
    assert_eq!(
        ok(dir, "snapshot", Value::Null)["snapshot"]["connected"],
        false
    );
    assert!(dir.join("pre-native-state.json").exists());
    assert_eq!(ready["confirm"]["action"], "apply_native");
    ok(dir, "apply_native", ready["confirm"]["data"].clone());
    let snapshot = ok(dir, "snapshot", Value::Null)["snapshot"].clone();
    assert_eq!(snapshot["connected"], true);
    assert_eq!(snapshot["api_ready"], true);
    assert_eq!(snapshot["running_tun"], false);
    assert_eq!(snapshot["running_settings"]["mode"], "port");
    ok(dir, "disconnect", Value::Null);
}
#[test]
#[ignore = "Real core + loopback-only manager; no public endpoints or system changes"]
fn native_migration_edit_apply_conflict_and_rollback() {
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
    let mut guard = Guard(dir, child);
    for _ in 0..100 {
        if UnixStream::connect(dir.join("manager.sock")).is_ok() {
            break;
        }
        assert!(guard.1.try_wait().unwrap().is_none());
        std::thread::sleep(Duration::from_millis(30));
    }
    let port = || {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    };
    let mut s = ok(dir, "snapshot", Value::Null)["snapshot"]["store"]["settings"].clone();
    s["core"] = json!(core);
    s["port"] = json!(port());
    s["api_port"] = json!(port());
    ok(dir, "save_settings", s);
    ok(
        dir,
        "import",
        json!({"source":"trojan://test-secret@127.0.0.1:9#Fixture","name":"Fixture","user_agent":""}),
    );
    ok(dir, "commit_import", Value::Null);
    let review = ok(dir, "review_migration", Value::Null);
    assert!(!review.to_string().contains("test-secret"));
    assert!(!dir.join("pre-native-state.json").exists());
    ok(dir, "adopt_native", review["confirm"]["data"].clone());
    let backup: Value =
        serde_json::from_slice(&std::fs::read(dir.join("pre-native-state.json")).unwrap()).unwrap();
    assert_eq!(backup["schema"], 1);
    assert!(backup["native"].is_null());
    let snap = ok(dir, "snapshot", Value::Null);
    assert_eq!(snap["snapshot"]["manager_protocol"], 9);
    assert!(!snap.to_string().contains("test-secret"));
    assert_eq!(snap["snapshot"]["connected"], false);
    let mut edit = ok(dir, "read_native", json!(""))["edit"].clone();
    let d = &mut edit["value"];
    d["dns"] =
        json!({"servers":[{"type":"local","tag":"bootstrap"}],"final":"bootstrap","timeout":"3s"});
    d["route"]["rules"] = json!([]);
    d["route"]["final"] = json!("direct");
    d["outbounds"].as_array_mut().unwrap().push(
        json!({"type":"selector","tag":"nested","outbounds":["proxy","direct"],"default":"direct"}),
    );
    ok(dir, "write_native", edit.clone());
    assert_eq!(rpc(dir, "write_native", edit)["ok"], false);
    let dns_before = ok(dir, "read_native", json!("/dns"))["edit"]["value"].clone();
    ok(
        dir,
        "import_rules",
        json!({"source":"HOST-SUFFIX,media.example.invalid,ExternalPolicy","name":"Media","format":"qx","target":"nested"}),
    );
    ok(dir, "commit_rules", Value::Null);
    assert_eq!(
        ok(dir, "read_native", json!("/dns"))["edit"]["value"],
        dns_before
    );
    assert_eq!(
        ok(dir, "read_native", json!("/route/rules"))["edit"]["value"][0]["outbound"],
        "nested"
    );
    // Validate protocol-specific DNS fields with the pinned real core, without
    // starting these resolvers or sending requests to public services.
    // The product worksheet saves its group, rule set and binding in one draft
    // transaction. Check the real manager boundary and pinned core syntax.
    let draft = ok(dir, "read_native", json!(""))["edit"].clone();
    let preview = ok(
        dir,
        "prepare_rule_draft",
        json!({
            "source":"HOST-SUFFIX,worksheet.invalid,IgnoredPolicy", "name":"Worksheet",
            "format":"qx", "revision":draft["revision"]
        }),
    )["rules_preview"]
        .clone();
    let mut commit = json!({
        "id":preview["draft_id"],"revision":draft["revision"],"target":"worksheet-group","position":999,
        "group":{"revision":draft["revision"],"original_tag":null,"name":"Worksheet Media",
            "value":{"type":"selector","tag":"worksheet-group","outbounds":["nested","direct"],"default":"direct"}}
    });
    assert_eq!(rpc(dir, "commit_rule_draft", commit.clone())["ok"], false);
    assert_eq!(
        ok(dir, "read_native", json!(""))["edit"]["value"],
        draft["value"]
    );
    commit["position"] = json!(0);
    ok(dir, "commit_rule_draft", commit);
    assert_eq!(
        ok(dir, "read_native", json!("/dns"))["edit"]["value"],
        dns_before
    );
    assert_eq!(
        ok(dir, "read_native", json!("/route/rules"))["edit"]["value"][0]["outbound"],
        "worksheet-group"
    );
    let native_source = json!({"version":3,"rules":[{"type":"logical","mode":"and","rules":[{"domain_suffix":["native.invalid"]},{"process_name":["Fixture"]}]}]});
    let revision = ok(dir, "read_native", json!("/route"))["edit"]["revision"].clone();
    let preview = ok(dir,"prepare_rule_draft",json!({"source":native_source.to_string(),"format":"auto","name":"Native JSON","revision":revision}))["rules_preview"].clone();
    assert_eq!(preview["count"], 1);
    ok(
        dir,
        "commit_rule_draft",
        json!({"id":preview["draft_id"],"revision":revision,"target":"direct","position":0,"group":null}),
    );
    assert!(
        ok(dir, "read_native", json!("/route/rule_set"))["edit"]["value"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["rules"] == native_source["rules"])
    );
    let source = dir.join("native-source.json");
    let binary = dir.join("native-binary.srs");
    std::fs::write(
        &source,
        br#"{"version":1,"rules":[{"domain_suffix":["srs.example.invalid"]}]}"#,
    )
    .unwrap();
    let compiled = Command::new(std::env::var("SING_TEST_CORE").unwrap())
        .args(["rule-set", "compile"])
        .arg(&source)
        .arg("--output")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let revision = ok(dir, "read_native", json!("/route"))["edit"]["revision"].clone();
    let p = ok(
        dir,
        "prepare_rule_draft",
        json!({"source":binary,"name":"Native Binary","format":"native-srs","revision":revision}),
    )["rules_preview"]
        .clone();
    ok(
        dir,
        "commit_rule_draft",
        json!({"id":p["draft_id"],"revision":p["revision"],"target":"nested","position":0,"group":null}),
    );
    let sets = ok(dir, "read_native", json!("/route/rule_set"))["edit"]["value"].clone();
    assert!(sets
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["type"] == "local" && s["format"] == "binary"));
    assert_eq!(
        ok(dir, "read_native", json!("/dns"))["edit"]["value"],
        dns_before
    );
    ok(dir, "check", Value::Null);
    let dns_base = ok(dir, "read_native", json!("/dns"))["edit"]["value"].clone();
    for kind in ["udp", "tcp", "tls", "https", "quic", "h3", "fakeip"] {
        let mut e = ok(dir, "read_native", json!("/dns"))["edit"].clone();
        let server = if kind == "fakeip" {
            json!({"type":kind,"tag":"test-dns","inet4_range":"198.18.0.0/15","inet6_range":"fc00::/18"})
        } else if kind == "https" || kind == "h3" {
            json!({"type":kind,"tag":"test-dns","server":"resolver.invalid","server_port":8443,"path":"/custom-query","domain_resolver":"bootstrap","tls":{"server_name":"resolver.invalid"}})
        } else {
            json!({"type":kind,"tag":"test-dns","server":"127.0.0.1","server_port":15353})
        };
        e["value"]["servers"].as_array_mut().unwrap().push(server);
        ok(dir, "write_native", e);
        ok(dir, "check", Value::Null);
        let mut e = ok(dir, "read_native", json!("/dns"))["edit"].clone();
        e["value"] = dns_base.clone();
        ok(dir, "write_native", e);
    }
    ok(dir, "check", Value::Null);
    let r = ok(dir, "review_apply", Value::Null);
    ok(dir, "apply_native", r["confirm"]["data"].clone());
    let before_selection = ok(dir, "read_native", json!(""))["edit"].clone();
    ok(
        dir,
        "select_native",
        json!({"group":"nested","member":"proxy"}),
    );
    let snap = ok(dir, "snapshot", Value::Null);
    assert_eq!(snap["snapshot"]["dirty"], false);
    assert_eq!(snap["snapshot"]["connected"], true);
    assert_eq!(ok(dir, "read_native", json!(""))["edit"], before_selection);
    let live = |snap: &Value| {
        snap["snapshot"]["groups"]["group"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["tag"] == "nested")
            .unwrap()["selected"]
            .clone()
    };
    assert_eq!(live(&snap), "proxy");
    let remembered = std::fs::read(dir.join("selections.json")).unwrap();
    assert_eq!(
        rpc(
            dir,
            "select_native",
            json!({"group":"nested","member":"missing"})
        )["ok"],
        false
    );
    assert_eq!(
        std::fs::read(dir.join("selections.json")).unwrap(),
        remembered
    );
    let review = ok(dir, "review_apply", Value::Null);
    assert!(review["config"]
        .as_str()
        .unwrap()
        .contains("Selection recovery"));
    assert!(review["diff"].as_str().unwrap().contains("Native Diff"));
    ok(dir, "apply_native", review["confirm"]["data"].clone());
    assert_eq!(live(&ok(dir, "snapshot", Value::Null)), "proxy");
    ok(
        dir,
        "select_native",
        json!({"group":"nested","member":"direct"}),
    );
    let mut edit = ok(dir, "read_native", json!("/outbounds"))["edit"].clone();
    edit["value"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|g| g["tag"] == "nested")
        .unwrap()["default"] = json!("proxy");
    ok(dir, "write_native", edit);
    let review = ok(dir, "review_apply", Value::Null);
    ok(dir, "apply_native", review["confirm"]["data"].clone());
    assert_eq!(
        live(&ok(dir, "snapshot", Value::Null)),
        "proxy",
        "Explicit default must supersede remembered direct"
    );
    let mut edit = ok(dir, "read_native", json!("/dns"))["edit"].clone();
    edit["value"]["timeout"] = json!("4s");
    ok(dir, "write_native", edit);
    let snap = ok(dir, "snapshot", Value::Null);
    assert_eq!(snap["snapshot"]["dirty"], true);
    assert_eq!(snap["snapshot"]["connected"], true);
    let review = ok(dir, "review_apply", Value::Null);
    let mut edit = ok(dir, "read_native", json!("/dns"))["edit"].clone();
    edit["value"]["timeout"] = json!("5s");
    ok(dir, "write_native", edit);
    assert_eq!(
        rpc(dir, "apply_native", review["confirm"]["data"].clone())["ok"],
        false
    );
    let r = ok(dir, "review_apply", Value::Null);
    ok(dir, "apply_native", r["confirm"]["data"].clone());
    ok(dir, "rollback", Value::Null);
    let doc = ok(dir, "read_native", json!("/dns"))["edit"]["value"].clone();
    assert_eq!(doc["timeout"], "3s");
    let mut e = ok(dir, "read_native", json!("/outbounds/0"))["edit"].clone();
    e["value"]["outbounds"] = json!(["missing"]);
    ok(dir, "write_native", e);
    assert_eq!(rpc(dir, "check", Value::Null)["ok"], false);
    assert_eq!(
        ok(dir, "snapshot", Value::Null)["snapshot"]["connected"],
        true
    );
    ok(dir, "disconnect", Value::Null);
}
