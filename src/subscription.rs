use crate::model::{clean, id, Node};
use anyhow::{anyhow, bail, Context, Result};
use base64::{
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD},
    Engine,
};
use percent_encoding::percent_decode_str;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};
use url::Url;

const LIMIT: usize = 8 * 1024 * 1024;
#[derive(Debug)]
pub struct Parsed {
    pub nodes: Vec<Node>,
    pub format: String,
    pub warnings: Vec<String>,
}
fn b64(s: &str) -> Result<String> {
    let t: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    for engine in [STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD] {
        if let Ok(v) = engine.decode(t.as_bytes()) {
            if let Ok(s) = String::from_utf8(v) {
                return Ok(s);
            }
        }
    }
    bail!("Invalid Base64 encoding")
}
fn dec(s: &str) -> String {
    percent_decode_str(s).decode_utf8_lossy().into_owned()
}
pub async fn fetch(source: &str, agent: &str, proxy_port: Option<u16>) -> Result<String> {
    let source = source.trim();
    if !source.starts_with("https://") && !source.starts_with("http://") {
        if let Some(encoded) = source.strip_prefix("sub://") {
            let actual = b64(encoded)?;
            if !actual.starts_with("https://") && !actual.starts_with("http://") {
                bail!("sub:// must contain an HTTP(S) subscription URL");
            }
            return Box::pin(fetch(&actual, agent, proxy_port)).await;
        }
        if source.starts_with("file://") || std::path::Path::new(source).is_file() {
            let path = if source.starts_with("file://") {
                Url::parse(source)?
                    .to_file_path()
                    .map_err(|_| anyhow!("Invalid file URL"))?
            } else {
                source.into()
            };
            if std::fs::metadata(&path)?.len() > LIMIT as u64 {
                bail!("Subscription exceeds 8 MiB");
            }
            return std::fs::read_to_string(path).context("Cannot read subscription file");
        }
        return Ok(source.into());
    }
    let url = Url::parse(source).context("Invalid subscription URL")?;
    if !url.username().is_empty() || url.password().is_some() {
        bail!("Credentials in URL authority are unsupported; use your provider's subscription URL");
    }
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent(if agent.trim().is_empty() {
            "sing-box/1.14.0 sing/0.1"
        } else {
            agent
        });
    if let Some(port) = proxy_port {
        builder = builder.proxy(reqwest::Proxy::all(format!("http://127.0.0.1:{port}"))?);
    }
    let mut res = builder
        .build()?
        .get(url)
        .send()
        .await
        .map_err(|e| anyhow!("Subscription download failed: {}", e.without_url()))?;
    if !res.status().is_success() {
        bail!(
            "Subscription server returned HTTP {}",
            res.status().as_u16()
        );
    }
    if res.content_length().unwrap_or(0) > LIMIT as u64 {
        bail!("Subscription exceeds 8 MiB");
    }
    let mut body = Vec::new();
    while let Some(chunk) = res
        .chunk()
        .await
        .map_err(|e| anyhow!("Download interrupted: {}", e.without_url()))?
    {
        if body.len() + chunk.len() > LIMIT {
            bail!("Subscription exceeds 8 MiB");
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).context("Subscription is not UTF-8 text")
}

pub fn parse(text: &str, provider: &str) -> Result<Parsed> {
    if text.len() > LIMIT {
        bail!("Subscription exceeds 8 MiB");
    }
    let text = text.trim().trim_start_matches('\u{feff}').trim();
    if text.is_empty() {
        bail!("Subscription is empty; previous nodes were kept");
    }
    let mut values: Vec<(String, Result<Value>)> = vec![];
    let format;
    let mut warnings = vec![];
    let object = serde_json::from_str::<Value>(text)
        .ok()
        .or_else(|| serde_yaml::from_str::<Value>(text).ok());
    if let Some(proxies) = object.as_ref().and_then(|v| v["proxies"].as_array()) {
        format = "Clash YAML";
        for (i, p) in proxies.iter().enumerate() {
            values.push((
                format!(
                    "Node {} ({})",
                    i + 1,
                    clean(p["name"].as_str().unwrap_or("unnamed"))
                ),
                clash(p),
            ));
        }
        warnings.push("Imported nodes only; subscription routing and DNS were not adopted.".into());
    } else if let Some(outbounds) = object.as_ref().and_then(|v| v["outbounds"].as_array()) {
        format = "sing-box JSON";
        for (i, p) in outbounds.iter().enumerate() {
            if ["selector", "urltest", "direct", "block", "dns"]
                .contains(&p["type"].as_str().unwrap_or(""))
            {
                continue;
            }
            values.push((
                format!(
                    "Node {} ({})",
                    i + 1,
                    clean(p["tag"].as_str().unwrap_or("unnamed"))
                ),
                Ok(p.clone()),
            ));
        }
        warnings.push(
            "Imported outbounds only; DNS, routes and groups remain managed by this client.".into(),
        );
    } else {
        let decoded = b64(text).ok();
        let body = decoded
            .as_deref()
            .filter(|s| s.contains("://"))
            .unwrap_or(text);
        format = if body == text {
            "Share links"
        } else {
            "Base64 subscription"
        };
        for (i, line) in body.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            values.push((format!("Line {}", i + 1), uri(line)));
        }
    }
    let mut nodes = vec![];
    let mut seen = HashSet::new();
    for (label, result) in values {
        let result = result.and_then(validate_node);
        match result {
            Ok(mut outbound) => {
                if outbound["tls"]["insecure"] == true {
                    warnings.push(format!(
                        "{label}: TLS certificate verification is disabled by this node."
                    ));
                }
                let name = clean(outbound["tag"].as_str().unwrap_or("Unnamed node"));
                outbound.as_object_mut().unwrap().remove("tag");
                let node_id = id(&format!(
                    "{}:{}",
                    provider,
                    serde_json::to_string(&outbound)?
                ));
                if !seen.insert(node_id.clone()) {
                    continue;
                }
                nodes.push(Node {
                    id: node_id,
                    name,
                    provider: provider.into(),
                    outbound,
                    favorite: false,
                });
            }
            Err(e) => warnings.push(format!("{label}: {e}")),
        }
    }
    if nodes.is_empty() {
        bail!(
            "No supported nodes found. {}",
            warnings
                .first()
                .map(String::as_str)
                .unwrap_or("Expected share links, Base64, Clash proxies or sing-box outbounds.")
        );
    }
    Ok(Parsed {
        nodes,
        format: format.into(),
        warnings,
    })
}

