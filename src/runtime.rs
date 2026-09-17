use crate::{
    api, config,
    model::{self, Node, ProxyGroup, RuleBinding, RuleResource, Settings, Store, Subscription},
    native, ruleset, subscription,
    system_proxy::{helper as proxy_helper, ProxyStatus},
};
use anyhow::{bail, ensure, Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        net::{UnixListener, UnixStream},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};

pub const CORE_VERSION: &str = "1.14.0";
/// Bumped whenever the UI depends on new manager actions.
pub const PROTOCOL: u32 = 10;
mod controls;
mod cores;
mod selection;
pub use cores::{CoreInfo, CoreReport};
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(tag = "action", content = "data", rename_all = "snake_case")]
pub enum Action {
    ReviewMigration,
    AdoptNative {
        revision: String,
    },
    ReadNative(String),
    WriteNative(native::Edit),
    WriteGroup(native::GroupChange),
    PrepareRuleDraft {
        source: String,
        name: String,
        format: String,
        revision: String,
    },
    CommitRuleDraft {
        id: String,
        revision: String,
        target: String,
        position: usize,
        group: Option<native::GroupChange>,
    },
    ReviewApply,
    ApplyNative {
        revision: String,
    },
    SelectNative {
        group: String,
        member: String,
    },
    Snapshot,
    Connections,
    CloseConnection(String),
    Diagnostics,
    Import {
        source: String,
        name: String,
        user_agent: String,
    },
    Refresh(String),
    RefreshAll,
    ReprepareSubscriptions(String),
    ConnectionSetupInfo,
    ReviewConnectionSetup(native::ConnectionSetup),
    SaveConnectionSetup(native::ConnectionSetup),
    CommitSubscriptions {
        id: String,
        revision: String,
    },
    CancelSubscriptions(String),
    CommitImport,
    CancelImport,
    Delete(String),
    Favorite(String),
    Select(String),
    SaveSettings(Settings),
    Preview,
    Check,
    Connect,
    Disconnect,
    Test(String),
    InstallCore,
    Rollback,
    Shutdown,
    Logs,
    RestoreProxy,
    Probe,
    SaveGroup(ProxyGroup),
    DeleteGroup(String),
    SelectGroup {
        group: String,
        node: String,
    },
    ImportRules {
        source: String,
        name: String,
        format: String,
        target: String,
    },
    RefreshRules(String),
    CommitRules,
    CancelRules,
    CancelRuleDraft(String),
    SaveBinding(RuleBinding),
    DeleteBinding(String),
    MoveBinding {
        id: String,
        delta: i32,
    },
    /// Live Rule / Global / Direct switch; no restart.
    SetMode(String),
    /// Live Global-mode target through the generated GLOBAL selector.
    SetGlobalTarget(String),
    /// macOS system proxy on/off; live when the core is running.
    SetSystemProxy(bool),
    /// Add or park the TUN inbound. Takes effect after Apply.
    SetTun {
        enabled: bool,
        revision: String,
    },
    CoreReport {
        releases: bool,
    },
    InstallCoreVersion(String),
    SelectCore(String),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RulesPreview {
    #[serde(default)]
    pub draft_id: String,
    #[serde(default)]
    pub revision: String,
    pub name: String,
    pub format: String,
    pub input_count: usize,
    pub count: usize,
    pub added: usize,
    pub removed: usize,
    pub target: String,
    pub warnings: Vec<String>,
    pub policies: Vec<String>,
    pub sample: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProbeStatus {
    pub state: String,
    pub detail: String,
    pub checked_at: u64,
}
impl Default for ProbeStatus {
    fn default() -> Self {
        Self {
            state: "not_checked".into(),
            detail: "Internet access has not been checked".into(),
            checked_at: 0,
        }
    }
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ImportPreview {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub revision: String,
    #[serde(default)]
    pub sources: Vec<String>,
    pub name: String,
    pub format: String,
    pub count: usize,
    pub added: usize,
    pub removed: usize,
    pub warnings: Vec<String>,
    pub names: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Snapshot {
    /// Unix seconds when the running core became ready; 0 when stopped.
    #[serde(default)]
    pub started_at: u64,
    #[serde(default)]
    pub selection_recovery: String,
    #[serde(default)]
    pub manager_protocol: u32,
    #[serde(default)]
    pub system_proxy: ProxyStatus,
    #[serde(default)]
    pub connectivity: ProbeStatus,
    pub store: Store,
    pub connected: bool,
    pub api_ready: bool,
    pub core: String,
    pub version: String,
    pub running_settings: Option<Settings>,
    #[serde(default)]
    pub running_tun: bool,
    pub dirty: bool,
    pub status: api::Status,
    pub groups: api::Groups,
    pub activity: Vec<String>,
    pub host: String,
    pub ssh: bool,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Reply {
    #[serde(default)]
    pub diff: Option<String>,
    #[serde(default)]
    pub edit: Option<native::Edit>,
    #[serde(default)]
    pub confirm: Option<Action>,
    #[serde(default)]
    pub connections: Option<ConnectionReport>,
    #[serde(default)]
    pub rules_preview: Option<RulesPreview>,
    #[serde(default)]
    pub auth_kind: String,
    #[serde(default)]
    pub after_auth: Option<Action>,
    pub ok: bool,
    pub message: String,
    pub snapshot: Option<Snapshot>,
    pub preview: Option<ImportPreview>,
    pub config: Option<String>,
    pub needs_auth: bool,
    #[serde(default)]
    pub cores: Option<CoreReport>,
}
impl Reply {
    fn success(message: impl Into<String>) -> Self {
        Self {
            diff: None,
            edit: None,
            confirm: None,
            connections: None,
            rules_preview: None,
            auth_kind: String::new(),
            after_auth: None,
            ok: true,
            message: message.into(),
            snapshot: None,
            preview: None,
            config: None,
            needs_auth: false,
            cores: None,
        }
    }
    fn error(e: anyhow::Error) -> Self {
        let mut r = Self::success(model::clean_multiline(&format!("{e:#}")));
        r.ok = false;
        r
    }
}

#[derive(Clone, Default, Serialize, Deserialize, Debug)]
pub struct ConnectionReport {
    pub observed_at: u64,
    pub total: usize,
    pub items: Vec<api::Connection>,
}

pub fn request(dir: &Path, action: Action) -> Result<Reply> {
    let mut frame = serde_json::to_vec(&action).context("Could not encode manager request")?;
    frame.push(b'\n');
    let mut stream = UnixStream::connect(dir.join("manager.sock"))
        .context("Could not connect to manager. Check that sing is running.")?;
    stream.set_read_timeout(Some(Duration::from_secs(180)))?;
    stream.set_write_timeout(Some(Duration::from_secs(180)))?;
    stream
        .write_all(&frame)
        .context("Could not send request to manager")?;
    let mut line = String::new();
    BufReader::new(stream)
        .take(16 * 1024 * 1024)
        .read_line(&mut line)
        .context("Could not read manager response; check state before retrying changes")?;
    serde_json::from_str(&line).context("Manager returned an invalid response")
}

pub fn ensure_daemon(dir: &Path) -> Result<()> {
    model::private_dir(dir)?;
    if UnixStream::connect(dir.join("manager.sock")).is_ok() {
        return Ok(());
    }
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(dir.join("manager.log"))?;
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.arg("--data-dir")
        .arg(dir)
        .arg("--daemon")
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = cmd.spawn()?;
    for _ in 0..50 {
        if UnixStream::connect(dir.join("manager.sock")).is_ok() {
            return Ok(());
        }
        if child.try_wait()?.is_some() {
            bail!("Manager could not start; inspect manager.log in the data directory");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    bail!("Manager startup timed out")
}

#[derive(Clone)]
struct Pending {
    sub: Subscription,
    nodes: Vec<Node>,
}
#[derive(Clone)]
struct PendingSubscriptions {
    items: Vec<Pending>,
    id: String,
    revision: String,
}
struct PendingRules {
    native_set: Option<serde_json::Value>,
    resource: RuleResource,
    binding: RuleBinding,
    revision: String,
    id: String,
    unbound: bool,
}
pub(crate) struct Manager {
    selection_recovery: String,
    pending_rules: Option<PendingRules>,
    lease: proxy_helper::Lease,
    connectivity: ProbeStatus,
    dir: PathBuf,
    store: Store,
    child: Option<Child>,
    tun: bool,
    running: Option<Store>,
    pending: Option<PendingSubscriptions>,
    activity: Vec<String>,
    status: api::Status,
    groups: api::Groups,
    api_ready: bool,
    version: String,
    last_sample: u64,
    started_at: u64,
}
impl Drop for Manager {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
fn traffic_sample(
    previous: &api::Status,
    mut current: api::Status,
    last: u64,
    now: u64,
) -> api::Status {
    let elapsed = now.saturating_sub(last);
    current.traffic_available &= last != 0
        && elapsed > 0
        && current.uplink_total >= previous.uplink_total
        && current.downlink_total >= previous.downlink_total;
    current.uplink = current
        .uplink_total
        .saturating_sub(previous.uplink_total)
        .max(0)
        / elapsed.max(1) as i64;
    current.downlink = current
        .downlink_total
        .saturating_sub(previous.downlink_total)
        .max(0)
        / elapsed.max(1) as i64;
    current
}
impl Manager {
    fn log(&mut self, msg: impl Into<String>) {
        self.activity
            .push(format!("{}  {}", model::now(), model::clean(&msg.into())));
        if self.activity.len() > 200 {
            self.activity.remove(0);
        }
    }
    fn core(&self) -> Option<PathBuf> {
        find_core(&self.dir, &self.store.settings.core)
    }
    fn connected(&mut self) -> bool {
        if self.tun {
            return helper_request(&self.dir, "status")
                .map(|x| x == "running")
                .unwrap_or(false);
        }
        if let Some(child) = &mut self.child {
            if let Ok(Some(status)) = child.try_wait() {
                self.child = None;
                self.api_ready = false;
                self.log(format!(
                    "Core exited ({status}). Inspect core.log for details."
                ));
            }
        }
        self.child.is_some()
    }
    async fn api(&self) -> Result<api::Api> {
        let settings = &self.running.as_ref().context("Not connected")?.settings;
        api::Api::connect(settings.api_port, &self.store.secret).await
    }
    async fn snapshot(&mut self) -> Snapshot {
        let connected = self.connected();
        if !connected && self.version.is_empty() {
            if let Some(core) = self.core() {
                self.version = core_version(&core)
                    .await
                    .unwrap_or_else(|_| "Unable to read version".into());
            }
        }
        if connected {
            match self.api().await {
                Ok(mut api) => {
                    if let Ok(status) = api.status().await {
                        let now = model::now();
                        self.status = traffic_sample(&self.status, status, self.last_sample, now);
                        self.last_sample = now;
                        self.api_ready = true;
                    } else {
                        self.api_ready = false;
                        self.last_sample = 0;
                    }
                    if let Ok(groups) = api.groups().await {
                        self.groups = groups;
                    } else {
                        self.groups = api::Groups::default();
                    }
                }
                Err(_) => {
                    self.api_ready = false;
                    self.last_sample = 0;
                    self.groups = api::Groups::default();
                }
            }
        } else {
            self.api_ready = false;
            self.last_sample = 0;
            self.status = api::Status::default();
            self.groups = api::Groups::default();
        }
        let system_proxy = proxy_helper::status(
            &self.dir,
            self.running
                .as_ref()
                .map(|s| s.settings.port)
                .unwrap_or(self.store.settings.port),
        );
        let dirty = self
            .running
            .as_ref()
            .map(|r| {
                r.settings != self.store.settings
                    || r.native != self.store.native
                    || r.selected != self.store.selected
                    || r.proxy_groups != self.store.proxy_groups
                    || r.rule_bindings != self.store.rule_bindings
                    || r.rule_resources.iter().map(|s| (&s.id, &s.rules)).ne(self
                        .store
                        .rule_resources
                        .iter()
                        .map(|s| (&s.id, &s.rules)))
                    || r.nodes.iter().map(|n| (&n.id, &n.outbound)).ne(self
                        .store
                        .nodes
                        .iter()
                        .map(|n| (&n.id, &n.outbound)))
                    || (connected && r.settings.mode == "system" && !system_proxy.configured)
            })
            .unwrap_or(false);
        let mut store = self.store.clone();
        store.secret.clear();
        for sub in &mut store.subscriptions {
            sub.source = source_label(&sub.source);
            if !sub.user_agent.is_empty() {
                sub.user_agent = "[configured]".into();
            }
        }
        for resource in &mut store.rule_resources {
            resource.source = source_label(&resource.source);
        }
        for n in &mut store.nodes {
            n.outbound = config::redacted(&n.outbound);
        }
        store.native = store.native.as_ref().map(config::redacted);
        Snapshot {
            started_at: if connected { self.started_at } else { 0 },
            selection_recovery: self.selection_recovery.clone(),
            manager_protocol: PROTOCOL,
            system_proxy,
            connectivity: if connected {
                self.connectivity.clone()
            } else {
                ProbeStatus::default()
            },
            store,
            connected,
            api_ready: self.api_ready,
            core: self
                .core()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            version: self.version.clone(),
            running_settings: self.running.as_ref().map(|s| s.settings.clone()),
            running_tun: connected && self.running.as_ref().is_some_and(native::uses_tun),
            dirty,
            status: self.status.clone(),
            groups: self.groups.clone(),
            activity: self.activity.clone(),
            host: host(),
            ssh: std::env::var_os("SSH_CONNECTION").is_some(),
        }
    }
    async fn prepare(
        &mut self,
        source: String,
        name: String,
        user_agent: String,
        existing: Option<String>,
    ) -> Result<Reply> {
        self.pending = None;
        ensure!(
            !source.trim().is_empty(),
            "Paste a subscription URL or node URI"
        );
        let id = existing.unwrap_or_else(|| model::id(&source));
        let text = match subscription::fetch(&source, &user_agent, None).await {
            Ok(s) => s,
            Err(first) => {
                if self.connected() {
                    subscription::fetch(
                        &source,
                        &user_agent,
                        self.running.as_ref().map(|r| r.settings.port),
                    )
                    .await
                    .map_err(|_| first)?
                } else {
                    return Err(first);
                }
            }
        };
        let mut parsed = subscription::parse(&text, &id)?;
        for n in &mut parsed.nodes {
            if let Some(old) = self.store.nodes.iter().find(|old| old.id == n.id) {
                n.favorite = old.favorite;
            }
        }
        let old: Vec<_> = self
            .store
            .nodes
            .iter()
            .filter(|n| n.provider == id)
            .collect();
        let name = if name.trim().is_empty() {
            format!("Subscription {}", self.store.subscriptions.len() + 1)
        } else {
            model::clean(&name)
        };
        let p = ImportPreview {
            id: model::token()?,
            revision: native::revision(&self.store),
            sources: vec![],
            name: name.clone(),
            format: parsed.format.clone(),
            count: parsed.nodes.len(),
            added: parsed
                .nodes
                .iter()
                .filter(|n| !old.iter().any(|o| o.id == n.id))
                .count(),
            removed: old
                .iter()
                .filter(|n| !parsed.nodes.iter().any(|new| new.id == n.id))
                .count(),
            warnings: parsed.warnings.clone(),
            names: parsed
                .nodes
                .iter()
                .take(12)
                .map(|n| n.name.clone())
                .collect(),
        };
        self.pending = Some(PendingSubscriptions {
            id: p.id.clone(),
            revision: p.revision.clone(),
            items: vec![Pending {
                sub: Subscription {
                    id,
                    name,
                    source,
                    format: parsed.format,
                    updated_at: model::now(),
                    warnings: parsed.warnings,
                    user_agent,
                },
                nodes: parsed.nodes,
            }],
        });
        let mut r = Reply::success("Review the import before saving");
        r.preview = Some(p);
        Ok(r)
    }
    fn save(&mut self, mut store: Store) -> Result<()> {
        if store.native == self.store.native {
            native::reconcile(&self.store, &mut store)?;
        }
        store.save(&self.dir)?;
        self.store = store;
        Ok(())
    }
    fn commit_subscriptions(&mut self) -> Result<Reply> {
        let pending = self
            .pending
            .as_ref()
            .context("No subscription preview; review sources again")?;
        ensure!(
            pending.revision == native::revision(&self.store),
            "Draft changed; review subscriptions again before saving"
        );
        let mut s = self.store.clone();
        let mut count = 0;
        for p in &pending.items {
            s.nodes.retain(|n| n.provider != p.sub.id);
            s.nodes.extend(p.nodes.clone());
            s.subscriptions.retain(|x| x.id != p.sub.id);
            s.subscriptions.push(p.sub.clone());
            count += p.nodes.len();
        }
        if !s.nodes.iter().any(|n| Some(&n.id) == s.selected.as_ref()) {
            s.selected = s.nodes.first().map(|n| n.id.clone());
        }
        self.save(s)?;
        self.pending = None;
        self.log(format!(
            "Saved {count} subscription nodes; running configuration unchanged"
        ));
        Ok(Reply::success("Subscriptions saved to draft; not applied"))
    }
    async fn refresh_all(&mut self) -> Result<Reply> {
        self.pending = None;
        let subs = self.store.subscriptions.clone();
        ensure!(!subs.is_empty(), "Import a subscription first");
        let mut items = vec![];
        let mut preview = ImportPreview {
            id: model::token()?,
            revision: native::revision(&self.store),
            sources: vec![],
            name: format!("{} subscriptions", subs.len()),
            format: "batch update".into(),
            count: 0,
            added: 0,
            removed: 0,
            warnings: vec![],
            names: vec![],
        };
        for sub in subs {
            let r = self
                .prepare(sub.source, sub.name.clone(), sub.user_agent, Some(sub.id))
                .await
                .with_context(|| {
                    format!(
                        "Update failed for {}; no subscriptions saved",
                        model::clean(&sub.name)
                    )
                })?;
            let p = r.preview.context("Missing subscription preview")?;
            preview.sources.push(format!(
                "{}: {} nodes · +{} / -{}",
                p.name, p.count, p.added, p.removed
            ));
            preview.count += p.count;
            preview.added += p.added;
            preview.removed += p.removed;
            preview
                .warnings
                .extend(p.warnings.into_iter().map(|w| format!("{}: {w}", p.name)));
            items.extend(
                self.pending
                    .take()
                    .context("Missing staged subscription")?
                    .items,
            );
        }
        self.pending = Some(PendingSubscriptions {
            items,
            id: preview.id.clone(),
            revision: preview.revision.clone(),
        });
        let mut r = Reply::success("Review all updates; one save, no automatic Apply");
        r.preview = Some(preview);
        Ok(r)
    }
    async fn prepare_rules(
        &mut self,
        source: String,
        name: String,
        format: String,
        target: String,
        existing: Option<String>,
    ) -> Result<Reply> {
        self.pending_rules = None;
        let is_srs = format == "auto"
            && url::Url::parse(&source)
                .map(|u| u.path().ends_with(".srs"))
                .unwrap_or_else(|_| source.ends_with(".srs"));
        if ["native-source", "native-srs"].contains(&format.as_str()) || is_srs {
            ensure!(
                target.is_empty() && existing.is_none(),
                "Use Import Rule Set for native references; the core owns updates"
            );
            let format = if is_srs {
                "native-srs".to_string()
            } else {
                format
            };
            let value = native::resource::reference(&source, &format)?;
            let revision = native::revision(&self.store);
            let id = model::token()?;
            let name = if name.trim().is_empty() {
                "Native Rule Set".into()
            } else {
                model::clean(&name)
            };
            let mut reply =
                Reply::success("Native reference preview; no download or conversion performed");
            reply.rules_preview = Some(RulesPreview {
                draft_id: id.clone(),
                revision: revision.clone(),
                name: name.clone(),
                format: format.clone(),
                input_count: 0,
                count: 0,
                added: 0,
                removed: 0,
                target: String::new(),
                warnings: vec![],
                policies: vec![],
                sample: vec![
                    format!(
                        "Native {} / {} resource",
                        value["type"].as_str().unwrap(),
                        value["format"].as_str().unwrap()
                    ),
                    "Contents are loaded by sing-box on Apply / Start; rule count is unknown."
                        .into(),
                    "Remote refresh, cache and HTTP settings belong to this native object.".into(),
                ],
            });
            self.pending_rules = Some(PendingRules {
                native_set: Some(value),
                resource: RuleResource {
                    id: String::new(),
                    name,
                    source,
                    format,
                    updated_at: 0,
                    digest: String::new(),
                    input_count: 0,
                    rules: vec![],
                    native_document: None,
                    warnings: vec![],
                },
                binding: RuleBinding {
                    id: String::new(),
                    resource: String::new(),
                    target: String::new(),
                    enabled: true,
                },
                revision,
                id,
                unbound: true,
            });
            return Ok(reply);
        }
        ensure!(
            !source.trim().is_empty(),
            "Paste a rule subscription URL / local file / rule text"
        );
        ensure!(
            (target.is_empty() && self.store.native.is_some())
                || (existing.is_some() && self.store.native.is_some())
                || config::target_exists(&self.store, &target, true),
            "Choose an existing target group"
        );
        let text = match subscription::fetch(&source, "sing/0.3 rules", None).await {
            Ok(s) => s,
            Err(first) if self.connected() => subscription::fetch(
                &source,
                "sing/0.3 rules",
                self.running.as_ref().map(|s| s.settings.port),
            )
            .await
            .map_err(|_| first)?,
            Err(e) => return Err(e),
        };
        let native_document = if self.store.native.is_some() {
            ruleset::native_document(&text, &format)?
        } else {
            None
        };
        let parsed = if let Some(doc) = &native_document {
            ruleset::Parsed {
                rules: vec![],
                warnings: vec![],
                policies: vec![],
                input_count: doc["rules"].as_array().unwrap().len(),
                format: "native".into(),
            }
        } else {
            ruleset::parse(&text, &format)?
        };
        let existing_native_refresh = existing.is_some() && self.store.native.is_some();
        let id = existing.unwrap_or_else(|| model::id(&format!("rules:{source}")));
        let old = self.store.rule_resources.iter().find(|r| r.id == id);
        let name = if name.trim().is_empty() {
            format!("Rules {}", self.store.rule_resources.len() + 1)
        } else {
            model::clean(&name)
        };
        let resource = RuleResource {
            id: id.clone(),
            name: name.clone(),
            source,
            format: parsed.format.clone(),
            updated_at: model::now(),
            digest: model::id(&text),
            input_count: parsed.input_count,
            rules: parsed.rules.clone(),
            native_document,
            warnings: parsed.warnings.clone(),
        };
        // Count source entries, not the consolidated OR buckets emitted by the
        // external converter. Native entries retain their complete JSON shape.
        let entries = |r: &RuleResource| -> Vec<String> {
            if r.native_document.is_some() {
                r.native_rules()
                    .iter()
                    .map(serde_json::Value::to_string)
                    .collect()
            } else {
                r.rules
                    .iter()
                    .map(|v| format!("{}  {}", v.kind, v.value))
                    .collect()
            }
        };
        let new_entries = entries(&resource);
        let old_rules: std::collections::HashSet<_> =
            old.map(entries).unwrap_or_default().into_iter().collect();
        let new_rules: std::collections::HashSet<_> = new_entries.iter().cloned().collect();
        let added = new_rules.difference(&old_rules).count();
        let removed = old_rules.difference(&new_rules).count();
        let binding = self
            .store
            .rule_bindings
            .iter()
            .find(|b| b.resource == id)
            .cloned()
            .unwrap_or(RuleBinding {
                id: model::id(&format!("binding:{id}")),
                resource: id,
                target: target.clone(),
                enabled: true,
            });
        let mut reply = Reply::success("Review conversion and target before saving; unsupported rules are excluded only after confirmation");
        let revision = native::revision(&self.store);
        let draft_id = model::token()?;
        reply.rules_preview = Some(RulesPreview {
            draft_id: draft_id.clone(),
            revision: revision.clone(),
            name,
            format: parsed.format,
            input_count: parsed.input_count,
            count: new_entries.len(),
            added,
            removed,
            target: if existing_native_refresh {
                "Existing native routing is preserved".into()
            } else {
                binding.target.clone()
            },
            warnings: parsed.warnings,
            policies: parsed.policies,
            sample: new_entries.into_iter().take(8).collect(),
        });
        self.pending_rules = Some(PendingRules {
            native_set: None,
            resource,
            binding,
            revision,
            id: draft_id,
            unbound: target.is_empty(),
        });
        Ok(reply)
    }
    async fn check_store(&self, store: &Store) -> Result<PathBuf> {
        let core = find_core(&self.dir, &store.settings.core)
            .context("Core missing. Press i on Overview to install sing-box.")?;
        let version = core_version(&core).await?;
        ensure!(
            supported_version(&version),
            "sing-box 1.14+ required; found {version}"
        );
        let candidate = self.dir.join("candidate.json");
        model::atomic_write(
            &candidate,
            &serde_json::to_vec_pretty(&config::generate(store)?)?,
        )?;
        let output = tokio::time::timeout(
            Duration::from_secs(15),
            tokio::process::Command::new(&core)
                .arg("check")
                .arg("-c")
                .arg(&candidate)
                .arg("-D")
                .arg(&self.dir)
                .kill_on_drop(true)
                .output(),
        )
        .await
        .context("Config check timed out")??;
        if !output.status.success() {
            bail!(
                "Config check failed: {}",
                redact_error(&String::from_utf8_lossy(&output.stderr), store)
            );
        }
        Ok(core)
    }
    fn stop(&mut self) -> Result<()> {
        if self.dir.join(proxy_helper::MARKER).exists() {
            let restored = proxy_helper::restore(&self.dir)?;
            self.log(restored.detail);
        }
        self.lease.set(false);
        if self.tun {
            helper_request(&self.dir, "stop")?;
            self.tun = false;
        }
        if let Some(mut c) = self.child.take() {
            let _ = unsafe { libc::kill(c.id() as i32, libc::SIGTERM) };
            for _ in 0..30 {
                if c.try_wait()?.is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            if c.try_wait()?.is_none() {
                c.kill()?;
                c.wait()?;
            }
        }
        self.api_ready = false;
        self.connectivity = ProbeStatus::default();
        Ok(())
    }
    async fn start(&mut self, core: &Path, store: &Store) -> Result<()> {
        self.selection_recovery.clear();
        self.last_sample = 0;
        self.status = api::Status::default();
        self.connectivity = ProbeStatus::default();
        let log_offset = fs::metadata(self.dir.join("core.log")).map_or(0, |m| m.len());
        for port in if store.native.is_some() {
            vec![store.settings.api_port]
        } else {
            vec![store.settings.port, store.settings.api_port]
        } {
            std::net::TcpListener::bind(("127.0.0.1", port))
                .with_context(|| format!("Port {port} is already in use. Change it in Config."))?;
        }
        model::atomic_write(
            &self.dir.join("runtime.json"),
            &serde_json::to_vec_pretty(&config::generate(store)?)?,
        )?;
        if native::uses_tun(store) {
            ensure!(
                helper_request(&self.dir, "start")? == "running",
                "TUN helper failed to start"
            );
            self.tun = true;
        } else {
            self.child = Some(spawn_core(core, &self.dir)?);
        }
        self.running = Some(store.clone());
        for _ in 0..40 {
            if !self.connected() {
                bail!(
                    "{}: {}",
                    "Core stopped during startup",
                    startup_error(&self.dir, log_offset, store)
                );
            }
            if let Ok(mut api) = self.api().await {
                if let Ok(version) = api.version().await {
                    self.version = version.version;
                    self.api_ready = true;
                    self.log("Core ready · native gRPC connected");
                    self.started_at = model::now();
                    self.restore_selections(store).await;
                    if store.settings.mode == "system" {
                        self.lease.set(true);
                        match proxy_helper::enable(&self.dir, store.settings.port) {
                            Ok(s) => self.log(s.detail),
                            Err(e) => return Err(e),
                        }
                    }
                    return Ok(());
                }
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
        bail!("Core started but its native gRPC API did not become ready")
    }
    async fn connect(&mut self) -> Result<Reply> {
        ensure!(self.store.settings.mode!="system"||std::env::var_os("SSH_CONNECTION").is_none(),"System proxy takeover is disabled over SSH. Use Port mode; this controls the remote host.");
        let core = self.check_store(&self.store).await?;
        if self.store.settings.mode == "system"
            && proxy_helper::query(&self.dir, proxy_helper::Request::Status).is_err()
        {
            let mut r = Reply::success(
                "Authorize macOS system proxy management; no system settings changed yet",
            );
            r.needs_auth = true;
            r.auth_kind = "system".into();
            r.after_auth = Some(Action::Connect);
            return Ok(r);
        }
        if native::uses_tun(&self.store) && helper_request(&self.dir, "status").is_err() {
            let mut r = Reply::success("TUN requires administrator authorization on this host");
            r.needs_auth = true;
            return Ok(r);
        }
        let old = self.running.clone().filter(|_| self.connected());
        self.stop()?;
        if let Some(old) = &old {
            model::atomic_write(
                &self.dir.join("previous-state.json"),
                &serde_json::to_vec(old)?,
            )?;
        }
        let new = self.store.clone();
        match self.start(&core, &new).await {
            Ok(()) => Ok(Reply::success(if native::uses_tun(&new) {
                "TUN core ready. Internet access is not yet checked."
            } else {
                match new.settings.mode.as_str() {
                "system" => {
                    "Core ready; system proxy configured. Internet access is not yet checked (v)."
                }
                "tun" => "TUN core ready. Internet access is not yet checked (v).",
                _ => "Proxy port ready; applications are NOT automatically routed through it.",
            }
            })),
            Err(error) => {
                self.stop().context(
                    "Apply failed, but proxy restoration is still pending; keeping the core alive",
                )?;
                let recovery = if let Some(old) = old {
                    let old_core = find_core(&self.dir, &old.settings.core).unwrap_or(core);
                    match self.start(&old_core, &old).await {
                        Ok(()) => {
                            self.log("Apply failed; restored the previously running configuration");
                            "Previous configuration restarted and native API is ready. Internet access remains unchecked.".to_string()
                        }
                        Err(e) => {
                            self.log(format!("Rollback also failed: {e}"));
                            format!("Rollback failed: {e}. Inspect Activity before retrying.")
                        }
                    }
                } else {
                    "Core stopped; there was no previous running configuration to restore.".into()
                };
                bail!("{error:#}\nRecovery: {recovery}")
            }
        }
    }
    async fn handle(&mut self, action: Action) -> Result<Reply> {
        if self.store.native.is_some() {
            ensure!(
                !matches!(
                    action,
                    Action::SaveGroup(_)
                        | Action::DeleteGroup(_)
                        | Action::SelectGroup { .. }
                        | Action::Select(_)
                        | Action::SaveBinding(_)
                        | Action::DeleteBinding(_)
                        | Action::MoveBinding { .. }
                ),
                "Use native Outbounds / Routing editors after migration"
            );
        }
        match action {
            Action::ReprepareSubscriptions(id) => {
                let pending = self
                    .pending
                    .as_ref()
                    .context("Preview expired; use Update or Import again")?;
                ensure!(
                    pending.id == id,
                    "Preview was replaced; use Update or Import again"
                );
                let sources = pending
                    .items
                    .iter()
                    .map(|p| p.sub.clone())
                    .collect::<Vec<_>>();
                let original = pending.clone();
                let mut items = vec![];
                let mut combined: Option<ImportPreview> = None;
                for source in sources {
                    let reply = match self
                        .prepare(
                            source.source,
                            source.name,
                            source.user_agent,
                            Some(source.id),
                        )
                        .await
                    {
                        Ok(reply) => reply,
                        Err(error) => {
                            self.pending = Some(original);
                            return Err(error);
                        }
                    };
                    let staged = self.pending.take().context("Source preview missing")?;
                    items.extend(staged.items);
                    let p = reply.preview.context("Source preview missing")?;
                    if let Some(all) = &mut combined {
                        all.count += p.count;
                        all.added += p.added;
                        all.removed += p.removed;
                        all.names.extend(p.names);
                        all.warnings.extend(p.warnings);
                        all.sources.extend(p.sources);
                    } else {
                        combined = Some(p);
                    }
                }
                let mut preview = combined.context("Preview has no sources")?;
                if items.len() > 1 {
                    preview.name = format!("{} subscriptions", items.len());
                }
                preview.id = model::token()?;
                preview.revision = native::revision(&self.store);
                self.pending = Some(PendingSubscriptions {
                    items,
                    id: preview.id.clone(),
                    revision: preview.revision.clone(),
                });
                let mut reply = Reply::success("Preview refreshed; review changes before saving");
                reply.preview = Some(preview);
                Ok(reply)
            }
            Action::ConnectionSetupInfo => {
                let mut r = Reply::success("Choose target and capture; no changes yet");
                r.edit = Some(native::Edit {
                    revision: native::revision(&self.store),
                    pointer: String::new(),
                    value: config::redacted(&native::migration(&self.store)?),
                });
                Ok(r)
            }
            Action::ReviewConnectionSetup(change) => {
                let s = native::connection_setup(
                    &self.store,
                    &change,
                    std::env::var_os("SSH_CONNECTION").is_some(),
                    cfg!(target_os = "macos"),
                )?;
                let mut r = Reply::success("Review connection setup; nothing saved or started yet");
                r.config = Some(native::connection_setup_review(&self.store, &s)?);
                r.confirm = Some(Action::SaveConnectionSetup(change));
                Ok(r)
            }
            Action::SaveConnectionSetup(change) => {
                let s = native::connection_setup(
                    &self.store,
                    &change,
                    std::env::var_os("SSH_CONNECTION").is_some(),
                    cfg!(target_os = "macos"),
                )?;
                let review = native::review(&s, self.running.as_ref())?;
                let diff = native::review::detailed(
                    &self
                        .running
                        .as_ref()
                        .map(config::generate)
                        .transpose()?
                        .unwrap_or(serde_json::json!({})),
                    &config::generate(&s)?,
                );
                if self.store.native.is_none() && !self.dir.join("pre-native-state.json").exists() {
                    model::atomic_write(
                        &self.dir.join("pre-native-state.json"),
                        &serde_json::to_vec_pretty(&self.store)?,
                    )?;
                }
                self.save(s)?;
                let mut r =
                    Reply::success("Setup saved to draft. Confirm Start / Apply only when ready.");
                r.diff = Some(diff);
                r.config = Some(review);
                r.confirm = Some(Action::ApplyNative {
                    revision: native::revision(&self.store),
                });
                Ok(r)
            }
            Action::ReviewMigration => {
                ensure!(
                    self.store.native.is_none(),
                    "Already using native configuration"
                );
                let doc = native::migration(&self.store)?;
                let mut r = Reply::success("Review native configuration upgrade");
                r.config = Some(format!("Create a private pre-native-state.json backup, then adopt this native draft. No core restart or network changes. Existing DNS and rules are materialized once. Future edits are independent.\n\n{}", serde_json::to_string_pretty(&config::redacted(&doc))?));
                r.confirm = Some(Action::AdoptNative {
                    revision: native::revision(&self.store),
                });
                Ok(r)
            }
            Action::AdoptNative { revision } => {
                ensure!(
                    self.store.native.is_none() && revision == native::revision(&self.store),
                    "State changed; review upgrade again"
                );
                let doc = native::migration(&self.store)?;
                let backup = self.dir.join("pre-native-state.json");
                if !backup.exists() {
                    model::atomic_write(&backup, &serde_json::to_vec_pretty(&self.store)?)?;
                }
                let mut s = self.store.clone();
                native::adopt(&mut s, doc)?;
                self.save(s)?;
                Ok(Reply::success(
                    "Native draft adopted. Original state backed up; running core unchanged.",
                ))
            }
            Action::ReadNative(pointer) => {
                let mut r = Reply::success("Edit native draft");
                r.edit = Some(native::read(&self.store, pointer)?);
                Ok(r)
            }
            Action::WriteNative(edit) => {
                let mut s = self.store.clone();
                native::write(&mut s, edit)?;
                self.save(s)?;
                Ok(Reply::success("Draft saved. Review & Apply when ready."))
            }
            Action::WriteGroup(change) => {
                let mut store = self.store.clone();
                native::write_group(&mut store, change)?;
                self.save(store)?;
                Ok(Reply::success(
                    "Group saved to draft. Review Changes to apply.",
                ))
            }
            Action::ReviewApply => {
                let mut r = Reply::success("Review & Apply");
                let connected = self.connected();
                r.diff = Some(native::review::detailed(
                    &self
                        .running
                        .as_ref()
                        .filter(|_| connected)
                        .map(config::generate)
                        .transpose()?
                        .unwrap_or(serde_json::json!({})),
                    &config::generate(&self.store)?,
                ));
                r.config = Some(native::review(
                    &self.store,
                    self.running.as_ref().filter(|_| connected),
                )?);
                let memory = selection::load(&self.dir)?;
                r.config
                    .as_mut()
                    .unwrap()
                    .push_str(&format!("\n\n{}", selection::summary(&self.store, &memory)));
                r.confirm = Some(Action::ApplyNative {
                    revision: native::revision(&self.store),
                });
                Ok(r)
            }
            Action::ApplyNative { revision } => {
                ensure!(
                    revision == native::revision(&self.store),
                    "Draft changed; review again before applying"
                );
                self.connect().await
            }
            Action::SelectNative { group, member } => self.select_native(group, member).await,
            Action::Connections => {
                let mut items = if self.connected() {
                    self.api().await?.connections().await?
                } else {
                    vec![]
                };
                let total = items.len();
                items.sort_by(|a, b| {
                    b.created_at
                        .cmp(&a.created_at)
                        .then_with(|| a.id.cmp(&b.id))
                });
                items.truncate(500);
                for c in &mut items {
                    for s in [
                        &mut c.id,
                        &mut c.inbound,
                        &mut c.inbound_type,
                        &mut c.network,
                        &mut c.source,
                        &mut c.destination,
                        &mut c.domain,
                        &mut c.protocol,
                        &mut c.rule,
                        &mut c.outbound,
                        &mut c.outbound_type,
                    ] {
                        *s = model::clean(s);
                    }
                    for tag in &mut c.chain {
                        *tag = model::clean(tag);
                    }
                    if let Some(p) = &mut c.process {
                        p.path = model::clean(&p.path);
                    }
                }
                let mut reply = Reply::success(
                    "Connections refreshed · sampled live state, not complete history",
                );
                reply.connections = Some(ConnectionReport {
                    observed_at: model::now(),
                    total,
                    items,
                });
                Ok(reply)
            }
            Action::CloseConnection(id) => {
                ensure!(
                    id.len() == 36 && id.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-'),
                    "Invalid connection identifier"
                );
                ensure!(self.connected(), "Core is not connected");
                self.api().await?.close_connection(id).await?;
                Ok(Reply::success("Close requested for this connection only. Its application may reconnect automatically."))
            }
            Action::Diagnostics => {
                let connected = self.connected();
                let mut reply =
                    Reply::success("Local configuration diagnostics · no probe request sent");
                reply.config = Some(config::diagnostics(
                    &self.store,
                    self.running.as_ref().filter(|_| connected),
                ));
                Ok(reply)
            }
            Action::SaveGroup(mut group) => {
                group.name = model::clean(group.name.trim());
                config::validate_group(&self.store, &group)?;
                let mut store = self.store.clone();
                if let Some(old) = store.proxy_groups.iter_mut().find(|g| g.id == group.id) {
                    *old = group;
                } else {
                    store.proxy_groups.push(group);
                }
                self.save(store)?;
                Ok(Reply::success("Group saved. Press c to apply; automatic groups test gstatic every 3 minutes while active."))
            }
            Action::DeleteGroup(id) => {
                let mut store = self.store.clone();
                ensure!(
                    store.proxy_groups.iter().any(|g| g.id == id),
                    "Group not found"
                );
                store.proxy_groups.retain(|g| g.id != id);
                config::validate_references(&store)
                    .context("Group is still referenced: change its rules / default route first")?;
                self.save(store)?;
                Ok(Reply::success(
                    "Group removed from saved configuration; c applies changes",
                ))
            }
            Action::SelectGroup { group, node } => {
                let mut store = self.store.clone();
                let g = store
                    .proxy_groups
                    .iter_mut()
                    .find(|g| g.id == group)
                    .context("Group not found")?;
                ensure!(
                    g.kind == "selector",
                    "Automatic groups select their own node"
                );
                ensure!(
                    g.members.contains(&node) && store.nodes.iter().any(|n| n.id == node),
                    "Unavailable group member"
                );
                g.selected = Some(node.clone());
                let active = self.connected()
                    && self.running.as_ref().is_some_and(|s| {
                        s.settings.route_mode == "rule"
                            || (s.settings.route_mode == "global"
                                && s.settings.global_target == g.tag())
                    });
                if active {
                    ensure!(
                        self.running.as_ref().is_some_and(|s| s
                            .proxy_groups
                            .iter()
                            .any(|g| g.id == group
                                && g.kind == "selector"
                                && g.members.contains(&node))),
                        "Group/member not loaded yet; press c to apply first"
                    );
                    self.api()
                        .await?
                        .select_group(g.tag(), format!("n-{node}"))
                        .await?;
                    if let Some(r) = &mut self.running {
                        r.proxy_groups
                            .iter_mut()
                            .find(|g| g.id == group)
                            .unwrap()
                            .selected = Some(node);
                    }
                }
                self.save(store)?;
                self.connectivity = ProbeStatus::default();
                Ok(Reply::success(if active {
                    "Group node selected; existing connections may keep their previous route"
                } else {
                    "Selection saved; this group is not active in the current routing mode. c applies saved settings."
                }))
            }
            Action::ImportRules {
                source,
                name,
                format,
                target,
            } => {
                ensure!(!target.is_empty(), "Choose an existing target group");
                self.prepare_rules(source, name, format, target, None).await
            }
            Action::PrepareRuleDraft {
                source,
                name,
                format,
                revision,
            } => {
                ensure!(
                    revision == native::revision(&self.store),
                    "Draft changed. Reopen rule import before reviewing."
                );
                self.prepare_rules(source, name, format, String::new(), None)
                    .await
            }
            Action::CommitRuleDraft {
                id,
                revision,
                target,
                position,
                group,
            } => {
                let pending = self
                    .pending_rules
                    .as_ref()
                    .context("No rule import pending. Review the source again.")?;
                ensure!(
                    pending.unbound && pending.id == id && pending.revision == revision,
                    "Rule import preview was replaced. Review the source again."
                );
                ensure!(
                    revision == native::revision(&self.store),
                    "Draft changed. Reopen rule import before saving."
                );
                let mut store = self.store.clone();
                if let Some(group) = group {
                    ensure!(
                        native::tag(&group.value) == target,
                        "The new group must be the rule target"
                    );
                    native::write_group(&mut store, group)?;
                }
                if let Some(value) = &pending.native_set {
                    native::resource::bind(
                        &mut store,
                        value.clone(),
                        &pending.resource.name,
                        &target,
                        position,
                    )?;
                } else {
                    native::bind_rule_resource(&mut store, &pending.resource, &target, position)?;
                }
                self.save(store)?;
                self.pending_rules = None;
                Ok(Reply::success("Rule set and routing target saved to draft. DNS unchanged. Review Changes to apply."))
            }
            Action::RefreshRules(id) => {
                let r = self
                    .store
                    .rule_resources
                    .iter()
                    .find(|r| r.id == id)
                    .context("Rule resource not found")?
                    .clone();
                let target = self
                    .store
                    .rule_bindings
                    .iter()
                    .find(|b| b.resource == id)
                    .context("Binding missing")?
                    .target
                    .clone();
                self.prepare_rules(r.source, r.name, r.format, target, Some(id))
                    .await
            }
            Action::CommitRules => {
                let pending = self
                    .pending_rules
                    .as_ref()
                    .context("No rules pending review")?;
                ensure!(
                    !pending.unbound,
                    "Choose a target and insertion position before saving"
                );
                ensure!(
                    pending.revision == native::revision(&self.store),
                    "Draft changed. Review rule update again."
                );
                let resource = pending.resource.clone();
                let binding = pending.binding.clone();
                let mut store = self.store.clone();
                if let Some(old) = store
                    .rule_resources
                    .iter_mut()
                    .find(|r| r.id == resource.id)
                {
                    *old = resource;
                } else {
                    store.rule_resources.push(resource);
                }
                if !store.rule_bindings.iter().any(|b| b.id == binding.id) {
                    store.rule_bindings.push(binding);
                }
                if store.native.is_none() {
                    config::validate_references(&store)?;
                }
                self.save(store)?;
                self.pending_rules = None;
                Ok(Reply::success(
                    "Rules saved locally; source policies are not executed. Press c to apply.",
                ))
            }
            Action::CancelRuleDraft(id) => {
                if self
                    .pending_rules
                    .as_ref()
                    .is_some_and(|p| p.unbound && p.id == id)
                {
                    self.pending_rules = None;
                }
                Ok(Reply::success(
                    "Rule import discarded; saved configuration unchanged",
                ))
            }
            Action::CancelRules => {
                self.pending_rules = None;
                Ok(Reply::success("Rule import cancelled; old rules kept"))
            }
            Action::SaveBinding(binding) => {
                let mut store = self.store.clone();
                let old = store
                    .rule_bindings
                    .iter_mut()
                    .find(|b| b.id == binding.id)
                    .context("Binding not found")?;
                ensure!(
                    old.resource == binding.resource,
                    "Cannot replace a binding's resource"
                );
                *old = binding;
                config::validate_references(&store)?;
                self.save(store)?;
                Ok(Reply::success(
                    "Binding saved; c applies routing and paired domain DNS changes",
                ))
            }
            Action::DeleteBinding(id) => {
                let mut store = self.store.clone();
                let resource = store
                    .rule_bindings
                    .iter()
                    .find(|b| b.id == id)
                    .context("Binding not found")?
                    .resource
                    .clone();
                store.rule_bindings.retain(|b| b.id != id);
                if !store.rule_bindings.iter().any(|b| b.resource == resource) {
                    store.rule_resources.retain(|r| r.id != resource);
                }
                self.save(store)?;
                Ok(Reply::success(
                    "Rule subscription removed locally; c applies. Provider was not changed.",
                ))
            }
            Action::MoveBinding { id, delta } => {
                ensure!([-1, 1].contains(&delta), "Move one position at a time");
                let mut store = self.store.clone();
                let i = store
                    .rule_bindings
                    .iter()
                    .position(|b| b.id == id)
                    .context("Binding missing")?;
                let j = i
                    .saturating_add_signed(delta as isize)
                    .min(store.rule_bindings.len() - 1);
                store.rule_bindings.swap(i, j);
                self.save(store)?;
                Ok(Reply::success(
                    "Rule order saved; first matching terminal route wins. c applies.",
                ))
            }
            Action::RestoreProxy => {
                ensure!(
                    cfg!(target_os = "macos"),
                    "System proxy recovery is macOS-only"
                );
                if !self.dir.join(proxy_helper::MARKER).exists() {
                    return Ok(Reply::success(
                        "No system proxy restoration is pending for this instance",
                    ));
                }
                if proxy_helper::query(&self.dir, proxy_helper::Request::Status).is_err() {
                    let mut r =
                        Reply::success("Authorize recovery of original macOS proxy settings");
                    r.needs_auth = true;
                    r.auth_kind = "system".into();
                    r.after_auth = Some(Action::RestoreProxy);
                    return Ok(r);
                }
                let s = proxy_helper::restore(&self.dir)?;
                self.lease.set(false);
                self.log(s.detail.clone());
                Ok(Reply::success(if s.detail.is_empty() {
                    "No proxy restoration is pending".into()
                } else {
                    s.detail
                }))
            }
            Action::Probe => {
                ensure!(
                    self.connected(),
                    "Start the core before checking Internet access"
                );
                let port = self
                    .running
                    .as_ref()
                    .context("No running config")?
                    .settings
                    .port;
                self.connectivity = probe(port, "https://www.gstatic.com/generate_204").await;
                let mut r = Reply::success(self.connectivity.detail.clone());
                r.ok = self.connectivity.state == "passed";
                Ok(r)
            }
            Action::Logs => {
                use std::io::{Seek, SeekFrom};
                let mut raw = String::new();
                if let Ok(mut api) = self.api().await {
                    if let Ok(log) = api.logs().await {
                        raw = log
                            .messages
                            .iter()
                            .map(|m| m.message.as_str())
                            .collect::<Vec<_>>()
                            .join("\n");
                    }
                }
                if raw.is_empty() {
                    if let Ok(mut file) = fs::File::open(self.dir.join("core.log")) {
                        let len = file.metadata()?.len();
                        file.seek(SeekFrom::Start(len.saturating_sub(65536)))?;
                        let mut data = vec![];
                        file.take(65536).read_to_end(&mut data)?;
                        raw = String::from_utf8_lossy(&data).into_owned();
                    }
                }
                let text = raw
                    .lines()
                    .map(|l| redact_error(l, &self.store))
                    .collect::<Vec<_>>()
                    .join("\n");
                let mut r = Reply::success("Core diagnostics · credentials redacted");
                r.config = Some(if text.is_empty() {
                    "No core warnings or errors recorded.".into()
                } else {
                    text
                });
                Ok(r)
            }
            Action::Shutdown => {
                self.stop()?;
                self.running = None;
                let _ = helper_request(&self.dir, "exit");
                let _ = proxy_helper::query(&self.dir, proxy_helper::Request::Exit);
                Ok(Reply::success("Disconnected; background manager stopped"))
            }
            Action::Snapshot => {
                let mut r = Reply::success("");
                r.snapshot = Some(self.snapshot().await);
                Ok(r)
            }
            Action::Import {
                source,
                name,
                user_agent,
            } => self.prepare(source, name, user_agent, None).await,
            Action::Refresh(id) => {
                let s = self
                    .store
                    .subscriptions
                    .iter()
                    .find(|s| s.id == id)
                    .context("Subscription not found")?
                    .clone();
                self.prepare(s.source, s.name, s.user_agent, Some(id)).await
            }
            Action::RefreshAll => self.refresh_all().await,
            Action::CommitSubscriptions { id, revision } => {
                let p = self
                    .pending
                    .as_ref()
                    .context("No subscription preview; review sources again")?;
                ensure!(
                    p.id == id && p.revision == revision,
                    "Subscription preview was replaced; review sources again"
                );
                self.commit_subscriptions()
            }
            Action::CancelSubscriptions(id) => {
                if self.pending.as_ref().is_some_and(|p| p.id == id) {
                    self.pending = None;
                }
                Ok(Reply::success(
                    "Subscription preview discarded; saved configuration unchanged",
                ))
            }
            Action::CancelImport => {
                self.pending = None;
                Ok(Reply::success("Import cancelled"))
            }
            Action::CommitImport => self.commit_subscriptions(),
            Action::Delete(id) => {
                let mut s = self.store.clone();
                s.subscriptions.retain(|x| x.id != id);
                s.nodes.retain(|n| n.provider != id);
                if !s.nodes.iter().any(|n| Some(&n.id) == s.selected.as_ref()) {
                    s.selected = s.nodes.first().map(|n| n.id.clone());
                }
                self.save(s)?;
                self.log(
                    "Subscription removed from saved settings; active core unchanged until Apply",
                );
                Ok(Reply::success("Subscription removed"))
            }
            Action::Favorite(id) => {
                let mut s = self.store.clone();
                let n = s
                    .nodes
                    .iter_mut()
                    .find(|n| n.id == id)
                    .context("Node not found")?;
                n.favorite = !n.favorite;
                self.save(s)?;
                Ok(Reply::success("Favorite updated"))
            }
            Action::Select(id) => {
                self.connectivity = ProbeStatus::default();
                let tag = self
                    .store
                    .nodes
                    .iter()
                    .find(|n| n.id == id)
                    .context("Node not found")?
                    .tag();
                let active = self.connected()
                    && self
                        .running
                        .as_ref()
                        .is_some_and(|s| s.settings.route_mode != "direct");
                if active {
                    ensure!(
                        self.running
                            .as_ref()
                            .is_some_and(|r| r.nodes.iter().any(|n| n.id == id)),
                        "New node is not loaded yet. Press c to apply the subscription first."
                    );
                    self.api().await?.select(tag).await?;
                    if let Some(r) = &mut self.running {
                        r.selected = Some(id.clone());
                    }
                }
                let mut s = self.store.clone();
                s.selected = Some(id);
                self.save(s)?;
                Ok(Reply::success(if active {
                    "Built-in proxy node selected; custom groups keep their own selections"
                } else {
                    "Node selection saved; not active until a proxy routing mode is applied"
                }))
            }
            Action::SaveSettings(settings) => {
                if self.store.native.is_none() {
                    config::validate(&settings)?;
                } else {
                    ensure!(
                        ["port", "system"].contains(&settings.mode.as_str()),
                        "System integration must be port (off) or system; edit TUN under Inbounds"
                    );
                    ensure!(
                        ["rule", "global", "direct"].contains(&settings.route_mode.as_str()),
                        "Invalid routing override"
                    );
                    ensure!(
                        settings.api_port >= 1024
                            && settings.port >= 1024
                            && settings.api_port != settings.port,
                        "Proxy and API ports must be different and at least 1024"
                    );
                }
                if settings.core != self.store.settings.core && !self.connected() {
                    self.version.clear();
                }
                let mut s = self.store.clone();
                if settings.api_port != s.settings.api_port {
                    if let Some(doc) = s.native.as_mut() {
                        let api = doc["services"]
                            .as_array_mut()
                            .context("Missing services")?
                            .iter_mut()
                            .find(|v| native::tag(v) == "management")
                            .context("Missing management service")?;
                        api["listen_port"] = serde_json::json!(settings.api_port);
                    }
                }
                s.settings = settings;
                if s.native.is_none() {
                    config::validate_references(&s)?;
                }
                self.save(s)?;
                Ok(Reply::success(
                    "Settings saved. Review & Apply to activate changes.",
                ))
            }
            Action::Preview => {
                let mut r = Reply::success("Generated config · credentials hidden");
                r.config = Some(serde_json::to_string_pretty(&config::redacted(
                    &config::generate(&self.store)?,
                ))?);
                Ok(r)
            }
            Action::Check => {
                self.check_store(&self.store).await?;
                self.log("sing-box check passed");
                Ok(Reply::success(
                    "Config valid · sing-box check passed (not a connectivity test)",
                ))
            }
            Action::Connect => self.connect().await,
            Action::SetMode(mode) => self.set_mode(mode).await,
            Action::SetGlobalTarget(target) => self.set_global_target(target).await,
            Action::SetSystemProxy(on) => self.set_system_proxy(on),
            Action::SetTun { enabled, revision } => self.set_tun(enabled, revision),
            Action::CoreReport { releases } => self.core_report(releases).await,
            Action::InstallCoreVersion(version) => self.install_core_version(version).await,
            Action::SelectCore(path) => self.select_core(path).await,
            Action::Disconnect => {
                self.stop()?;
                self.running = None;
                self.log("Disconnected by user");
                Ok(Reply::success("Disconnected"))
            }
            Action::Test(id) => {
                if self.store.native.is_some() {
                    let running = self
                        .running
                        .as_ref()
                        .context("Start the core before testing")?;
                    let doc = running
                        .native
                        .as_ref()
                        .context("Apply the native draft first")?;
                    ensure!(
                        native::array(doc, "/outbounds")
                            .iter()
                            .any(|v| native::tag(v) == id),
                        "Apply this outbound first"
                    );
                    self.api().await?.test(id).await?;
                    return Ok(Reply::success(
                        "Latency test requested; results appear beside outbounds",
                    ));
                }
                ensure!(self.running.as_ref().is_none_or(|s| s.settings.route_mode != "direct"),
                    "Direct mode does not load proxy nodes. Apply rule/global mode before testing nodes.");
                let tag = if id.is_empty() {
                    "proxy".into()
                } else {
                    self.store
                        .nodes
                        .iter()
                        .find(|n| n.id == id)
                        .context("Node not found")?
                        .tag()
                };
                self.api().await?.test(tag).await?;
                Ok(Reply::success(
                    "Latency test requested; results update on Nodes",
                ))
            }
            Action::InstallCore => {
                ensure!(!self.connected(), "Disconnect before installing a core");
                let path = install_core(&self.dir).await?;
                let mut s = self.store.clone();
                s.settings.core = path.display().to_string();
                self.save(s)?;
                self.version = core_version(&path).await?;
                self.log(format!(
                    "Installed official sing-box {CORE_VERSION}; SHA-256 verified"
                ));
                Ok(Reply::success(
                    "Core installed. Import a subscription, then press c.",
                ))
            }
            Action::Rollback => {
                let previous: Store = serde_json::from_slice(
                    &fs::read(self.dir.join("previous-state.json"))
                        .context("No previous applied configuration")?,
                )?;
                self.check_store(&previous).await?;
                let mut s = self.store.clone();
                s.nodes = previous.nodes;
                s.subscriptions = previous.subscriptions;
                s.selected = previous.selected;
                s.settings = previous.settings;
                s.proxy_groups = previous.proxy_groups;
                s.rule_resources = previous.rule_resources;
                s.rule_bindings = previous.rule_bindings;
                s.native = previous.native;
                s.schema = previous.schema;
                self.save(s)?;
                self.connect().await
            }
        }
    }
}

pub fn daemon(dir: &Path) -> Result<()> {
    model::private_dir(dir)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(dir.join("manager.lock"))?;
    lock.try_lock_exclusive()
        .context("Manager already running")?;
    let socket = dir.join("manager.sock");
    if socket.exists() {
        fs::remove_file(&socket)?;
    }
    let listener = UnixListener::bind(&socket)?;
    listener.set_nonblocking(true)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
    let store = Store::load(dir)?;
    store.save(dir)?;
    let mut manager = Manager {
        pending_rules: None,
        lease: proxy_helper::Lease::start(dir),
        connectivity: ProbeStatus::default(),
        dir: dir.into(),
        store,
        child: None,
        tun: false,
        running: None,
        pending: None,
        activity: vec![],
        status: api::Status::default(),
        groups: api::Groups::default(),
        api_ready: false,
        version: String::new(),
        last_sample: 0,
        started_at: 0,
        selection_recovery: String::new(),
    };
    manager.log("Manager ready · q closes only the interface");
    let rt = tokio::runtime::Runtime::new()?;
    static STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    extern "C" fn stop_signal(_: libc::c_int) {
        STOP.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    unsafe {
        libc::signal(
            libc::SIGTERM,
            stop_signal as *const () as libc::sighandler_t,
        );
        libc::signal(libc::SIGINT, stop_signal as *const () as libc::sighandler_t);
    }
    let mut health_tick = std::time::Instant::now();
    let mut last_recovery_error = String::new();
    while !STOP.load(std::sync::atomic::Ordering::Relaxed) {
        if health_tick.elapsed() > Duration::from_secs(1) {
            health_tick = std::time::Instant::now();
            if !manager.connected() && dir.join(proxy_helper::MARKER).exists() {
                manager.lease.set(false);
                match proxy_helper::restore(dir) {
                    Ok(s) => {
                        manager.log(s.detail);
                        last_recovery_error.clear();
                    }
                    Err(e) => {
                        let error = e.to_string();
                        if error != last_recovery_error {
                            manager.log(format!("System proxy recovery required: {error}"));
                            last_recovery_error = error;
                        }
                    }
                }
            }
        }
        let mut stream = match listener.accept() {
            Ok((s, _)) => s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        // macOS inherits O_NONBLOCK from the listener. Framed reads below
        // must wait for the complete request, including across partial writes.
        if stream.set_nonblocking(false).is_err()
            || stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .is_err()
            || stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .is_err()
        {
            continue;
        }
        let mut line = String::new();
        if BufReader::new(&stream)
            .take(10 * 1024 * 1024)
            .read_line(&mut line)
            .is_err()
            || line.is_empty()
        {
            continue;
        }
        let shutdown = matches!(serde_json::from_str::<Action>(&line), Ok(Action::Shutdown));
        let reply = match serde_json::from_str::<Action>(&line) {
            Ok(action) => {
                let mut private_sources: Vec<String> = match &action {
                    Action::Import {
                        source, user_agent, ..
                    } => vec![source.clone(), user_agent.clone()],
                    Action::PrepareRuleDraft { source, .. }
                    | Action::ImportRules { source, .. } => vec![source.clone()],
                    _ => vec![],
                };
                if let Some(pending) = &manager.pending {
                    private_sources.extend(pending.items.iter().map(|p| p.sub.source.clone()));
                }
                match rt.block_on(manager.handle(action)) {
                    Ok(r) => r,
                    Err(e) => {
                        let mut r = Reply::error(e);
                        r.message = redact_sources(
                            &redact_error(&r.message, &manager.store),
                            private_sources.iter().map(String::as_str),
                        );
                        manager.log(format!("Error: {}", r.message));
                        r
                    }
                }
            }
            Err(_) => Reply::error(anyhow::anyhow!("Invalid request")),
        };
        if let Ok(mut frame) = serde_json::to_vec(&reply) {
            frame.push(b'\n');
            let _ = stream.write_all(&frame);
        }
        if shutdown && reply.ok {
            break;
        }
    }
    manager.stop()?;
    let _ = fs::remove_file(socket);
    Ok(())
}

async fn probe(port: u16, url: &str) -> ProbeStatus {
    let result = async {
        let client = reqwest::Client::builder()
            .no_proxy()
            .proxy(reqwest::Proxy::all(format!("http://127.0.0.1:{port}"))?)
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let response = client
            .get(url)
            .send()
            .await
            .map_err(reqwest::Error::without_url)?;
        ensure!(
            response.status() == reqwest::StatusCode::NO_CONTENT,
            "Connectivity endpoint returned HTTP {} (expected 204)",
            response.status()
        );
        Ok::<_, anyhow::Error>(())
    }
    .await;
    ProbeStatus{state:if result.is_ok(){"passed"}else{"failed"}.into(),detail:match result{Ok(())=>"HTTPS probe passed through the local proxy. This does not prove every app / site is reachable.".into(),Err(e)=>format!("HTTPS probe failed: {}",model::clean(&e.to_string()))},checked_at:model::now()}
}

pub fn find_core(dir: &Path, custom: &str) -> Option<PathBuf> {
    if !custom.trim().is_empty() {
        let p = PathBuf::from(custom);
        return p.is_file().then_some(p);
    }
    let bundled = dir.join("bin/sing-box");
    if bundled.is_file() {
        return Some(bundled);
    }
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join("sing-box"))
            .find(|p| p.is_file())
    })
}
pub(crate) async fn core_version(core: &Path) -> Result<String> {
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new(core)
            .arg("version")
            .kill_on_drop(true)
            .output(),
    )
    .await??;
    ensure!(output.status.success(), "Cannot execute sing-box");
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string())
}
pub(crate) fn supported_version(version: &str) -> bool {
    version
        .split_whitespace()
        .last()
        .and_then(|s| {
            s.trim_start_matches('v')
                .split('.')
                .take(2)
                .map(str::parse::<u32>)
                .collect::<std::result::Result<Vec<_>, _>>()
                .ok()
        })
        .is_some_and(|v| v.len() == 2 && (v[0] > 1 || (v[0] == 1 && v[1] >= 14)))
}
fn spawn_core(core: &Path, dir: &Path) -> Result<Child> {
    let log = OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(dir.join("core.log"))?;
    Ok(Command::new(core)
        .arg("run")
        .arg("-c")
        .arg(dir.join("runtime.json"))
        .arg("-D")
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()?)
}
pub fn host() -> String {
    let mut name = [0u8; 256];
    unsafe {
        libc::gethostname(name.as_mut_ptr().cast(), name.len());
    }
    model::clean(&String::from_utf8_lossy(
        &name[..name.iter().position(|b| *b == 0).unwrap_or(name.len())],
    ))
}
fn source_label(s: &str) -> String {
    url::Url::parse(s)
        .ok()
        .and_then(|u| u.host_str().map(|h| format!("{}://{h}/••••", u.scheme())))
        .unwrap_or_else(|| "Local / pasted source (hidden)".into())
}
pub(crate) fn redact_error(error: &str, store: &Store) -> String {
    let mut e = redact_sources(
        error,
        store
            .subscriptions
            .iter()
            .map(|s| s.source.as_str())
            .chain(store.rule_resources.iter().map(|r| r.source.as_str())),
    );
    for n in &store.nodes {
        for key in ["password", "uuid"] {
            if let Some(s) = n.outbound[key].as_str() {
                if !s.is_empty() {
                    e = e.replace(s, "[redacted]");
                }
            }
        }
    }
    fn secrets(value: &serde_json::Value, masked: &serde_json::Value, found: &mut Vec<String>) {
        if masked == "••••••" {
            fn strings(v: &serde_json::Value, out: &mut Vec<String>) {
                match v {
                    serde_json::Value::String(s) if !s.is_empty() => out.push(s.clone()),
                    serde_json::Value::Array(a) => {
                        for v in a {
                            strings(v, out);
                        }
                    }
                    serde_json::Value::Object(m) => {
                        for v in m.values() {
                            strings(v, out);
                        }
                    }
                    _ => {}
                }
            }
            strings(value, found);
        } else {
            match value {
                serde_json::Value::Object(m) => {
                    for (k, v) in m {
                        secrets(v, &masked[k], found);
                    }
                }
                serde_json::Value::Array(a) => {
                    for (i, v) in a.iter().enumerate() {
                        secrets(v, &masked[i], found);
                    }
                }
                _ => {}
            }
        }
    }
    if let Some(doc) = &store.native {
        let mut found = vec![];
        secrets(doc, &config::redacted(doc), &mut found);
        found.sort_by_key(|s| std::cmp::Reverse(s.len()));
        for secret in found {
            e = e.replace(&secret, "[redacted]");
        }
    }
    if !store.secret.is_empty() {
        e = e.replace(&store.secret, "[redacted]");
    }
    model::clean_multiline(&e)
}
pub(crate) fn redact_sources<'a>(
    error: &str,
    sources: impl IntoIterator<Item = &'a str>,
) -> String {
    let mut result = error.to_string();
    for source in sources {
        for line in source.lines().filter(|s| !s.is_empty()) {
            result = result.replace(line, "[private value]");
            if let Ok(url) = url::Url::parse(line) {
                let mut parts = vec![url.username().to_string()];
                parts.extend(url.password().map(str::to_string));
                parts.extend(url.query_pairs().map(|(_, v)| v.into_owned()));
                for part in parts.into_iter().filter(|p| !p.is_empty()) {
                    result = result.replace(&part, "[private value]");
                }
            }
        }
    }
    result
}

fn startup_error(dir: &Path, offset: u64, store: &Store) -> String {
    use std::io::{Seek, SeekFrom};
    let read = || -> std::io::Result<String> {
        let mut file = fs::File::open(dir.join("core.log"))?;
        file.seek(SeekFrom::Start(offset))?;
        let mut data = vec![];
        file.take(65536).read_to_end(&mut data)?;
        let raw = String::from_utf8_lossy(&data);
        let ansi = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
        let raw = ansi.replace_all(&raw, "");
        let line = raw
            .lines()
            .rev()
            .find(|l| l.contains("FATAL") || l.contains("panic") || l.contains("fatal"));
        Ok(line.map(|l| redact_error(l, store)).unwrap_or_default())
    };
    let detail = read().unwrap_or_default();
    if detail.is_empty() {
        "see Activity / core.log".into()
    } else {
        detail
    }
}

pub(crate) async fn install_core(dir: &Path) -> Result<PathBuf> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        _ => bail!("Supported platforms: macOS / Linux"),
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        _ => bail!("Automatic installation supports arm64 / amd64. Set a custom core path."),
    };
    let asset = format!("sing-box-{CORE_VERSION}-{os}-{arch}.tar.gz");
    let base = format!("https://github.com/SagerNet/sing-box/releases/download/v{CORE_VERSION}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(150))
        .user_agent("sing/0.1")
        .build()?;
    // Pinned from https://github.com/SagerNet/sing-box/releases/expanded_assets/v1.14.0
    // Do not depend on the unauthenticated GitHub API at install time (rate limits).
    let expected = match (os, arch) {
        ("darwin", "arm64") => "a150c94012ff768b7261939cd236b9c8554127f45137230295d23a5660225cc9",
        ("darwin", "amd64") => "6cf26fc3501f3117cf781e9405cf5338f60add6da5affae39421af6800ebbcb4",
        ("linux", "arm64") => "04d9b40bc98dc55b6f509ce3292145c65478f65866bea64826ebb2f382385088",
        ("linux", "amd64") => "2375de6999f4f56ab46b4fc5ddf26a6aba1d3e61a0f4e7ddec2f4690457d5f63",
        _ => bail!("No verified core digest for this platform"),
    };
    let mut response = client
        .get(format!("{base}/{asset}"))
        .send()
        .await?
        .error_for_status()?;
    let mut data = vec![];
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            data.len() + chunk.len() < 150 * 1024 * 1024,
            "Core archive too large"
        );
        data.extend_from_slice(&chunk);
    }
    ensure!(
        format!("{:x}", Sha256::digest(&data)) == expected.to_lowercase(),
        "Core checksum mismatch; installation aborted"
    );
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(&data[..]));
    for item in archive.entries()? {
        let mut item = item?;
        if item.header().entry_type().is_file()
            && item.path()?.file_name().is_some_and(|n| n == "sing-box")
        {
            let mut binary = vec![];
            item.by_ref()
                .take(200 * 1024 * 1024)
                .read_to_end(&mut binary)?;
            let path = dir.join("bin/sing-box");
            model::atomic_write(&path, &binary)?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
            return Ok(path);
        }
    }
    bail!("Executable missing in the official archive")
}

fn helper_request(dir: &Path, action: &str) -> Result<String> {
    let mut s = UnixStream::connect(dir.join("tun.sock")).context("TUN authorization required")?;
    s.set_read_timeout(Some(Duration::from_secs(8)))?;
    writeln!(s, "{action}")?;
    let mut line = String::new();
    BufReader::new(s).take(1024).read_line(&mut line)?;
    ensure!(!line.starts_with("error"), "{}", line.trim());
    Ok(line.trim().into())
}

/// Explicitly launched by sudo from the foreground TUI. Only fixed lifecycle commands.
pub fn tun_helper(dir: &Path, core: &Path) -> Result<()> {
    ensure!(
        unsafe { libc::geteuid() } == 0,
        "TUN helper must be explicitly authorized using sudo"
    );
    let meta = fs::symlink_metadata(dir)?;
    ensure!(
        meta.is_dir() && meta.mode() & 0o077 == 0,
        "Data directory must be private"
    );
    ensure!(
        core.is_absolute() && core.is_file(),
        "Core must be an absolute executable path"
    );
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(dir.join("tun.lock"))?;
    lock.try_lock_exclusive()?;
    let socket = dir.join("tun.sock");
    if socket.exists() {
        fs::remove_file(&socket)?;
    }
    let listener = UnixListener::bind(&socket)?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))?;
    let cpath = std::ffi::CString::new(socket.as_os_str().as_encoded_bytes())?;
    ensure!(
        unsafe { libc::chown(cpath.as_ptr(), meta.uid(), meta.gid()) } == 0,
        "Cannot set helper socket owner"
    );
    let mut child: Option<Child> = None;
    for stream in listener.incoming() {
        let Ok(mut s) = stream else { continue };
        s.set_read_timeout(Some(Duration::from_secs(3)))?;
        let mut action = String::new();
        if BufReader::new(&s).take(64).read_line(&mut action).is_err() {
            continue;
        }
        if let Some(c) = &mut child {
            if c.try_wait()?.is_some() {
                child = None;
            }
        }
        let reply: Result<String> = (|| match action.trim() {
            "start" => {
                ensure!(child.is_none(), "Core already running");
                let config: serde_json::Value =
                    serde_json::from_slice(&fs::read(dir.join("runtime.json"))?)?;
                ensure!(
                    config["inbounds"]
                        .as_array()
                        .is_some_and(|a| a.iter().any(|i| i["type"] == "tun")),
                    "No TUN configured"
                );
                child = Some(spawn_core(core, dir)?);
                Ok("running".into())
            }
            "stop" | "exit" => {
                if let Some(mut c) = child.take() {
                    unsafe {
                        libc::kill(c.id() as i32, libc::SIGTERM);
                    };
                    for _ in 0..40 {
                        if c.try_wait()?.is_some() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    if c.try_wait()?.is_none() {
                        c.kill()?;
                        c.wait()?;
                    }
                }
                Ok("stopped".into())
            }
            "status" => Ok(if child.is_some() {
                "running"
            } else {
                "stopped"
            }
            .into()),
            _ => bail!("Unknown helper command"),
        })();
        let _ = writeln!(s, "{}", reply.unwrap_or_else(|e| format!("error: {e}")));
        if action.trim() == "exit" {
            break;
        }
    }
    if let Some(mut c) = child {
        let _ = c.kill();
        let _ = c.wait();
    }
    Ok(())
}

pub fn authorize_tun(dir: &Path, core: &str) -> Result<()> {
    // Create the log as the user before starting the root helper.
    OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(dir.join("core.log"))?;
    let result = Command::new("sudo")
        .arg("-b")
        .arg(std::env::current_exe()?)
        .arg("--data-dir")
        .arg(dir)
        .arg("--tun-helper")
        .arg("--core")
        .arg(fs::canonicalize(core)?)
        .status()?;
    ensure!(
        result.success(),
        "Administrator authorization cancelled or failed"
    );
    for _ in 0..30 {
        if helper_request(dir, "status").is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    bail!("TUN helper did not become ready")
}

#[cfg(test)]
mod tests {
    #[test]
    fn traffic_rate_needs_two_valid_samples_and_resets_after_counter_restart() {
        let base = super::api::Status {
            traffic_available: true,
            uplink_total: 100,
            downlink_total: 200,
            ..Default::default()
        };
        let next = super::api::Status {
            traffic_available: true,
            uplink_total: 150,
            downlink_total: 400,
            ..Default::default()
        };
        assert!(!super::traffic_sample(&base, next.clone(), 0, 10).traffic_available);
        assert!(!super::traffic_sample(&base, next.clone(), 10, 10).traffic_available);
        let measured = super::traffic_sample(&base, next.clone(), 10, 12);
        assert!(measured.traffic_available);
        assert_eq!((measured.uplink, measured.downlink), (25, 100));
        assert!(!super::traffic_sample(&next, base, 12, 14).traffic_available);
    }
    use super::*;
    use serde_json::json;
    #[test]
    fn failed_requests_redact_unsaved_sources_and_keep_long_diagnostics() {
        let source = "https://fixture.invalid/sub?token=top-secret";
        let message = format!(
            "Failed {source}\nQuery token top-secret\n{}",
            "details ".repeat(150)
        );
        let redacted = redact_sources(&message, [source]);
        assert!(!redacted.contains("top-secret") && !redacted.contains("fixture.invalid"));
        let reply = Reply::error(anyhow::anyhow!(redacted));
        assert!(reply.message.len() > 512 && reply.message.contains('\n'));
    }
    #[tokio::test]
    async fn native_remote_draft_is_atomic_cancelable_and_not_converted() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = draft_manager(dir.path());
        let before = m.store.native.clone();
        let p = m
            .handle(Action::PrepareRuleDraft {
                source: "https://fixture.invalid/video.srs?token=private".into(),
                name: "Video".into(),
                format: "auto".into(),
                revision: native::revision(&m.store),
            })
            .await
            .unwrap()
            .rules_preview
            .unwrap();
        assert_eq!(p.format, "native-srs");
        assert!(!serde_json::to_string(&p).unwrap().contains("token=private"));
        assert!(m
            .handle(Action::CommitRuleDraft {
                id: p.draft_id.clone(),
                revision: p.revision.clone(),
                target: "missing".into(),
                position: 0,
                group: None
            })
            .await
            .is_err());
        assert_eq!(m.store.native, before);
        m.handle(Action::CommitRuleDraft {
            id: p.draft_id,
            revision: p.revision,
            target: "direct".into(),
            position: 0,
            group: None,
        })
        .await
        .unwrap();
        let doc = m.store.native.as_ref().unwrap();
        assert_eq!(doc["route"]["rule_set"][0]["type"], "remote");
        assert_eq!(doc["route"]["rule_set"][0]["format"], "binary");
        assert!(m.store.rule_resources.is_empty());
        assert_eq!(doc["dns"], before.unwrap()["dns"]);
        let before = m.store.native.clone();
        let p = m
            .handle(Action::PrepareRuleDraft {
                source: "https://fixture.invalid/video.json".into(),
                name: String::new(),
                format: "native-source".into(),
                revision: native::revision(&m.store),
            })
            .await
            .unwrap()
            .rules_preview
            .unwrap();
        m.handle(Action::CancelRuleDraft(p.draft_id)).await.unwrap();
        assert_eq!(m.store.native, before);
    }
    #[tokio::test]
    async fn refresh_preview_retains_scope_and_recovers_from_fetch_failure() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = draft_manager(dir.path());
        let source = dir.path().join("nodes.txt");
        fs::write(&source, "trojan://fictional@127.0.0.1:9#A").unwrap();
        let p = m
            .prepare(
                source.to_string_lossy().into(),
                "A".into(),
                String::new(),
                None,
            )
            .await
            .unwrap()
            .preview
            .unwrap();
        m.store.settings.bypass_lan = !m.store.settings.bypass_lan;
        let fresh = m
            .handle(Action::ReprepareSubscriptions(p.id))
            .await
            .unwrap()
            .preview
            .unwrap();
        assert_eq!(fresh.revision, native::revision(&m.store));
        fs::write(&source, "<html>failure</html>").unwrap();
        assert!(m
            .handle(Action::ReprepareSubscriptions(fresh.id.clone()))
            .await
            .is_err());
        assert_eq!(m.pending.as_ref().unwrap().id, fresh.id);
        fs::write(&source, "trojan://fictional@127.0.0.1:9#A").unwrap();
        let fresh = m
            .handle(Action::ReprepareSubscriptions(fresh.id))
            .await
            .unwrap()
            .preview
            .unwrap();
        assert_eq!(fresh.count, 1);
        m.handle(Action::CommitSubscriptions {
            id: fresh.id,
            revision: fresh.revision,
        })
        .await
        .unwrap();
        assert_eq!(m.store.subscriptions.len(), 1);
    }
    fn draft_manager(dir: &Path) -> Manager {
        let mut store = Store::new().unwrap();
        let doc = native::migration(&store).unwrap();
        native::adopt(&mut store, doc).unwrap();
        store.native.as_mut().unwrap()["route"]["rules"] = json!([
            {"action":"sniff"}, {"domain_suffix":["existing.invalid"],"action":"route","outbound":"direct"}
        ]);
        store.save(dir).unwrap();
        Manager {
            pending_rules: None,
            lease: proxy_helper::Lease::start(dir),
            connectivity: Default::default(),
            dir: dir.into(),
            store,
            child: None,
            tun: false,
            running: None,
            pending: None,
            activity: vec![],
            status: Default::default(),
            groups: Default::default(),
            api_ready: false,
            version: String::new(),
            last_sample: 0,
            started_at: 0,
            selection_recovery: String::new(),
        }
    }
    async fn rule_draft(m: &mut Manager) -> Action {
        let revision = native::revision(&m.store);
        let p = m
            .handle(Action::PrepareRuleDraft {
                source: "HOST-SUFFIX,video.invalid,External\nUNSUPPORTED,ignored.invalid,External"
                    .into(),
                name: "Video".into(),
                format: "qx".into(),
                revision: revision.clone(),
            })
            .await
            .unwrap()
            .rules_preview
            .unwrap();
        assert_eq!(p.count, 1);
        assert!(!p.warnings.is_empty());
        Action::CommitRuleDraft {
            id: p.draft_id,
            revision: revision.clone(),
            target: "media".into(),
            position: 1,
            group: Some(native::GroupChange {
                revision,
                original_tag: None,
                name: "Media".into(),
                value: json!({"type":"selector","tag":"media","outbounds":["direct"]}),
            }),
        }
    }
    async fn import_fixture(m: &mut Manager, source: &Path, name: &str) {
        m.handle(Action::Import {
            source: source.display().to_string(),
            name: name.into(),
            user_agent: String::new(),
        })
        .await
        .unwrap();
        m.handle(Action::CommitImport).await.unwrap();
    }
    #[tokio::test]
    async fn update_all_previews_and_commits_once_preserving_native_policy() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = draft_manager(dir.path());
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, "trojan://fixture@127.0.0.1:9#A").unwrap();
        fs::write(&b, "trojan://fixture@127.0.0.1:10#B").unwrap();
        import_fixture(&mut m, &a, "A").await;
        import_fixture(&mut m, &b, "B").await;
        let before = serde_json::to_value(&m.store).unwrap();
        fs::write(
            &a,
            "trojan://fixture@127.0.0.1:9#A\ntrojan://fixture@127.0.0.1:11#A2",
        )
        .unwrap();
        fs::write(
            &b,
            "trojan://fixture@127.0.0.1:10#B\ntrojan://fixture@127.0.0.1:12#B2",
        )
        .unwrap();
        let p = m.handle(Action::RefreshAll).await.unwrap().preview.unwrap();
        assert_eq!(p.sources.len(), 2);
        assert_eq!(p.added, 2);
        assert_eq!(serde_json::to_value(&m.store).unwrap(), before);
        m.handle(Action::CancelSubscriptions("different-window".into()))
            .await
            .unwrap();
        assert!(m.pending.is_some());
        m.handle(Action::CommitSubscriptions {
            id: p.id.clone(),
            revision: p.revision.clone(),
        })
        .await
        .unwrap();
        assert_eq!(m.store.nodes.len(), 4);
        assert_eq!(
            m.store.native.as_ref().unwrap()["dns"],
            before["native"]["dns"]
        );
        assert_eq!(
            m.store.native.as_ref().unwrap()["route"],
            before["native"]["route"]
        );
        assert!(m
            .handle(Action::CommitSubscriptions {
                id: p.id,
                revision: p.revision
            })
            .await
            .is_err());
        assert!(m.running.is_none() && m.child.is_none());
        assert_eq!(Store::load(dir.path()).unwrap().native, m.store.native);
    }
    #[tokio::test]
    async fn update_all_failure_cancel_and_stale_preview_preserve_saved_state() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = draft_manager(dir.path());
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        fs::write(&a, "trojan://fixture@127.0.0.1:9#A").unwrap();
        fs::write(&b, "trojan://fixture@127.0.0.1:10#B").unwrap();
        import_fixture(&mut m, &a, "A").await;
        import_fixture(&mut m, &b, "B").await;
        let before = serde_json::to_value(&m.store).unwrap();
        fs::write(&b, "<html>expired</html>").unwrap();
        assert!(m.handle(Action::RefreshAll).await.is_err());
        assert!(m.pending.is_none());
        assert_eq!(serde_json::to_value(&m.store).unwrap(), before);
        fs::write(&b, "trojan://fixture@127.0.0.1:10#B").unwrap();
        let p = m.handle(Action::RefreshAll).await.unwrap().preview.unwrap();
        m.handle(Action::CancelSubscriptions(p.id.clone()))
            .await
            .unwrap();
        assert!(m
            .handle(Action::CommitSubscriptions {
                id: p.id,
                revision: p.revision
            })
            .await
            .is_err());
        let p = m.handle(Action::RefreshAll).await.unwrap().preview.unwrap();
        m.store.settings.bypass_lan = !m.store.settings.bypass_lan;
        assert!(m
            .handle(Action::CommitSubscriptions {
                id: p.id,
                revision: p.revision
            })
            .await
            .unwrap_err()
            .to_string()
            .contains("Draft changed"));
        assert_eq!(
            serde_json::to_value(Store::load(dir.path()).unwrap()).unwrap(),
            before
        );
    }
    #[tokio::test]
    async fn removing_referenced_subscription_nodes_is_a_non_destructive_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = draft_manager(dir.path());
        let a = dir.path().join("a.txt");
        fs::write(&a, "trojan://fixture@127.0.0.1:9#A").unwrap();
        import_fixture(&mut m, &a, "A").await;
        let node = m.store.nodes[0].tag();
        m.store.native.as_mut().unwrap()["route"]["rules"]
            .as_array_mut()
            .unwrap()
            .push(json!({"domain_suffix":["keep.invalid"],"outbound":node}));
        m.store.save(dir.path()).unwrap();
        let before = serde_json::to_value(&m.store).unwrap();
        fs::write(&a, "trojan://fixture@127.0.0.1:11#Replacement").unwrap();
        let p = m.handle(Action::RefreshAll).await.unwrap().preview.unwrap();
        let e = m
            .handle(Action::CommitSubscriptions {
                id: p.id,
                revision: p.revision,
            })
            .await
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("still referenced") && e.contains("/route/rules/2/outbound"),
            "{e}"
        );
        assert_eq!(serde_json::to_value(&m.store).unwrap(), before);
        assert!(m.pending.is_some());
    }
    #[tokio::test]
    async fn setup_initialization_requires_save_and_never_starts_the_core() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = draft_manager(dir.path());
        m.store = Store::new().unwrap();
        m.store.nodes = subscription::parse("trojan://fixture@127.0.0.1:9#A", "fixture")
            .unwrap()
            .nodes;
        m.store.save(dir.path()).unwrap();
        let before = serde_json::to_value(&m.store).unwrap();
        let info = m
            .handle(Action::ConnectionSetupInfo)
            .await
            .unwrap()
            .edit
            .unwrap();
        let change = native::ConnectionSetup {
            revision: info.revision,
            target: Some("proxy".into()),
            mode: "rule".into(),
            capture: "port".into(),
        };
        let r = m
            .handle(Action::ReviewConnectionSetup(change))
            .await
            .unwrap();
        assert!(r.config.as_ref().unwrap().contains("Preview only"));
        assert!(!r.config.as_ref().unwrap().contains("Apply restarts"));
        assert_eq!(serde_json::to_value(&m.store).unwrap(), before);
        assert!(!dir.path().join("pre-native-state.json").exists());
        let action = r.confirm.unwrap();
        let saved = m.handle(action.clone()).await.unwrap();
        assert!(m.store.native.is_some());
        assert!(dir.path().join("pre-native-state.json").exists());
        assert!(matches!(saved.confirm, Some(Action::ApplyNative { .. })));
        assert!(m.running.is_none() && m.child.is_none());
        assert!(m.handle(action).await.is_err());
    }
    #[tokio::test]
    async fn rule_group_route_commit_is_atomic_and_preserves_dns_and_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = draft_manager(dir.path());
        let before = serde_json::to_value(&m.store).unwrap();
        let action = rule_draft(&mut m).await;
        assert_eq!(serde_json::to_value(&m.store).unwrap(), before);
        assert_eq!(
            serde_json::to_value(Store::load(dir.path()).unwrap()).unwrap(),
            before
        );
        m.handle(action.clone()).await.unwrap();
        let doc = m.store.native.as_ref().unwrap();
        assert_eq!(doc["dns"], before["native"]["dns"]);
        assert_eq!(
            doc["route"]["rules"][0],
            before["native"]["route"]["rules"][0]
        );
        assert_eq!(doc["route"]["rules"][1]["outbound"], "media");
        assert_eq!(
            doc["route"]["rules"][2],
            before["native"]["route"]["rules"][1]
        );
        assert_eq!(m.store.rule_resources.len(), 1);
        assert_eq!(m.store.display_names["media"], "Media");
        assert!(m.pending_rules.is_none());
        assert_eq!(Store::load(dir.path()).unwrap().native, m.store.native);
        assert!(m.handle(action).await.is_err()); // A preview cannot be saved twice.
        assert!(m.child.is_none() && m.running.is_none());
    }
    #[tokio::test]
    async fn failed_or_cancelled_rule_drafts_never_leave_an_orphan_group() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = draft_manager(dir.path());
        let action = rule_draft(&mut m).await;
        let before = serde_json::to_value(&m.store).unwrap();
        for case in 0..5 {
            let mut bad = action.clone();
            if let Action::CommitRuleDraft {
                id,
                target,
                position,
                group,
                ..
            } = &mut bad
            {
                match case {
                    0 => *position = 999,
                    1 => *id = "replaced-preview".into(),
                    2 => *target = "direct".into(),
                    3 => group.as_mut().unwrap().value["outbounds"] = json!([]),
                    _ => group.as_mut().unwrap().revision = "stale".into(),
                }
            }
            assert!(m.handle(bad).await.is_err());
            assert_eq!(serde_json::to_value(&m.store).unwrap(), before);
            assert_eq!(
                serde_json::to_value(Store::load(dir.path()).unwrap()).unwrap(),
                before
            );
            assert!(m.pending_rules.is_some());
        }
        let id = m.pending_rules.as_ref().unwrap().id.clone();
        m.handle(Action::CancelRuleDraft("another-window".into()))
            .await
            .unwrap();
        assert!(m.pending_rules.is_some());
        m.handle(Action::CancelRuleDraft(id)).await.unwrap();
        assert!(m.pending_rules.is_none());
        assert!(m.handle(action).await.is_err());
        assert_eq!(serde_json::to_value(&m.store).unwrap(), before);
        let action = rule_draft(&mut m).await;
        m.store.native.as_mut().unwrap()["dns"]["timeout"] = json!("7s");
        assert!(m
            .handle(action)
            .await
            .unwrap_err()
            .to_string()
            .contains("Draft changed"));
        assert!(m.store.display_names.is_empty());
    }
    #[tokio::test]
    async fn rule_save_io_error_keeps_preview_for_retry() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = draft_manager(dir.path());
        let action = rule_draft(&mut m).await;
        let before = serde_json::to_value(&m.store).unwrap();
        let blocked = dir.path().join("not-a-directory");
        fs::write(&blocked, b"fixture").unwrap();
        m.dir = blocked;
        assert!(m.handle(action.clone()).await.is_err());
        assert_eq!(serde_json::to_value(&m.store).unwrap(), before);
        assert!(m.pending_rules.is_some());
        m.dir = dir.path().into();
        m.handle(action).await.unwrap();
    }
    #[tokio::test]
    async fn native_rule_import_and_refresh_preserve_compound_and_future_fields() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = draft_manager(dir.path());
        let source = dir.path().join("native-rules.json");
        let mut doc = json!({"version":3,"rules":[
            {"type":"logical","mode":"and","rules":[{"domain_suffix":["native.invalid"]},{"process_name":["Fixture"]}]},
            {"domain":["future.invalid"],"future_predicate":{"must_preserve":true}}
        ]});
        fs::write(&source, doc.to_string()).unwrap();
        let dns = m.store.native.as_ref().unwrap()["dns"].clone();
        let revision = native::revision(&m.store);
        let p = m
            .handle(Action::PrepareRuleDraft {
                source: source.display().to_string(),
                name: "Native".into(),
                format: "auto".into(),
                revision: revision.clone(),
            })
            .await
            .unwrap()
            .rules_preview
            .unwrap();
        assert_eq!(p.format, "native");
        assert_eq!(p.count, 2);
        assert!(p.warnings.is_empty());
        m.handle(Action::CommitRuleDraft {
            id: p.draft_id,
            revision,
            target: "direct".into(),
            position: 0,
            group: None,
        })
        .await
        .unwrap();
        let resource = m.store.rule_resources[0].clone();
        assert_eq!(resource.native_document, Some(doc.clone()));
        assert_eq!(
            m.store.native.as_ref().unwrap()["route"]["rule_set"][0]["rules"],
            doc["rules"]
        );
        let routes = m.store.native.as_ref().unwrap()["route"]["rules"].clone();
        doc["rules"][0]["rules"][0]["domain_suffix"] = json!(["updated.invalid"]);
        fs::write(&source, doc.to_string()).unwrap();
        m.handle(Action::RefreshRules(resource.id.clone()))
            .await
            .unwrap();
        m.handle(Action::CommitRules).await.unwrap();
        assert_eq!(
            m.store.native.as_ref().unwrap()["route"]["rule_set"][0]["rules"],
            doc["rules"]
        );
        assert_eq!(m.store.native.as_ref().unwrap()["route"]["rules"], routes);
        assert_eq!(m.store.native.as_ref().unwrap()["dns"], dns);
        // A user's local rule-set edit must never be replaced by refresh.
        m.store.native.as_mut().unwrap()["route"]["rule_set"][0]["rules"][0]["invert"] =
            json!(true);
        doc["rules"][0]["mode"] = json!("or");
        fs::write(&source, doc.to_string()).unwrap();
        let before = m.store.native.clone();
        m.handle(Action::RefreshRules(resource.id)).await.unwrap();
        assert!(m.handle(Action::CommitRules).await.is_err());
        assert_eq!(m.store.native, before);
    }
    #[tokio::test]
    #[ignore = "Loopback-only HTTP proxy responses; no public endpoint or real node"]
    async fn connectivity_probe_requires_204_and_does_not_follow_redirects() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for (status, expected) in [
            ("204 No Content", "passed"),
            ("200 OK", "failed"),
            ("302 Found", "failed"),
        ] {
            let proxy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = proxy.local_addr().unwrap().port();
            let server = tokio::spawn(async move {
                let (mut socket, _) = proxy.accept().await.unwrap();
                let mut buf = [0; 4096];
                let n = socket.read(&mut buf).await.unwrap();
                assert!(
                    String::from_utf8_lossy(&buf[..n]).starts_with("GET http://fixture.invalid/")
                );
                socket.write_all(format!("HTTP/1.1 {status}\r\nLocation: http://never-follow.invalid/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
            });
            let result = probe(port, "http://fixture.invalid/").await;
            assert_eq!(result.state, expected);
            assert!(result.checked_at > 0);
            server.await.unwrap();
        }
    }
    #[tokio::test]
    #[ignore = "Downloads an official core into the workspace; requires network"]
    async fn install_official_core() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".build/test-core");
        let path = install_core(&dir).await.unwrap();
        assert!(supported_version(&core_version(&path).await.unwrap()));
        println!("Verified core: {}", path.display());
    }
    #[tokio::test]
    #[ignore = "Requires SING_TEST_CORE; starts loopback-only services, no TUN"]
    async fn real_core_lifecycle_and_grpc() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let core = std::env::var("SING_TEST_CORE").expect("Set SING_TEST_CORE to sing-box 1.14+");
        let dir = tempfile::tempdir().unwrap();
        let free_port = || {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let mut store = Store::new().unwrap();
        store.settings.core = core;
        store.settings.port = free_port();
        store.settings.api_port = free_port();
        store.nodes = subscription::parse(
            "trojan://fake@127.0.0.1:9#A\ntrojan://fake@127.0.0.1:10#B",
            "fixture",
        )
        .unwrap()
        .nodes;
        store.selected = Some(store.nodes[0].id.clone());
        store.save(dir.path()).unwrap();
        let mut m = Manager {
            pending_rules: None,
            lease: proxy_helper::Lease::start(dir.path()),
            connectivity: ProbeStatus::default(),
            dir: dir.path().into(),
            store,
            child: None,
            tun: false,
            running: None,
            pending: None,
            activity: vec![],
            status: Default::default(),
            groups: Default::default(),
            api_ready: false,
            version: String::new(),
            last_sample: 0,
            started_at: 0,
            selection_recovery: String::new(),
        };
        let response = m.handle(Action::Connect).await;
        if let Err(e) = &response {
            panic!(
                "{e:#}\n{}",
                fs::read_to_string(dir.path().join("core.log")).unwrap_or_default()
            );
        }
        assert!(response.unwrap().ok);
        let snap = m.snapshot().await;
        assert!(snap.connected && snap.api_ready);
        assert!(!snap.dirty);
        assert!(snap.groups.group.iter().any(|g| g.tag == "proxy"));
        let second = m.store.nodes[1].id.clone();
        m.handle(Action::Select(second.clone())).await.unwrap();
        let snap = m.snapshot().await;
        assert_eq!(
            snap.groups
                .group
                .iter()
                .find(|g| g.tag == "proxy")
                .unwrap()
                .selected,
            format!("n-{second}")
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = [0; 4096];
            let received = s.read(&mut buf).await.unwrap();
            assert!(received > 0);
            s.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\nConnection: close\r\n\r\nloopback-ok!",
            )
            .await
            .unwrap();
        });
        let client = reqwest::Client::builder()
            .proxy(
                reqwest::Proxy::all(format!("http://127.0.0.1:{}", m.store.settings.port)).unwrap(),
            )
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let result = client
            .get(format!("http://{origin}/"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert_eq!(result, "loopback-ok!");
        server.await.unwrap();
        // A failed refresh never changes a saved subscription / node set.
        let before = serde_json::to_value(&m.store).unwrap();
        assert!(m
            .prepare("bad://unsupported".into(), "broken".into(), "".into(), None)
            .await
            .is_err());
        assert_eq!(before, serde_json::to_value(&m.store).unwrap());
        // A port collision on Apply restores the previous working config.
        let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let old_port = m.store.settings.port;
        m.store.settings.port = occupied.local_addr().unwrap().port();
        assert!(m.handle(Action::Connect).await.is_err());
        assert!(m.connected());
        assert_eq!(m.running.as_ref().unwrap().settings.port, old_port);
        m.store.settings.port = old_port;
        let doc = native::migration(&m.store).unwrap();
        native::adopt(&mut m.store, doc).unwrap();
        m.handle(Action::Connect).await.unwrap();
        let member = m.store.nodes[0].tag();
        m.handle(Action::SelectNative {
            group: "proxy".into(),
            member: member.clone(),
        })
        .await
        .unwrap();
        let remembered = fs::read(dir.path().join("selections.json")).unwrap();
        let draft = m.store.native.clone();
        let secret = std::mem::replace(&mut m.store.secret, "wrong-api-secret".into());
        assert!(m
            .handle(Action::SelectNative {
                group: "proxy".into(),
                member
            })
            .await
            .is_err());
        let failed = m.snapshot().await;
        assert!(failed.connected && !failed.api_ready && failed.groups.group.is_empty());
        assert_eq!(
            fs::read(dir.path().join("selections.json")).unwrap(),
            remembered
        );
        assert_eq!(m.store.native, draft);
        m.store.secret = secret;
        m.handle(Action::Disconnect).await.unwrap();
        assert!(!m.connected());
        println!("PASS: config check, native gRPC, live selector, local HTTP proxy, failed import preservation, failed apply rollback, disconnect");
    }
    #[test]
    fn supported_versions() {
        assert!(supported_version("sing-box version 1.14.0"));
        assert!(supported_version("sing-box version 1.15.0-alpha.1"));
        assert!(!supported_version("sing-box version 1.13.0"));
    }
    #[test]
    fn subscription_label_hides_token() {
        let label = source_label("https://provider.test/api?token=private-secret");
        assert!(!label.contains("private-secret"));
    }
    #[test]
    fn startup_failure_uses_only_this_attempt_and_redacts_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::new().unwrap();
        store.nodes = subscription::parse("trojan://private-pass@127.0.0.1:9#Fixture", "fixture")
            .unwrap()
            .nodes;
        let old = "FATAL previous failure\n";
        let new = format!(
            "{old}\x1b[31mFATAL\x1b[0m start DNS private-pass {}\n",
            store.secret
        );
        fs::write(dir.path().join("core.log"), &new).unwrap();
        let error = startup_error(dir.path(), old.len() as u64, &store);
        assert!(error.contains("FATAL start DNS"), "{error}");
        assert!(
            !error.contains("previous")
                && !error.contains("private-pass")
                && !error.contains(&store.secret)
        );
        assert!(!error.contains("[31m"));
        assert_eq!(
            startup_error(dir.path(), new.len() as u64, &store),
            "see Activity / core.log"
        );
    }
}
