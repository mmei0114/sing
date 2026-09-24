use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn id(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))[..20].to_string()
}
pub fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| {
            !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(512)
        .collect()
}
pub fn clean_multiline(text: &str) -> String {
    text.chars()
        .filter(|c| {
            (!c.is_control() || *c == '\n')
                && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(32768)
        .collect()
}
pub fn default_dir() -> PathBuf {
    let base = if let Some(base) = std::env::var_os("XDG_DATA_HOME") {
        PathBuf::from(base)
    } else {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into()));
        if cfg!(target_os = "macos") {
            home.join("Library/Application Support")
        } else {
            home.join(".local/share")
        }
    };
    compatible_dir(&base)
}

fn compatible_dir(base: &Path) -> PathBuf {
    let current = base.join("sing");
    let legacy = base.join("sbtui");
    // Preserve saved subscriptions and the socket of an already running manager.
    // Do not move a live Unix socket, copy private credentials, or restart a core.
    if !current.exists() && legacy.exists() {
        legacy
    } else {
        current
    }
}
pub fn private_dir(path: &Path) -> Result<()> {
    if path.exists() && fs::symlink_metadata(path)?.file_type().is_symlink() {
        bail!("Data directory must not be a symbolic link");
    }
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path.parent().context("Missing parent directory")?;
    private_dir(parent)?;
    let temp = parent.join(format!(".write-{}-{}", std::process::id(), token()?));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)?;
    let result = (|| -> Result<()> {
        file.write_all(data)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
pub fn token() -> Result<String> {
    let mut bytes = [0u8; 24];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Node {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub outbound: Value,
    #[serde(default)]
    pub favorite: bool,
}
impl Node {
    pub fn kind(&self) -> &str {
        self.outbound["type"].as_str().unwrap_or("unknown")
    }
    pub fn tag(&self) -> String {
        format!("n-{}", self.id)
    }
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Subscription {
    pub id: String,
    pub name: String,
    pub source: String,
    pub format: String,
    pub updated_at: u64,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub user_agent: String,
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Rule {
    pub kind: String,
    pub value: String,
    pub target: String,
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct ProxyGroup {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub members: Vec<String>,
    pub selected: Option<String>,
}
impl ProxyGroup {
    pub fn tag(&self) -> String {
        format!("g-{}", self.id)
    }
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
pub struct MatchRule {
    pub kind: String,
    pub value: String,
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct RuleResource {
    pub id: String,
    pub name: String,
    pub source: String,
    pub format: String,
    pub updated_at: u64,
    pub digest: String,
    pub input_count: usize,
    pub rules: Vec<MatchRule>,
    /// Original native JSON rule-set; never flatten compound predicates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_document: Option<Value>,
    pub warnings: Vec<String>,
}
impl RuleResource {
    pub fn native_rules(&self) -> Vec<Value> {
        self.native_document
            .as_ref()
            .and_then(|d| d["rules"].as_array())
            .cloned()
            .unwrap_or_else(|| crate::ruleset::native_rules(&self.rules))
    }
    pub fn tag(&self) -> String {
        format!("rs-{}", self.id)
    }
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct RuleBinding {
    pub id: String,
    pub resource: String,
    pub target: String,
    pub enabled: bool,
}
fn legacy_dns() -> String {
    "legacy".into()
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub language: String,
    pub mode: String,
    pub routing: String,
    pub route_mode: String,
    pub global_target: String,
    pub bypass_lan: bool,
    pub dns: String,
    #[serde(default = "legacy_dns")]
    pub dns_policy: String,
    #[serde(default = "ipv4_strategy")]
    pub dns_strategy: String,
    pub port: u16,
    pub api_port: u16,
    pub core: String,
    pub rules: Vec<Rule>,
    pub auto_update: bool,
    /// The TUN inbound removed by the TUN switch, restored verbatim when the
    /// switch is turned on again so custom TUN options are not lost.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parked_tun: Option<Value>,
}
fn ipv4_strategy() -> String {
    "ipv4_only".into()
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "en".into(),
            mode: "port".into(),
            routing: "proxy".into(),
            route_mode: "rule".into(),
            global_target: "proxy".into(),
            bypass_lan: true,
            dns: "1.1.1.1".into(),
            dns_policy: "paired".into(),
            dns_strategy: ipv4_strategy(),
            port: 2080,
            api_port: 2090,
            core: String::new(),
            rules: vec![],
            auto_update: false,
            parked_tun: None,
        }
    }
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Store {
    pub schema: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native: Option<Value>,
    /// Client labels; never serialized into the sing-box native document.
    #[serde(default)]
    pub display_names: std::collections::BTreeMap<String, String>,
    pub secret: String,
    pub subscriptions: Vec<Subscription>,
    pub nodes: Vec<Node>,
    pub selected: Option<String>,
    #[serde(default)]
    pub proxy_groups: Vec<ProxyGroup>,
    #[serde(default)]
    pub rule_resources: Vec<RuleResource>,
    #[serde(default)]
    pub rule_bindings: Vec<RuleBinding>,
    pub settings: Settings,
    #[serde(default)]
    pub last_check: u64,
}
impl Store {
    pub fn new() -> Result<Self> {
        Ok(Self {
            schema: 1,
            native: None,
            display_names: Default::default(),
            secret: token()?,
            subscriptions: vec![],
            nodes: vec![],
            selected: None,
            proxy_groups: vec![],
            rule_resources: vec![],
            rule_bindings: vec![],
            settings: Settings::default(),
            last_check: 0,
        })
    }
    pub fn load(dir: &Path) -> Result<Self> {
        let file = dir.join("state.json");
        if !file.exists() {
            let mut store = Self::new()?;
            let doc = crate::native::defaults::document(&store);
            crate::native::adopt(&mut store, doc)?;
            return Ok(store);
        }
        let data: Self = serde_json::from_slice(&fs::read(file)?)
            .context("Cannot read saved state; original file was not changed")?;
        if ![1, 2].contains(&data.schema) {
            bail!("Unsupported state version {}", data.schema);
        }
        Ok(data)
    }
    pub fn save(&self, dir: &Path) -> Result<()> {
        atomic_write(&dir.join("state.json"), &serde_json::to_vec_pretty(self)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pre_mode_settings_keep_rule_routing_and_lan_defaults() {
        let mut old = serde_json::to_value(Settings::default()).unwrap();
        let fields = old.as_object_mut().unwrap();
        for field in ["route_mode", "global_target", "bypass_lan"] {
            fields.remove(field);
        }
        fields.insert("routing".into(), serde_json::json!("direct"));
        let s: Settings = serde_json::from_value(old).unwrap();
        assert_eq!(s.route_mode, "rule");
        assert_eq!(s.global_target, "proxy");
        assert!(s.bypass_lan);
        assert_eq!(s.routing, "direct");
    }
    #[test]
    fn rename_preserves_legacy_state() {
        let d = tempfile::tempdir().unwrap();
        let legacy = d.path().join("sbtui");
        let mut saved = Store::new().unwrap();
        saved.settings.port = 4321;
        saved.save(&legacy).unwrap();
        assert_eq!(compatible_dir(d.path()), legacy);
        assert_eq!(
            Store::load(&compatible_dir(d.path()))
                .unwrap()
                .settings
                .port,
            4321
        );
        assert!(!d.path().join("sing").exists());
    }
    #[test]
    fn fresh_install_and_explicit_new_directory_use_sing() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(compatible_dir(d.path()), d.path().join("sing"));
        fs::create_dir(d.path().join("sbtui")).unwrap();
        fs::create_dir(d.path().join("sing")).unwrap();
        assert_eq!(compatible_dir(d.path()), d.path().join("sing"));
    }
    #[test]
    fn private_atomic_store() {
        let d = tempfile::tempdir().unwrap();
        let mut s = Store::new().unwrap();
        s.settings.rules.push(Rule {
            kind: "domain_suffix".into(),
            value: "example.com".into(),
            target: "direct".into(),
        });
        s.save(d.path()).unwrap();
        assert_eq!(Store::load(d.path()).unwrap().settings, s.settings);
        assert_eq!(
            fs::metadata(d.path().join("state.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    #[test]
    fn corrupt_store_is_not_replaced() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("state.json"), "broken").unwrap();
        assert!(Store::load(d.path()).is_err());
        assert_eq!(
            fs::read_to_string(d.path().join("state.json")).unwrap(),
            "broken"
        );
    }
    #[test]
    fn sanitize_terminal_controls() {
        assert!(!clean("x\x1b\n\u{202e}y").contains('\x1b'));
    }
}