fn validate_node(v: Value) -> Result<Value> {
    reject_local_references(&v)?;
    let kind = v["type"].as_str().unwrap_or("");
    if ![
        "vless",
        "vmess",
        "trojan",
        "shadowsocks",
        "hysteria2",
        "tuic",
        "anytls",
        "socks",
        "http",
    ]
    .contains(&kind)
    {
        bail!("Unsupported protocol: {}", clean(kind));
    }
    if v["server"].as_str().unwrap_or("").is_empty() {
        bail!("Missing server");
    }
    if !(1..=65535).contains(&v["server_port"].as_u64().unwrap_or(0)) {
        bail!("Invalid port");
    }
    if v.get("detour").is_some() {
        bail!("Dependent outbound chains require a complete custom configuration");
    }
    if ["vless", "vmess", "tuic"].contains(&kind) {
        let uuid = v["uuid"].as_str().unwrap_or("");
        let raw = uuid.replace('-', "");
        if raw.len() != 32 || !raw.chars().all(|c| c.is_ascii_hexdigit()) {
            bail!("Invalid UUID");
        }
    }
    if ["trojan", "shadowsocks", "hysteria2", "tuic", "anytls"].contains(&kind)
        && v["password"].as_str().unwrap_or("").is_empty()
    {
        bail!("Missing password");
    }
    if let Some(t) = v["transport"]["type"].as_str() {
        if !["ws", "grpc", "http", "httpupgrade"].contains(&t) {
            bail!("Unsupported transport: {}", clean(t));
        }
    }
    Ok(v)
}

