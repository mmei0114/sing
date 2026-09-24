//! macOS CLI TUN needs an explicit system resolver handoff. The TUN DNS
//! endpoint exists in sing-box, but the CLI does not make it the macOS default.
#![cfg(target_os = "macos")]

use crate::{
    model,
    system_proxy::{macos::MacBackend, Change, Dict, Plist},
};
use anyhow::{bail, ensure, Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    fs::OpenOptions,
    io::Read,
    net::{Ipv4Addr, SocketAddrV4, UdpSocket},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

#[derive(Serialize, Deserialize)]
struct Entry {
    id: String,
    name: String,
    original: Option<Dict>,
}

#[derive(Serialize, Deserialize)]
struct Journal {
    schema: u32,
    address: String,
    entries: Vec<Entry>,
}

fn journal_path() -> Result<PathBuf> {
    let root = Path::new("/private/var/db/sing");
    if root.exists() {
        let m = fs::symlink_metadata(root)?;
        ensure!(
            m.is_dir() && !m.file_type().is_symlink() && m.uid() == 0 && m.mode() & 0o077 == 0,
            "Unsafe DNS recovery directory"
        );
    } else {
        model::private_dir(root)?;
    }
    Ok(root.join("tun-dns.json"))
}

pub fn lock() -> Result<fs::File> {
    let path = journal_path()?.with_file_name("tun-dns.lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    file.try_lock_exclusive()
        .context("Another sing instance owns macOS TUN DNS")?;
    Ok(file)
}

fn system_uses(address: &str) -> bool {
    let Ok(output) = Command::new("/usr/sbin/scutil").arg("--dns").output() else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let main = text
        .split("DNS configuration (for scoped queries)")
        .next()
        .unwrap_or("");
    let first = main
        .split("resolver #1")
        .nth(1)
        .unwrap_or("")
        .split("resolver #2")
        .next()
        .unwrap_or("");
    first
        .lines()
        .any(|line| line.trim() == format!("nameserver[0] : {address}"))
}

fn configured_servers(service: &str) -> Result<Vec<String>> {
    let output = Command::new("/usr/sbin/networksetup")
        .args(["-getdnsservers", service])
        .output()?;
    ensure!(
        output.status.success(),
        "Could not inspect DNS for {service}"
    );
    let text = String::from_utf8(output.stdout)?;
    if text
        .trim_start()
        .starts_with("There aren't any DNS Servers set on")
    {
        return Ok(vec![]);
    }
    let servers: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    ensure!(
        servers.iter().all(|s| s
            .split('%')
            .next()
            .is_some_and(|ip| ip.parse::<std::net::IpAddr>().is_ok())),
        "macOS returned an unreadable DNS status for {service}"
    );
    Ok(servers)
}

fn set_servers(service: &str, original: Option<&Dict>) -> Result<()> {
    let addresses = server_addresses(original)?;
    let mut command = Command::new("/usr/sbin/networksetup");
    command.args(["-setdnsservers", service]);
    if addresses.is_empty() {
        command.arg("Empty");
    } else {
        command.args(&addresses);
    }
    let output = command.output()?;
    ensure!(
        output.status.success(),
        "Could not restore DNS for {service}: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

pub fn address(config: &Value) -> Result<Option<String>> {
    let Some(tun) = config["inbounds"]
        .as_array()
        .and_then(|a| a.iter().find(|i| i["type"] == "tun"))
    else {
        return Ok(None);
    };
    if tun["dns_mode"] == "disabled" || tun["auto_route"] != true {
        return Ok(None);
    }
    if let Some(explicit) = tun["dns_address"].as_array() {
        let ip = explicit
            .iter()
            .filter_map(Value::as_str)
            .find(|s| s.parse::<Ipv4Addr>().is_ok())
            .context("TUN DNS needs an IPv4 dns_address for macOS system DNS")?;
        return Ok(Some(ip.into()));
    }
    let prefix = tun["address"]
        .as_array()
        .and_then(|a| {
            a.iter().filter_map(Value::as_str).find(|s| {
                s.split('/')
                    .next()
                    .is_some_and(|ip| ip.parse::<Ipv4Addr>().is_ok())
            })
        })
        .context("TUN DNS needs an IPv4 interface address")?;
    let ip: Ipv4Addr = prefix.split('/').next().unwrap().parse()?;
    let next = u32::from(ip)
        .checked_add(1)
        .context("Invalid TUN IPv4 DNS address")?;
    Ok(Some(Ipv4Addr::from(next).to_string()))
}

fn dns_query_works(address: &str) -> bool {
    let Ok(ip) = address.parse::<Ipv4Addr>() else {
        return false;
    };
    let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else {
        return false;
    };
    let _ = socket.set_read_timeout(Some(Duration::from_millis(250)));
    // A fixed, harmless DNS A query. Require a response from sing-box before
    // directing the system resolver at its TUN endpoint.
    let mut query: [u8; 29] = [
        0x71, 0x82, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 7, b'e', b'x', b'a', b'm', b'p', b'l', b'e', 3,
        b'c', b'o', b'm', 0, 0, 1, 0, 1,
    ];
    if fs::File::open("/dev/urandom")
        .and_then(|mut random| random.read_exact(&mut query[..2]))
        .is_err()
    {
        return false;
    }
    let target = SocketAddrV4::new(ip, 53);
    if socket.send_to(&query, target).is_err() {
        return false;
    }
    let mut reply = [0u8; 512];
    matches!(socket.recv_from(&mut reply), Ok((n, from)) if from == target.into() && n >= 12 && reply[..2] == query[..2] && reply[2] & 0x80 != 0 && reply[3] & 0x0f == 0)
}

pub fn enable(config: &Value) -> Result<()> {
    let Some(address) = address(config)? else {
        return Ok(());
    };
    let path = journal_path()?;
    ensure!(
        !path.exists(),
        "Previous DNS recovery is pending; restore it before starting TUN"
    );
    let mut ready = false;
    for _ in 0..24 {
        if dns_query_works(&address) {
            ready = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(125));
    }
    ensure!(
        ready,
        "TUN DNS endpoint {address} did not answer; system DNS was not changed"
    );
    let mut backend = MacBackend::new()?;
    let services = backend.dns_services()?;
    ensure!(
        !services.is_empty(),
        "No enabled physical network service for macOS DNS handoff"
    );
    ensure!(
        services.iter().all(
            |s| s.proxies.as_ref().and_then(|d| d.get("ServerAddresses"))
                != Some(&Plist::Array(vec![Plist::String(address.clone())]))
        ),
        "A network service already uses the TUN DNS address; restore its previous DNS first"
    );
    let entries: Vec<_> = services
        .iter()
        .map(|s| Entry {
            id: s.id.clone(),
            name: s.name.clone(),
            original: s.proxies.clone(),
        })
        .collect();
    // Write ahead of the system change, so interrupted starts remain recoverable.
    model::atomic_write(
        &path,
        &serde_json::to_vec(&Journal {
            schema: 1,
            address: address.clone(),
            entries,
        })?,
    )?;
    let changes: Vec<_> = services
        .into_iter()
        .map(|s| {
            let mut replacement = s.proxies.clone().unwrap_or_default();
            replacement.insert(
                "ServerAddresses".into(),
                Plist::Array(vec![Plist::String(address.clone())]),
            );
            Change {
                id: s.id,
                expected: s.proxies,
                replacement: Some(replacement),
            }
        })
        .collect();
    if let Err(error) = backend.transact_dns(&changes) {
        let recovery = restore();
        bail!(
            "Cannot apply TUN DNS: {error}; recovery: {}",
            recovery
                .map(|_| "restored".into())
                .unwrap_or_else(|e| e.to_string())
        );
    }
    for change in &changes {
        ensure!(
            backend.read_dns(&change.id)? == change.replacement,
            "TUN DNS was written but could not be verified; recovery record retained"
        );
    }
    for _ in 0..30 {
        if system_uses(&address) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let recovery = restore();
    bail!(
        "macOS did not use the TUN DNS after applying it; recovery: {}",
        recovery
            .map(|_| "restored".into())
            .unwrap_or_else(|e| e.to_string())
    );
}

pub fn restore() -> Result<()> {
    let path = journal_path()?;
    if !path.exists() {
        return Ok(());
    }
    let journal: Journal = serde_json::from_slice(&fs::read(&path)?)
        .context("DNS recovery record unreadable; original DNS was not changed")?;
    ensure!(journal.schema == 1, "Unknown DNS recovery record version");
    let mut backend = MacBackend::new()?;
    restore_entries(&mut backend, &journal)?;
    fs::remove_file(path)?;
    Ok(())
}

trait DnsSettings {
    fn read(&mut self, id: &str) -> Result<Option<Dict>>;
    fn name(&self, id: &str) -> Result<String>;
    fn transact(&mut self, changes: &[Change]) -> Result<()>;
    fn servers(&self, name: &str) -> Result<Vec<String>>;
    fn set_servers(&mut self, name: &str, original: Option<&Dict>) -> Result<()>;
}
impl DnsSettings for MacBackend {
    fn read(&mut self, id: &str) -> Result<Option<Dict>> {
        self.read_dns(id)
    }
    fn name(&self, id: &str) -> Result<String> {
        self.dns_service_name(id)
    }
    fn transact(&mut self, changes: &[Change]) -> Result<()> {
        self.transact_dns(changes)
    }
    fn servers(&self, name: &str) -> Result<Vec<String>> {
        configured_servers(name)
    }
    fn set_servers(&mut self, name: &str, original: Option<&Dict>) -> Result<()> {
        set_servers(name, original)
    }
}

fn server_addresses(value: Option<&Dict>) -> Result<Vec<String>> {
    match value.and_then(|d| d.get("ServerAddresses")) {
        Some(Plist::Array(values)) => values
            .iter()
            .map(|v| match v {
                Plist::String(s) => Ok(s.clone()),
                _ => bail!("Unsupported DNS address type"),
            })
            .collect(),
        None => Ok(vec![]),
        _ => bail!("Unsupported DNS address list"),
    }
}

fn restore_entries(backend: &mut impl DnsSettings, journal: &Journal) -> Result<()> {
    let mut changes = vec![];
    let mut targets = vec![];
    let managed = vec![journal.address.clone()];
    for entry in &journal.entries {
        let current = match backend.read(&entry.id) {
            Ok(c) => c,
            Err(e) if e.is::<crate::system_proxy::RemovedService>() => continue,
            Err(e) => return Err(e),
        };
        let current_servers = server_addresses(current.as_ref())?;
        let original_servers = server_addresses(entry.original.as_ref())?;
        let owned = current_servers == managed;
        // A previous attempt may have committed preferences, then failed to
        // apply or verify them. Retry verification instead of losing the journal.
        if !owned && current_servers != original_servers {
            continue;
        } // Respect changes made by the user while sing ran.
        targets.push(entry);
        if !owned {
            continue;
        }
        let mut replacement = current.clone().unwrap_or_default();
        if let Some(old) = entry
            .original
            .as_ref()
            .and_then(|d| d.get("ServerAddresses"))
        {
            replacement.insert("ServerAddresses".into(), old.clone());
        } else {
            replacement.remove("ServerAddresses");
        }
        changes.push(Change {
            id: entry.id.clone(),
            expected: current,
            replacement: (!replacement.is_empty()).then_some(replacement),
        });
    }
    if !changes.is_empty() {
        backend.transact(&changes)?
    }
    for entry in &targets {
        let name = backend.name(&entry.id)?;
        let expected_servers = server_addresses(entry.original.as_ref())?;
        let configured = backend.servers(&name)?;
        if configured == expected_servers {
            continue;
        }
        if configured != managed {
            continue;
        } // Another application has taken over.
        let current_servers = server_addresses(backend.read(&entry.id)?.as_ref())?;
        if current_servers != managed && current_servers != expected_servers {
            continue;
        }
        backend.set_servers(&name, entry.original.as_ref())?;
        if backend.servers(&name)? != expected_servers {
            bail!("Original DNS for {name} was not restored; recovery will retry");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    struct FakeSettings {
        preferences: Option<Dict>,
        effective: Vec<String>,
        fail_verify: bool,
        writes: usize,
    }
    impl DnsSettings for FakeSettings {
        fn read(&mut self, _: &str) -> Result<Option<Dict>> {
            Ok(self.preferences.clone())
        }
        fn name(&self, _: &str) -> Result<String> {
            Ok("Renamed Wi-Fi".into())
        }
        fn transact(&mut self, changes: &[Change]) -> Result<()> {
            for c in changes {
                ensure!(self.preferences == c.expected, "concurrent change");
                self.preferences = c.replacement.clone();
                self.writes += 1;
            }
            Ok(())
        }
        fn servers(&self, _: &str) -> Result<Vec<String>> {
            ensure!(!self.fail_verify, "temporary status failure");
            Ok(self.effective.clone())
        }
        fn set_servers(&mut self, name: &str, original: Option<&Dict>) -> Result<()> {
            assert_eq!(name, "Renamed Wi-Fi");
            self.effective = server_addresses(original)?;
            self.writes += 1;
            Ok(())
        }
    }
    fn journal() -> Journal {
        Journal {
            schema: 1,
            address: "172.19.0.2".into(),
            entries: vec![Entry {
                id: "service".into(),
                name: "Wi-Fi".into(),
                original: None,
            }],
        }
    }
    #[test]
    fn interrupted_restore_is_retried_after_preferences_were_committed() {
        let mut current = Dict::new();
        current.insert(
            "ServerAddresses".into(),
            Plist::Array(vec![Plist::String("172.19.0.2".into())]),
        );
        current.insert(
            "SearchDomains".into(),
            Plist::Array(vec![Plist::String("work.invalid".into())]),
        );
        let mut backend = FakeSettings {
            preferences: Some(current),
            effective: vec!["172.19.0.2".into()],
            fail_verify: true,
            writes: 0,
        };
        assert!(restore_entries(&mut backend, &journal()).is_err());
        assert!(server_addresses(backend.preferences.as_ref())
            .unwrap()
            .is_empty());
        assert_eq!(backend.effective, ["172.19.0.2"]);
        backend.fail_verify = false;
        restore_entries(&mut backend, &journal()).unwrap();
        assert!(backend.effective.is_empty());
        assert!(backend
            .preferences
            .as_ref()
            .unwrap()
            .contains_key("SearchDomains"));
        let writes = backend.writes;
        restore_entries(&mut backend, &journal()).unwrap();
        assert_eq!(backend.writes, writes, "repeat recovery must be harmless");
    }
    #[test]
    fn recovery_preserves_dns_changed_by_another_application() {
        let mut current = Dict::new();
        current.insert(
            "ServerAddresses".into(),
            Plist::Array(vec![Plist::String("9.9.9.9".into())]),
        );
        let mut backend = FakeSettings {
            preferences: Some(current),
            effective: vec!["9.9.9.9".into()],
            fail_verify: false,
            writes: 0,
        };
        restore_entries(&mut backend, &journal()).unwrap();
        assert_eq!(backend.effective, ["9.9.9.9"]);
        assert_eq!(backend.writes, 0);
    }
    #[test]
    fn dns_address_follows_tun_address() {
        assert_eq!(address(&json!({"inbounds":[{"type":"tun","address":["172.19.0.1/30","fdfe:dcba:9876::1/126"],"auto_route":true,"dns_mode":"hijack"}]})).unwrap(),Some("172.19.0.2".into()));
        assert_eq!(address(&json!({"inbounds":[{"type":"tun","address":["172.19.0.1/30"],"dns_address":["172.19.0.3"],"auto_route":true,"dns_mode":"native"}]})).unwrap(),Some("172.19.0.3".into()));
        assert_eq!(address(&json!({"inbounds":[{"type":"tun","address":["172.19.0.1/30"],"auto_route":true,"dns_mode":"disabled"}]})).unwrap(),None);
        assert_eq!(address(&json!({"inbounds":[{"type":"tun","address":["172.19.0.1/30"],"auto_route":false,"dns_mode":"hijack"}]})).unwrap(),None);
    }
}
