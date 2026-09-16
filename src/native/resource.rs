//! Native local/remote resources stay native; sing never sends them through a
//! cross-format converter or downloads them on behalf of the core.
use super::*;

pub fn reference(source: &str, format: &str) -> Result<Value> {
    let source = source.trim();
    ensure!(
        !source.is_empty(),
        "Enter a native rule-set URL or local path"
    );
    let format = match format {
        "native-source" => "source",
        "native-srs" => "binary",
        _ => bail!("Choose Native Source or Native SRS"),
    };
    let mut value = if source.starts_with("https://") || source.starts_with("http://") {
        let url = url::Url::parse(source).context("Invalid rule-set URL")?;
        ensure!(url.host_str().is_some(), "Rule-set URL needs a host");
        json!({"type":"remote","url":source,"format":format})
    } else {
        let path =
            std::fs::canonicalize(source).context("Native rule-set file not found on this host")?;
        ensure!(path.is_file(), "Native rule-set path must be a file");
        json!({"type":"local","path":path,"format":format})
    };
    value["tag"] = json!(format!(
        "native-rs-{}",
        crate::model::id(&value.to_string())
    ));
    Ok(value)
}

pub fn bind(
    store: &mut Store,
    value: Value,
    name: &str,
    target: &str,
    position: usize,
) -> Result<()> {
    let mut next = store.clone();
    let doc = next
        .native
        .as_mut()
        .context("Initialize native configuration first")?;
    ensure!(
        target == "reject"
            || ["/outbounds", "/endpoints"]
                .iter()
                .any(|p| array(doc, p).iter().any(|v| tag(v) == target)),
        "Choose an existing target"
    );
    let mut rules = array(doc, "/route/rules").to_vec();
    ensure!(
        position <= rules.len(),
        "Rule order changed; review insertion position"
    );
    let tag = tag(&value).to_string();
    ensure!(!tag.is_empty(), "Native rule-set needs a tag");
    let mut sets = array(doc, "/route/rule_set").to_vec();
    if let Some(existing) = sets
        .iter()
        .find(|v| super::tag(v) == tag || array(v, "/tag").iter().any(|v| v == &tag))
    {
        ensure!(*existing == value, "Native rule-set already has local edits; reuse it from Add Rule instead of overwriting");
    } else {
        sets.push(value);
    }
    let rule = if target == "reject" {
        json!({"rule_set":[tag],"action":"reject"})
    } else {
        json!({"rule_set":[tag],"action":"route","outbound":target})
    };
    rules.insert(position, rule);
    set(doc, "/route/rule_set", json!(sets))?;
    set(doc, "/route/rules", json!(rules))?;
    shape(doc)?;
    if !name.trim().is_empty() {
        next.display_names.insert(tag, crate::model::clean(name));
    }
    *store = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_references_keep_type_format_and_are_atomic() {
        let value = reference(
            "https://example.invalid/video.srs?private=token",
            "native-srs",
        )
        .unwrap();
        assert_eq!(value["type"], "remote");
        assert_eq!(value["format"], "binary");
        let mut store = Store::new().unwrap();
        store.native = Some(
            json!({"outbounds":[{"type":"direct","tag":"direct"}],"dns":{"future":"keep"},"route":{"rules":[]}}),
        );
        let before = store.native.clone();
        assert!(bind(&mut store, value.clone(), "Video", "missing", 0).is_err());
        assert_eq!(store.native, before);
        bind(&mut store, value.clone(), "Video", "direct", 0).unwrap();
        bind(&mut store, value.clone(), "Video", "reject", 1).unwrap();
        assert_eq!(
            array(store.native.as_ref().unwrap(), "/route/rule_set"),
            &[value]
        );
        assert_eq!(
            store.native.as_ref().unwrap()["dns"],
            before.unwrap()["dns"]
        );
        assert!(store.rule_resources.is_empty());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("video.json");
        crate::model::atomic_write(
            &path,
            br#"{"version":1,"rules":[{"domain_suffix":["example.test"]}]}"#,
        )
        .unwrap();
        let local = reference(path.to_str().unwrap(), "native-source").unwrap();
        assert_eq!(local["type"], "local");
        assert_eq!(local["format"], "source");
        assert!(reference("missing-file.srs", "native-srs").is_err());
    }
}
