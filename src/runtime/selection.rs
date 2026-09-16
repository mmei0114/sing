use super::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Serialize, Deserialize)]
pub struct Remembered {
    pub member: String,
    pub default: Value,
}
pub type Memory = BTreeMap<String, Remembered>;
pub fn load(dir: &Path) -> Result<Memory> {
    let path = dir.join("selections.json");
    if !path.exists() {
        return Ok(Memory::new());
    }
    serde_json::from_slice(&fs::read(path)?)
        .context("Cannot read selections.json; original file was kept")
}
pub fn save(dir: &Path, memory: &Memory) -> Result<()> {
    model::atomic_write(
        &dir.join("selections.json"),
        &serde_json::to_vec_pretty(memory)?,
    )
}
pub fn core_cache(store: &Store) -> bool {
    store
        .native
        .as_ref()
        .and_then(|d| d.pointer("/experimental/cache_file/enabled"))
        == Some(&json!(true))
}
pub fn restorable<'a>(store: &Store, memory: &'a Memory) -> Vec<(&'a str, &'a str)> {
    if core_cache(store) {
        return vec![];
    }
    let Some(doc) = &store.native else {
        return vec![];
    };
    memory
        .iter()
        .filter_map(|(tag, choice)| {
            native::array(doc, "/outbounds")
                .iter()
                .find(|g| {
                    native::tag(g) == tag
                        && g["type"] == "selector"
                        && g["default"] == choice.default
                        && native::array(g, "/outbounds")
                            .iter()
                            .any(|m| m == &choice.member)
                })
                .map(|_| (tag.as_str(), choice.member.as_str()))
        })
        .collect()
}
pub fn summary(store: &Store, memory: &Memory) -> String {
    if core_cache(store) {
        return "Selection recovery: native cache owns restoration; sing will not override it.\n"
            .into();
    }
    let plan = restorable(store, memory);
    let mut result = String::from("Selection recovery (API-confirmed after startup):\n");
    if plan.is_empty() {
        result.push_str("  Native defaults; no compatible remembered choices.\n");
    }
    for (tag, member) in &plan {
        result.push_str(&format!(
            "  {} → {}\n",
            model::clean(tag),
            model::clean(member)
        ));
    }
    if memory.len() > plan.len() {
        result.push_str("  Changed defaults, removed members and automatic groups take precedence over old choices.\n");
    }
    result
}

impl Manager {
    pub(super) async fn restore_selections(&mut self, store: &Store) {
        if core_cache(store) {
            self.selection_recovery = "Native cache owns selection recovery".into();
            return;
        }
        let memory = match load(&self.dir) {
            Ok(v) => v,
            Err(e) => {
                self.selection_recovery = format!("Selection recovery unavailable: {e}");
                self.log(e.to_string());
                return;
            }
        };
        for (tag, member) in restorable(store, &memory) {
            let result = async {
                self.api()
                    .await?
                    .select_confirmed(tag.into(), member.into())
                    .await
            }
            .await;
            match result {
                Ok(groups) => {
                    self.groups = groups;
                    self.log(format!(
                        "Restored selection {} → {} (API confirmed)",
                        model::clean(tag),
                        model::clean(member)
                    ));
                }
                Err(e) => {
                    self.groups = api::Groups::default();
                    self.selection_recovery = format!(
                        "Selection restore failed for {}. See Activity / Logs.",
                        model::clean(tag)
                    );
                    self.log(format!("Selection restore failed for {}: {e}. Read current groups before retrying.",model::clean(tag)));
                }
            }
        }
    }
    pub(super) async fn select_native(&mut self, group: String, member: String) -> Result<Reply> {
        ensure!(
            self.connected(),
            "Start the core to select a live member. To change startup defaults, use Edit Group."
        );
        let loaded = self
            .running
            .as_ref()
            .and_then(|s| s.native.as_ref())
            .context("Apply native draft first")?;
        let live = native::array(loaded, "/outbounds")
            .iter()
            .find(|g| native::tag(g) == group)
            .context("Apply this group first")?;
        ensure!(
            live["type"] == "selector"
                && native::array(live, "/outbounds")
                    .iter()
                    .any(|m| m == &member),
            "Apply group membership first; this member is not loaded"
        );
        let default = live["default"].clone();
        let mut memory = load(&self.dir)?;
        self.groups = api::Groups::default();
        let groups = self
            .api()
            .await?
            .select_confirmed(group.clone(), member.clone())
            .await?;
        self.groups = groups;
        memory.insert(group, Remembered { member, default });
        if let Err(e) = save(&self.dir, &memory) {
            bail!("Selection is API-confirmed, but could not remember it for restart: {e}");
        }
        Ok(Reply::success("Live selection confirmed. Default member unchanged; existing connections may keep their current route."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_edits_membership_and_native_cache_override_memory() {
        let mut store = Store::new().unwrap();
        store.native = Some(
            json!({"outbounds":[{"type":"selector","tag":"g","outbounds":["a","b"],"default":"a"}]}),
        );
        let memory = Memory::from([(
            "g".into(),
            Remembered {
                member: "b".into(),
                default: json!("a"),
            },
        )]);
        assert_eq!(restorable(&store, &memory), vec![("g", "b")]);
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), &memory).unwrap();
        assert_eq!(load(dir.path()).unwrap()["g"].member, "b");
        store.native.as_mut().unwrap()["outbounds"][0]["default"] = json!("b");
        assert!(restorable(&store, &memory).is_empty());
        store.native.as_mut().unwrap()["outbounds"][0]["default"] = json!("a");
        store.native.as_mut().unwrap()["outbounds"][0]["type"] = json!("urltest");
        assert!(restorable(&store, &memory).is_empty());
        store.native.as_mut().unwrap()["outbounds"][0]["type"] = json!("selector");
        store.native.as_mut().unwrap()["experimental"] = json!({"cache_file":{"enabled":true}});
        assert!(restorable(&store, &memory).is_empty());
        assert!(summary(&store, &memory).contains("native cache owns"));
    }
}