fn reject_local_references(v: &Value) -> Result<()> {
    match v {
        Value::Object(m) => {
            for (key, value) in m {
                if key.ends_with("_path")
                    || [
                        "certificate_provider",
                        "client_certificate_provider",
                        "bind_interface",
                        "routing_mark",
                    ]
                    .contains(&key.as_str())
                {
                    bail!(
                        "Local file / host-specific option is not accepted from subscriptions: {}",
                        clean(key)
                    );
                }
                reject_local_references(value)?;
            }
        }
        Value::Array(a) => {
            for v in a {
                reject_local_references(v)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn uri(s: &str) -> Result<Value> {
    let scheme = s.split("://").next().unwrap_or("");
    if scheme == "vmess" {
        let payload = b64(s.trim_start_matches("vmess://"))?;
        let p: Value = serde_json::from_str(&payload).context("Invalid VMess payload")?;
        if p["type"]
            .as_str()
            .is_some_and(|s| !s.is_empty() && s != "none")
        {
            bail!("VMess header obfuscation is not supported");
        }
        let mut node = json!({"type":"vmess","tag":p["ps"].as_str().unwrap_or("VMess"),"server":p["add"],"server_port":number(&p["port"]),"uuid":p["id"],"security":p["scy"].as_str().unwrap_or("auto"),"alter_id":number(&p["aid"])});
        let mut q = HashMap::new();
        for (key, field) in [
            ("type", "net"),
            ("path", "path"),
            ("host", "host"),
            ("sni", "sni"),
            ("fp", "fp"),
            ("alpn", "alpn"),
        ] {
            if let Some(s) = p[field].as_str() {
                q.insert(key.into(), s.into());
            }
        }
        if p["tls"].as_str() == Some("tls") {
            q.insert("security".into(), "tls".into());
        }
        tls_transport(&mut node, &q, false)?;
        return Ok(node);
    }
    if scheme == "ss" {
        let mut body = s.trim_start_matches("ss://").to_string();
        let (main, name) = body
            .split_once('#')
            .map(|(a, b)| (a.to_string(), dec(b)))
            .unwrap_or((body.clone(), "Shadowsocks".into()));
        if !main.contains('@') {
            body = format!("{}#{}", b64(&main)?, name);
        }
        let url = Url::parse(&format!("ss://{body}")).context("Invalid Shadowsocks URL")?;
        if url.query().is_some_and(|q| !q.is_empty()) {
            bail!("Shadowsocks plugin/query options are not yet supported");
        }
        let credentials = if let Some(p) = url.password() {
            format!("{}:{}", dec(url.username()), dec(p))
        } else {
            b64(&dec(url.username()))?
        };
        let (method, password) = credentials
            .split_once(':')
            .context("Invalid Shadowsocks credentials")?;
        return Ok(
            json!({"type":"shadowsocks","tag":url.fragment().map(dec).unwrap_or(name),"server":url.host_str().unwrap_or("").trim_matches(['[',']']),"server_port":url.port().unwrap_or(0),"method":method,"password":password}),
        );
    }
    let u = Url::parse(s).context("Invalid share link")?;
    let kind = match scheme {
        "hy2" | "hysteria2" => "hysteria2",
        "ss" => "shadowsocks",
        "socks5" => "socks",
        "vless" | "trojan" | "tuic" | "anytls" | "socks" | "http" | "https" => scheme,
        _ => bail!("Unsupported link protocol"),
    };
    let mut n = json!({"type":if kind=="https"{"http"}else{kind},"tag":u.fragment().map(dec).unwrap_or_else(||kind.into()),"server":u.host_str().unwrap_or("").trim_matches(['[',']']),"server_port":u.port().or(u.port_or_known_default()).unwrap_or(443)});
    let q: HashMap<String, String> = u.query_pairs().into_owned().collect();
    // Reject extensions whose omission could change connectivity or security.
    for key in q.keys() {
        if ![
            "security",
            "sni",
            "peer",
            "servername",
            "type",
            "path",
            "host",
            "serviceName",
            "mode",
            "fp",
            "pbk",
            "sid",
            "flow",
            "encryption",
            "allowInsecure",
            "insecure",
            "alpn",
            "obfs",
            "obfs-password",
            "congestion_control",
            "udp_relay_mode",
            "disable_sni",
            "remarks",
        ]
        .contains(&key.as_str())
        {
            bail!("Unsupported parameter: {}", clean(key));
        }
    }
    match kind {
        "vless" => {
            n["uuid"] = dec(u.username()).into();
            if let Some(flow) = q.get("flow").filter(|v| !v.is_empty()) {
                if flow != "xtls-rprx-vision" {
                    bail!("Unsupported VLESS flow");
                }
                n["flow"] = flow.clone().into();
            }
            if q.get("encryption")
                .is_some_and(|e| !e.is_empty() && e != "none")
            {
                bail!("Unsupported VLESS encryption");
            }
        }
        "trojan" | "hysteria2" | "anytls" => {
            n["password"] = if let Some(p) = u.password() {
                format!("{}:{}", dec(u.username()), dec(p))
            } else {
                dec(u.username())
            }
            .into();
        }
        "tuic" => {
            n["uuid"] = dec(u.username()).into();
            n["password"] = dec(u.password().unwrap_or("")).into();
            for k in ["congestion_control", "udp_relay_mode"] {
                if let Some(v) = q.get(k) {
                    n[k] = v.clone().into();
                }
            }
        }
        "http" | "https" | "socks" => {
            if !u.username().is_empty() {
                n["username"] = dec(u.username()).into();
                n["password"] = dec(u.password().unwrap_or("")).into();
            }
            if kind == "socks" {
                n["version"] = "5".into();
            }
        }
        _ => {}
    }
    if kind == "hysteria2" {
        if let Some(obfs) = q.get("obfs").filter(|s| !s.is_empty() && *s != "none") {
            if obfs != "salamander" {
                bail!("Unsupported Hysteria2 obfuscation");
            }
            n["obfs"] = json!({"type":obfs,"password":q.get("obfs-password").context("Missing obfuscation password")?});
        }
    }
    tls_transport(
        &mut n,
        &q,
        ["trojan", "hysteria2", "tuic", "anytls", "https"].contains(&kind),
    )?;
    Ok(n)
}

fn tls_transport(n: &mut Value, q: &HashMap<String, String>, default_tls: bool) -> Result<()> {
    let security = q
        .get("security")
        .map(String::as_str)
        .unwrap_or(if default_tls { "tls" } else { "none" });
    if !["none", "", "tls", "reality"].contains(&security) {
        bail!("Unsupported TLS security mode");
    }
    if ["tls", "reality"].contains(&security) {
        let insecure = q
            .get("allowInsecure")
            .or(q.get("insecure"))
            .is_some_and(|v| v == "1" || v == "true");
        let mut tls = json!({"enabled":true,"insecure":insecure});
        if let Some(sni) = q.get("sni").or(q.get("peer")).or(q.get("servername")) {
            if !sni.is_empty() {
                tls["server_name"] = sni.clone().into();
            }
        }
        if let Some(alpn) = q.get("alpn").filter(|s| !s.is_empty()) {
            tls["alpn"] = json!(alpn.split(',').collect::<Vec<_>>());
        }
        if let Some(fp) = q.get("fp").filter(|s| !s.is_empty()) {
            tls["utls"] = json!({"enabled":true,"fingerprint":fp});
        }
        if security == "reality" {
            tls["reality"] = json!({"enabled":true,"public_key":q.get("pbk").context("Missing REALITY public key")?,"short_id":q.get("sid").map(String::as_str).unwrap_or("")});
        }
        if q.get("disable_sni")
            .is_some_and(|v| v == "1" || v == "true")
        {
            tls["disable_sni"] = true.into();
        }
        n["tls"] = tls;
    }
    let transport = q.get("type").map(String::as_str).unwrap_or("tcp");
    if transport == "grpc"
        && q.get("mode")
            .is_some_and(|v| v != "gun" && !v.is_empty() && v != "single")
    {
        bail!("Unsupported gRPC transport mode");
    }
    match transport {
        "" | "tcp" | "none" => {}
        "ws" | "httpupgrade" | "http" => {
            let mut t =
                json!({"type":transport,"path":q.get("path").map(String::as_str).unwrap_or("/")});
            if let Some(host) = q.get("host").filter(|s| !s.is_empty()) {
                if transport == "ws" {
                    t["headers"] = json!({"Host":host});
                } else if transport == "http" {
                    t["host"] = json!([host]);
                } else {
                    t["host"] = host.clone().into();
                }
            }
            n["transport"] = t;
        }
        "grpc" => {
            n["transport"] = json!({"type":"grpc","service_name":q.get("serviceName").or(q.get("path")).map(String::as_str).unwrap_or("")});
        }
        _ => bail!("Unsupported transport: {}", clean(transport)),
    }
    Ok(())
}
fn number(v: &Value) -> u64 {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(0)
}

fn clash(p: &Value) -> Result<Value> {
    for key in p.as_object().context("Node must be an object")?.keys() {
        if ![
            "name",
            "type",
            "server",
            "port",
            "uuid",
            "password",
            "cipher",
            "alterId",
            "tls",
            "servername",
            "sni",
            "skip-cert-verify",
            "alpn",
            "client-fingerprint",
            "reality-opts",
            "flow",
            "network",
            "ws-opts",
            "grpc-opts",
            "http-opts",
            "h2-opts",
            "udp",
            "tfo",
            "mptcp",
            "congestion-controller",
            "udp-relay-mode",
            "obfs",
            "obfs-password",
            "username",
            "version",
        ]
        .contains(&key.as_str())
        {
            bail!("Unsupported Clash option: {}", clean(key));
        }
    }
    let kind = p["type"].as_str().context("Missing protocol")?;
    let kind = match kind {
        "ss" => "shadowsocks",
        "socks5" => "socks",
        s => s,
    };
    let mut n = json!({"type":kind,"tag":p["name"].as_str().unwrap_or("Unnamed"),"server":p["server"],"server_port":number(&p["port"])});
    for key in ["uuid", "password", "username", "flow"] {
        if let Some(v) = p.get(key) {
            n[key] = v.clone();
        }
    }
    if kind == "vmess" {
        n["security"] = p.get("cipher").cloned().unwrap_or(json!("auto"));
        n["alter_id"] = json!(number(&p["alterId"]));
    }
    if kind == "shadowsocks" {
        n["method"] = p["cipher"].clone();
    }
    if kind == "socks" {
        n["version"] = "5".into();
    }
    for (from, to) in [
        ("tfo", "tcp_fast_open"),
        ("mptcp", "tcp_multi_path"),
        ("congestion-controller", "congestion_control"),
        ("udp-relay-mode", "udp_relay_mode"),
    ] {
        if let Some(v) = p.get(from) {
            n[to] = v.clone();
        }
    }
    if let Some(obfs) = p.get("obfs") {
        if obfs != "salamander" {
            bail!("Unsupported obfuscation");
        }
        n["obfs"] = json!({"type":obfs,"password":p["obfs-password"]});
    }
    let mut q = HashMap::new();
    for (key, field) in [
        ("sni", "servername"),
        ("sni", "sni"),
        ("fp", "client-fingerprint"),
        ("type", "network"),
    ] {
        if let Some(v) = p[field].as_str() {
            q.insert(key.into(), v.into());
        }
    }
    if p["tls"]
        .as_bool()
        .unwrap_or(["trojan", "hysteria2", "tuic", "anytls"].contains(&kind))
    {
        q.insert("security".into(), "tls".into());
    }
    if p["skip-cert-verify"].as_bool() == Some(true) {
        q.insert("insecure".into(), "1".into());
    }
    if let Some(a) = p["alpn"].as_array() {
        q.insert(
            "alpn".into(),
            a.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    if let Some(r) = p.get("reality-opts") {
        q.insert("security".into(), "reality".into());
        q.insert(
            "pbk".into(),
            r["public-key"]
                .as_str()
                .context("Missing REALITY public key")?
                .into(),
        );
        q.insert("sid".into(), r["short-id"].as_str().unwrap_or("").into());
    }
    let network = p["network"].as_str().unwrap_or("tcp");
    if network == "grpc" {
        q.insert(
            "serviceName".into(),
            p["grpc-opts"]["grpc-service-name"]
                .as_str()
                .unwrap_or("")
                .into(),
        );
    }
    if network == "ws" {
        q.insert(
            "path".into(),
            p["ws-opts"]["path"].as_str().unwrap_or("/").into(),
        );
    }
    if network == "h2" || network == "http" {
        bail!("Clash HTTP/H2 options are not yet supported; use a sing-box subscription");
    }
    tls_transport(&mut n, &q, false)?;
    if network == "ws" {
        if let Some(h) = p["ws-opts"].get("headers") {
            n["transport"]["headers"] = h.clone();
        }
        for (a, b) in [
            ("max-early-data", "max_early_data"),
            ("early-data-header-name", "early_data_header_name"),
        ] {
            if let Some(v) = p["ws-opts"].get(a) {
                n["transport"][b] = v.clone();
            }
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    const V: &str="vless://11111111-1111-4111-8111-111111111111@example.com:443?security=tls&sni=example.com&type=ws&path=%2Fproxy#Hong%20Kong";
    #[test]
    fn fixture_formats() {
        for text in [
            include_str!("../fixtures/share-links.txt"),
            include_str!("../fixtures/clash.yaml"),
            include_str!("../fixtures/sing-box.json"),
        ] {
            assert!(!parse(text, "fixture").unwrap().nodes.is_empty());
        }
    }
    #[test]
    fn quic_and_anytls_links() {
        for (text,kind) in [("hy2://pass@example.com:443?obfs=salamander&obfs-password=obfs#HY2","hysteria2"),("tuic://11111111-1111-4111-8111-111111111111:pass@example.com:443?congestion_control=bbr#TUIC","tuic"),("anytls://pass@example.com:443#AnyTLS","anytls")]{let p=parse(text,"p").unwrap();assert_eq!(p.nodes[0].kind(),kind);assert_eq!(p.nodes[0].outbound["tls"]["enabled"],true);}
    }
    #[test]
    fn vmess_ws_base64() {
        let v = json!({"v":"2","ps":"VMess","add":"example.com","port":"443","id":"11111111-1111-4111-8111-111111111111","aid":"0","net":"ws","type":"none","path":"/ws","tls":"tls","alpn":"h2,http/1.1"});
        let p = parse(&format!("vmess://{}", STANDARD.encode(v.to_string())), "p").unwrap();
        assert_eq!(p.nodes[0].outbound["transport"]["path"], "/ws");
        assert_eq!(p.nodes[0].outbound["tls"]["alpn"][0], "h2");
    }
    #[test]
    fn reality_parameters_preserved() {
        let p=parse("vless://11111111-1111-4111-8111-111111111111@example.com:443?security=reality&pbk=test-public-key&sid=1234&fp=chrome&flow=xtls-rprx-vision#Reality","p").unwrap();
        assert_eq!(
            p.nodes[0].outbound["tls"]["reality"]["public_key"],
            "test-public-key"
        );
        assert_eq!(p.nodes[0].outbound["flow"], "xtls-rprx-vision");
    }
    #[test]
    fn imported_local_file_reference_rejected() {
        assert!(parse(r#"{"outbounds":[{"type":"trojan","server":"example.com","server_port":443,"password":"pass","tls":{"enabled":true,"certificate_path":"/private/secret"}}]}"#,"p").is_err());
    }
    #[test]
    fn insecure_tls_is_visible_warning() {
        let p = parse("trojan://pass@example.com:443?insecure=1#Unsafe", "p").unwrap();
        assert!(p
            .warnings
            .iter()
            .any(|w| w.contains("verification is disabled")));
    }
    #[test]
    fn formats_and_unicode() {
        let a = parse(V, "p").unwrap();
        let b = parse(&STANDARD_NO_PAD.encode(V), "p").unwrap();
        assert_eq!(a.nodes[0].id, b.nodes[0].id);
        assert_eq!(a.nodes[0].outbound["transport"]["path"], "/proxy");
        assert_eq!(a.nodes[0].name, "Hong Kong");
    }
    #[test]
    fn yaml_does_not_require_clash() {
        let p=parse("proxies:\n- name: 日本\n  type: trojan\n  server: example.com\n  port: 443\n  password: test\n", "p").unwrap();
        assert_eq!(p.nodes[0].kind(), "trojan");
        assert_eq!(p.nodes[0].name, "日本");
        assert_eq!(p.nodes[0].outbound["tls"]["enabled"], true);
    }
    #[test]
    fn no_region_filter_and_dedup() {
        let p = parse(
            &format!("{V}\n{V}\ntrojan://pass@example.net:443#Germany"),
            "p",
        )
        .unwrap();
        assert_eq!(p.nodes.len(), 2);
    }
    #[test]
    fn unsupported_is_reported() {
        let p = parse(
            &format!("{V}\nvless://abc@example.com:443?type=xhttp#unsupported"),
            "p",
        )
        .unwrap();
        assert_eq!(p.nodes.len(), 1);
        assert!(p.warnings[0].contains("Unsupported transport"));
    }
    #[test]
    fn json_urls_not_stripped_as_comments() {
        let p=parse(r#"{"outbounds":[{"type":"trojan","tag":"A","server":"example.com","server_port":443,"password":"https://credential"},{"type":"direct","tag":"direct"}]}"#,"p").unwrap();
        assert_eq!(p.nodes[0].outbound["password"], "https://credential");
    }
    #[test]
    fn ss_sip002() {
        let p = parse("ss://YWVzLTEyOC1nY206cGFzcw@example.com:8388#SS", "p").unwrap();
        assert_eq!(p.nodes[0].outbound["method"], "aes-128-gcm");
    }
    #[test]
    fn reject_empty_and_html() {
        assert!(parse("<html>login</html>", "p").is_err());
        assert!(parse("", "p").is_err());
    }
    #[test]
    fn source_and_name_identity() {
        let a = parse(V, "a").unwrap();
        let b = parse(V, "b").unwrap();
        assert_ne!(a.nodes[0].id, b.nodes[0].id);
        let c = parse(&V.replace("Hong%20Kong", "Renamed"), "a").unwrap();
        assert_eq!(a.nodes[0].id, c.nodes[0].id);
    }
}
