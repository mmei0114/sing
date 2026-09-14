//! Durable, compare-and-restore system proxy transactions; OS backend is injectable.
use crate::model;
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub mod helper;
#[cfg(target_os = "macos")]
pub mod macos;

/// Preserve plist types (including unknown nested keys), not a lossy networksetup dump.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Plist {
    Bool(bool),
    Int(i64),
    Real(f64),
    String(String),
    Data(Vec<u8>),
    Array(Vec<Plist>),
    Dict(Dict),
}
pub type Dict = BTreeMap<String, Plist>;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Service {
    pub id: String,
    pub name: String,
    pub proxies: Option<Dict>,
}
#[derive(Clone, Debug)]
pub struct Change {
    pub id: String,
    pub expected: Option<Dict>,
    pub replacement: Option<Dict>,
}
#[derive(Debug)]
pub struct RemovedService;
impl std::fmt::Display for RemovedService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Network service was removed")
    }
}
impl std::error::Error for RemovedService {}
pub trait Backend {
    fn services(&mut self) -> Result<Vec<Service>>;
    fn read(&mut self, id: &str) -> Result<Option<Dict>>;
    /// All comparisons and writes must happen under the same OS preferences lock.
    fn transact(&mut self, changes: &[Change]) -> Result<()>;
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Entry {
    service: Service,
    applied: Dict,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Journal {
    schema: u32,
    owner: String,
    port: u16,
    released: bool,
    entries: Vec<Entry>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProxyStatus {
    pub supported: bool,
    pub helper_ready: bool,
    pub configured: bool,
    pub effective: bool,
    pub pending_restore: bool,
    pub safe_to_stop: bool,
    pub services: Vec<String>,
    pub detail: String,
}

fn number(d: &Dict, key: &str) -> bool {
    matches!(d.get(key),Some(Plist::Int(v)) if *v!=0) || d.get(key) == Some(&Plist::Bool(true))
}
pub fn points_to_local(d: &Dict, port: u16) -> bool {
    ["HTTP", "HTTPS", "SOCKS"].iter().any(|p| {
        number(d, &format!("{p}Enable"))
            && d.get(&format!("{p}Proxy")) == Some(&Plist::String("127.0.0.1".into()))
            && d.get(&format!("{p}Port")) == Some(&Plist::Int(port as i64))
    })
}
pub fn fully_local(d: &Dict, port: u16) -> bool {
    ["HTTP", "HTTPS", "SOCKS"].iter().all(|p| {
        number(d, &format!("{p}Enable"))
            && d.get(&format!("{p}Proxy")) == Some(&Plist::String("127.0.0.1".into()))
            && d.get(&format!("{p}Port")) == Some(&Plist::Int(port as i64))
    }) && !number(d, "ProxyAutoConfigEnable")
        && !number(d, "ProxyAutoDiscoveryEnable")
}
fn desired(old: Option<&Dict>, port: u16) -> Dict {
    let mut d = old.cloned().unwrap_or_default();
    for p in ["HTTP", "HTTPS", "SOCKS"] {
        d.insert(format!("{p}Enable"), Plist::Int(1));
        d.insert(format!("{p}Proxy"), Plist::String("127.0.0.1".into()));
        d.insert(format!("{p}Port"), Plist::Int(port as i64));
    }
    d.insert("ProxyAutoConfigEnable".into(), Plist::Int(0));
    d.insert("ProxyAutoDiscoveryEnable".into(), Plist::Int(0));
    d
}
fn group_keys(d: &Dict, prefix: &str) -> Dict {
    d.iter()
        .filter(|(k, _)| {
            if prefix == "HTTP" {
                k.starts_with("HTTP") && !k.starts_with("HTTPS")
            } else {
                k.starts_with(prefix)
            }
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

pub struct Controller {
    path: PathBuf,
    owner: String,
    journal: Option<Journal>,
}
impl Controller {
    pub fn load(path: &Path, owner: String) -> Result<Self> {
        let journal = if path.exists() {
            let j: Journal = serde_json::from_slice(&std::fs::read(path)?)
                .context("Recovery journal unreadable; refusing to overwrite it")?;
            ensure!(j.schema == 1, "Unknown proxy recovery journal version");
            ensure!(j.released||j.owner==owner,"System proxy belongs to another sing instance; restore it from that instance first");
            Some(j)
        } else {
            None
        };
        Ok(Self {
            path: path.into(),
            owner,
            journal,
        })
    }
    pub fn pending(&self) -> bool {
        self.journal.as_ref().is_some_and(|j| !j.released)
    }
    pub fn port(&self) -> Option<u16> {
        self.journal
            .as_ref()
            .filter(|j| !j.released)
            .map(|j| j.port)
    }
    fn persist(&self) -> Result<()> {
        if let Some(j) = &self.journal {
            model::atomic_write(&self.path, &serde_json::to_vec(j)?)?;
        }
        Ok(())
    }
    pub fn enable(&mut self, b: &mut impl Backend, port: u16) -> Result<ProxyStatus> {
        ensure!(port >= 1024, "Invalid local proxy port");
        ensure!(
            !self.pending(),
            "A system proxy recovery is pending; restore it first"
        );
        let services = b.services()?;
        ensure!(
            !services.is_empty(),
            "No enabled Wi-Fi / Ethernet network services in the current location"
        );
        for s in &services {
            if let Some(d) = &s.proxies {
                ensure!(!points_to_local(d,port),"{} already points to this local port; choose a different port or resolve the existing proxy first",s.name);
                ensure!(!d.keys().any(|k|k.to_lowercase().contains("auth")||k.to_lowercase().contains("user")||k.to_lowercase().contains("password")),"{} has authenticated proxy settings; automatic takeover is refused to protect credentials",s.name);
            }
        }
        let entries: Vec<_> = services
            .into_iter()
            .map(|s| Entry {
                applied: desired(s.proxies.as_ref(), port),
                service: s,
            })
            .collect();
        let changes = entries
            .iter()
            .map(|e| Change {
                id: e.service.id.clone(),
                expected: e.service.proxies.clone(),
                replacement: Some(e.applied.clone()),
            })
            .collect::<Vec<_>>();
        self.journal = Some(Journal {
            schema: 1,
            owner: self.owner.clone(),
            port,
            released: false,
            entries,
        });
        // Write-ahead journal: a commit/apply error may mean some changes reached the OS.
        self.persist()?;
        if let Err(e) = b.transact(&changes) {
            let recovery = self.restore(b);
            bail!(
                "Could not apply system proxy: {e}; recovery: {}",
                match recovery {
                    Ok(s) => s.detail,
                    Err(e) => e.to_string(),
                }
            );
        }
        let status = self.inspect(b)?;
        ensure!(
            status.configured,
            "System proxy changed before verification; recovery record retained"
        );
        Ok(status)
    }
    pub fn inspect(&self, b: &mut impl Backend) -> Result<ProxyStatus> {
        let mut s = ProxyStatus {
            supported: true,
            helper_ready: true,
            safe_to_stop: true,
            ..Default::default()
        };
        if let Some(j) = self.journal.as_ref().filter(|j| !j.released) {
            let mut unreadable = vec![];
            s.pending_restore = true;
            s.configured = true;
            for e in &j.entries {
                s.services.push(e.service.name.clone());
                let current = match b.read(&e.service.id) {
                    Ok(current) => current.unwrap_or_default(),
                    Err(error) if error.is::<RemovedService>() => {
                        s.configured = false;
                        continue;
                    }
                    Err(_) => {
                        unreadable.push(e.service.name.clone());
                        s.configured = false;
                        s.safe_to_stop = false;
                        continue;
                    }
                };
                s.configured &= fully_local(&current, j.port);
                s.safe_to_stop &= !points_to_local(&current, j.port);
            }
            s.detail = if s.configured {
                "System proxy configured; effective routing checked separately"
            } else {
                "System proxy changed externally; sing will not overwrite external changes"
            }
            .into();
            if !unreadable.is_empty() {
                s.detail = format!("Cannot verify network services (removed or unavailable): {}. Recovery record retained; review Network settings before stopping.", unreadable.join(", "));
            }
        } else {
            s.detail = "No system proxy owned by sing".into();
        }
        Ok(s)
    }
    pub fn restore(&mut self, b: &mut impl Backend) -> Result<ProxyStatus> {
        let Some(j) = self.journal.as_ref().filter(|j| !j.released) else {
            return self.inspect(b);
        };
        let mut changes = vec![];
        let mut conflicts = vec![];
        let mut unreadable = vec![];
        for e in &j.entries {
            let current = match b.read(&e.service.id) {
                Ok(current) => current,
                Err(error) if error.is::<RemovedService>() => continue,
                Err(_) => {
                    unreadable.push(e.service.name.clone());
                    continue;
                }
            };
            let mut updated = current.clone().unwrap_or_default();
            let before = e.service.proxies.clone().unwrap_or_default();
            for group in [
                "HTTP",
                "HTTPS",
                "SOCKS",
                "ProxyAutoConfig",
                "ProxyAutoDiscovery",
            ] {
                let now = group_keys(&updated, group);
                let expected = group_keys(&e.applied, group);
                let original = group_keys(&before, group);
                if now == expected {
                    for key in expected.keys() {
                        updated.remove(key);
                    }
                    updated.extend(original);
                } else if now != original {
                    conflicts.push(format!("{} / {}", e.service.name, group));
                }
            }
            let replacement = if updated.is_empty() && e.service.proxies.is_none() {
                None
            } else {
                Some(updated)
            };
            if replacement != current {
                changes.push(Change {
                    id: e.service.id.clone(),
                    expected: current,
                    replacement,
                });
            }
        }
        // Re-apply even an empty transaction: a previous persistent commit may have
        // succeeded while ApplyChanges failed. Recovery is not complete before Apply.
        b.transact(&changes)?;
        let mut status = self.inspect(b)?;
        if status.safe_to_stop {
            self.journal.as_mut().unwrap().released = true;
            self.persist()?;
            status.pending_restore = false;
            status.configured = false;
        }
        status.detail = if conflicts.is_empty() {
            "Original system proxy settings restored".into()
        } else {
            format!(
                "Restored unchanged proxy groups; left external changes untouched: {}{}",
                conflicts.join(", "),
                if status.safe_to_stop {
                    ""
                } else {
                    ". Some settings still point to sing: keep the core running and review macOS Network settings."
                }
            )
        };
        if !unreadable.is_empty() {
            status.detail = format!("Restored readable services; cannot verify {}. Recovery record retained; do not stop the core until Network settings are reviewed.", unreadable.join(", "));
        }
        Ok(status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Fake {
        items: BTreeMap<String, Option<Dict>>,
        fail_after: Option<usize>,
        writes: usize,
        unreadable: Option<String>,
    }
    impl Backend for Fake {
        fn services(&mut self) -> Result<Vec<Service>> {
            Ok(self
                .items
                .iter()
                .map(|(id, d)| Service {
                    id: id.clone(),
                    name: id.clone(),
                    proxies: d.clone(),
                })
                .collect())
        }
        fn read(&mut self, id: &str) -> Result<Option<Dict>> {
            ensure!(
                self.unreadable.as_deref() != Some(id),
                "Injected service read failure"
            );
            self.items
                .get(id)
                .cloned()
                .ok_or_else(|| RemovedService.into())
        }
        fn transact(&mut self, c: &[Change]) -> Result<()> {
            for ch in c {
                ensure!(self.read(&ch.id)? == ch.expected, "Concurrent edit");
            }
            for (i, ch) in c.iter().enumerate() {
                self.items.insert(ch.id.clone(), ch.replacement.clone());
                self.writes += 1;
                if self.fail_after == Some(i + 1) {
                    self.fail_after = None;
                    bail!("Injected partial apply failure");
                }
            }
            Ok(())
        }
    }
    fn fixture() -> Fake {
        Fake {
            items: BTreeMap::from([
                (
                    "Wi-Fi".into(),
                    Some(BTreeMap::from([
                        ("ProxyAutoConfigEnable".into(), Plist::Int(1)),
                        (
                            "ProxyAutoConfigURLString".into(),
                            Plist::String("https://example.invalid/proxy.pac".into()),
                        ),
                        (
                            "ExceptionsList".into(),
                            Plist::Array(vec![Plist::String("*.local".into())]),
                        ),
                        ("FTPPassive".into(), Plist::Int(1)),
                    ])),
                ),
                ("Ethernet".into(), None),
            ]),
            ..Default::default()
        }
    }
    fn control(d: &Path) -> Controller {
        Controller::load(&d.join("journal.json"), "test-user".into()).unwrap()
    }
    #[test]
    fn exact_roundtrip_and_idempotence() {
        let d = tempfile::tempdir().unwrap();
        let mut c = control(d.path());
        let mut b = fixture();
        let before = b.items.clone();
        assert!(c.enable(&mut b, 2080).unwrap().configured);
        assert!(c.restore(&mut b).unwrap().safe_to_stop);
        assert_eq!(b.items, before);
        c.restore(&mut b).unwrap();
        assert_eq!(b.items, before);
    }
    #[test]
    fn partial_apply_restores_every_written_service() {
        let d = tempfile::tempdir().unwrap();
        let mut c = control(d.path());
        let mut b = fixture();
        b.fail_after = Some(1);
        let before = b.items.clone();
        assert!(c.enable(&mut b, 2080).is_err());
        assert_eq!(b.items, before);
        assert!(!c.pending());
    }
    #[test]
    fn durable_recovery_after_process_loss() {
        let d = tempfile::tempdir().unwrap();
        let mut b = fixture();
        let before = b.items.clone();
        control(d.path()).enable(&mut b, 2080).unwrap();
        let mut fresh = control(d.path());
        assert!(fresh.pending());
        fresh.restore(&mut b).unwrap();
        assert_eq!(b.items, before);
    }
    #[test]
    fn unrelated_edits_survive_restore() {
        let d = tempfile::tempdir().unwrap();
        let mut c = control(d.path());
        let mut b = fixture();
        c.enable(&mut b, 2080).unwrap();
        b.items.get_mut("Wi-Fi").unwrap().as_mut().unwrap().insert(
            "ExceptionsList".into(),
            Plist::Array(vec![Plist::String("custom".into())]),
        );
        let r = c.restore(&mut b).unwrap();
        assert!(r.safe_to_stop);
        assert_eq!(
            b.items["Wi-Fi"].as_ref().unwrap()["ExceptionsList"],
            Plist::Array(vec![Plist::String("custom".into())])
        );
    }
    #[test]
    fn external_proxy_group_is_never_overwritten() {
        let d = tempfile::tempdir().unwrap();
        let mut c = control(d.path());
        let mut b = fixture();
        c.enable(&mut b, 2080).unwrap();
        b.items
            .get_mut("Wi-Fi")
            .unwrap()
            .as_mut()
            .unwrap()
            .insert("HTTPProxy".into(), Plist::String("another-proxy".into()));
        let r = c.restore(&mut b).unwrap();
        assert!(r.safe_to_stop);
        let p = b.items["Wi-Fi"].as_ref().unwrap();
        assert_eq!(p["HTTPProxy"], Plist::String("another-proxy".into()));
        assert_eq!(p["HTTPEnable"], Plist::Int(1));
        assert!(r.detail.contains("external"));
    }
    #[test]
    fn conflicting_group_still_using_local_port_blocks_stop() {
        let d = tempfile::tempdir().unwrap();
        let mut c = control(d.path());
        let mut b = fixture();
        c.enable(&mut b, 2080).unwrap();
        b.items
            .get_mut("Wi-Fi")
            .unwrap()
            .as_mut()
            .unwrap()
            .insert("HTTPUser".into(), Plist::String("changed".into()));
        assert!(!c.restore(&mut b).unwrap().safe_to_stop);
        assert!(c.pending());
    }
    #[test]
    fn unreadable_journal_and_other_owner_fail_closed() {
        let d = tempfile::tempdir().unwrap();
        let mut b = fixture();
        control(d.path()).enable(&mut b, 2080).unwrap();
        assert!(Controller::load(&d.path().join("journal.json"), "other-user".into()).is_err());
        std::fs::write(d.path().join("journal.json"), b"invalid").unwrap();
        assert!(Controller::load(&d.path().join("journal.json"), "test-user".into()).is_err());
    }
    #[test]
    fn backup_failure_prevents_all_os_writes() {
        let d = tempfile::tempdir().unwrap();
        let mut b = fixture();
        std::fs::write(d.path().join("not-directory"), b"x").unwrap();
        let mut c =
            Controller::load(&d.path().join("not-directory/journal"), "test-user".into()).unwrap();
        assert!(c.enable(&mut b, 2080).is_err());
        assert_eq!(b.writes, 0);
    }
    #[test]
    fn unavailable_service_does_not_prevent_other_services_being_restored() {
        let d = tempfile::tempdir().unwrap();
        let mut c = control(d.path());
        let mut b = fixture();
        let before = b.items["Wi-Fi"].clone();
        c.enable(&mut b, 2080).unwrap();
        b.unreadable = Some("Ethernet".into());
        let s = c.restore(&mut b).unwrap();
        assert_eq!(b.items["Wi-Fi"], before);
        assert!(!s.safe_to_stop);
        assert!(s.pending_restore);
        assert!(s.detail.contains("cannot verify"));
    }
    #[test]
    fn deleted_service_does_not_leave_a_permanent_recovery_lock() {
        let d = tempfile::tempdir().unwrap();
        let mut c = control(d.path());
        let mut b = fixture();
        let before = b.items["Wi-Fi"].clone();
        c.enable(&mut b, 2080).unwrap();
        b.items.remove("Ethernet");
        let s = c.restore(&mut b).unwrap();
        assert_eq!(b.items["Wi-Fi"], before);
        assert!(s.safe_to_stop);
        assert!(!s.pending_restore);
        assert!(!b.items.contains_key("Ethernet"));
    }
}
