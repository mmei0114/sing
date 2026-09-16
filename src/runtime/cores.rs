//! Core inventory: what is installed, which binary is selected, and which
//! official releases can be downloaded with a published SHA-256 digest.
use super::*;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CoreInfo {
    pub path: String,
    pub version: String,
    /// "sing" (downloaded by sing), "PATH" or "custom".
    pub source: String,
    pub selected: bool,
    pub supported: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    pub prerelease: bool,
    pub published: String,
    pub installed: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CoreReport {
    pub installed: Vec<CoreInfo>,
    pub releases: Vec<Release>,
    pub releases_error: String,
    pub running: bool,
    pub tested: String,
}

fn platform() -> Result<(&'static str, &'static str)> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        _ => bail!("Supported platforms: macOS / Linux"),
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        _ => bail!("Automatic installation supports arm64 / amd64. Choose a custom core path."),
    };
    Ok((os, arch))
}
fn valid_version(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 40
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
}
fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(150))
        .user_agent(concat!("sing/", env!("CARGO_PKG_VERSION")))
        .build()?)
}
async fn releases() -> Result<Vec<serde_json::Value>> {
    let list: Vec<serde_json::Value> = client()?
        .get("https://api.github.com/repos/SagerNet/sing-box/releases?per_page=40")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()
        .context("GitHub release list unavailable (rate limit or network)")?
        .json()
        .await?;
    Ok(list)
}

impl Manager {
    fn core_candidates(&self) -> Vec<(PathBuf, &'static str)> {
        let mut found: Vec<(PathBuf, &'static str)> = vec![];
        let mut push = |p: PathBuf, source| {
            if p.is_file() && !found.iter().any(|(q, _)| q == &p) {
                found.push((p, source));
            }
        };
        if let Ok(entries) = fs::read_dir(self.dir.join("cores")) {
            let mut dirs: Vec<_> = entries.flatten().map(|e| e.path()).collect();
            dirs.sort();
            dirs.reverse();
            for d in dirs {
                push(d.join("sing-box"), "sing");
            }
        }
        push(self.dir.join("bin/sing-box"), "sing");
        if let Some(path) = std::env::var_os("PATH") {
            for d in std::env::split_paths(&path) {
                push(d.join("sing-box"), "PATH");
            }
        }
        let custom = self.store.settings.core.trim();
        if !custom.is_empty() {
            push(PathBuf::from(custom), "custom");
        }
        found
    }

    pub(super) async fn core_report(&mut self, with_releases: bool) -> Result<Reply> {
        let selected = self.core();
        let mut report = CoreReport {
            running: self.connected(),
            tested: CORE_VERSION.into(),
            ..Default::default()
        };
        for (path, source) in self.core_candidates() {
            let version = core_version(&path)
                .await
                .unwrap_or_else(|_| "unreadable".into());
            report.installed.push(CoreInfo {
                selected: selected.as_ref() == Some(&path),
                supported: supported_version(&version),
                path: path.display().to_string(),
                version,
                source: source.into(),
            });
        }
        if with_releases {
            match releases().await {
                Ok(list) => {
                    for r in list {
                        let version = r["tag_name"]
                            .as_str()
                            .unwrap_or("")
                            .trim_start_matches('v')
                            .to_string();
                        if !valid_version(&version) || !supported_version(&version) {
                            continue;
                        }
                        report.releases.push(Release {
                            installed: self
                                .dir
                                .join("cores")
                                .join(&version)
                                .join("sing-box")
                                .is_file()
                                || (version == CORE_VERSION
                                    && self.dir.join("bin/sing-box").is_file()),
                            prerelease: r["prerelease"].as_bool().unwrap_or(false),
                            published: r["published_at"]
                                .as_str()
                                .unwrap_or("")
                                .chars()
                                .take(10)
                                .collect(),
                            version,
                        });
                    }
                }
                Err(e) => report.releases_error = model::clean(&format!("{e:#}")),
            }
        }
        let mut reply = Reply::success("Core inventory refreshed");
        reply.cores = Some(report);
        Ok(reply)
    }

    pub(super) async fn install_core_version(&mut self, version: String) -> Result<Reply> {
        ensure!(valid_version(&version), "Invalid version");
        ensure!(
            supported_version(&version),
            "sing needs sing-box 1.14 or newer"
        );
        let (os, arch) = platform()?;
        let asset = format!("sing-box-{version}-{os}-{arch}.tar.gz");
        let list = releases().await?;
        let release = list
            .iter()
            .find(|r| r["tag_name"].as_str() == Some(&format!("v{version}")))
            .context("Release not found in the recent official list")?;
        let item = release["assets"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|a| a["name"].as_str() == Some(&asset))
            .with_context(|| format!("No {asset} in this release"))?;
        let digest = item["digest"]
            .as_str()
            .and_then(|d| d.strip_prefix("sha256:"))
            .context("This release has no published SHA-256 digest; not installing")?
            .to_lowercase();
        let url = item["browser_download_url"]
            .as_str()
            .context("Missing download URL")?;
        ensure!(
            url.starts_with("https://github.com/SagerNet/sing-box/releases/download/"),
            "Unexpected download location"
        );
        let mut response = client()?.get(url).send().await?.error_for_status()?;
        let mut data = vec![];
        while let Some(chunk) = response.chunk().await? {
            ensure!(
                data.len() + chunk.len() < 150 * 1024 * 1024,
                "Core archive too large"
            );
            data.extend_from_slice(&chunk);
        }
        ensure!(
            format!("{:x}", Sha256::digest(&data)) == digest,
            "Checksum mismatch; installation aborted"
        );
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(&data[..]));
        let target = self.dir.join("cores").join(&version).join("sing-box");
        let mut installed = false;
        for item in archive.entries()? {
            let mut item = item?;
            if item.header().entry_type().is_file()
                && item.path()?.file_name().is_some_and(|n| n == "sing-box")
            {
                let mut binary = vec![];
                item.by_ref()
                    .take(200 * 1024 * 1024)
                    .read_to_end(&mut binary)?;
                model::atomic_write(&target, &binary)?;
                fs::set_permissions(&target, fs::Permissions::from_mode(0o700))?;
                installed = true;
                break;
            }
        }
        ensure!(installed, "Executable missing in the official archive");
        self.log(format!("Installed sing-box {version}; SHA-256 verified"));
        if self.connected() {
            return Ok(Reply::success(format!(
                "sing-box {version} installed. Select it, then restart to use it."
            )));
        }
        self.select_core(target.display().to_string()).await
    }

    pub(super) async fn select_core(&mut self, path: String) -> Result<Reply> {
        let file = PathBuf::from(&path);
        ensure!(file.is_file(), "Core file not found");
        let version = core_version(&file).await?;
        ensure!(
            supported_version(&version),
            "sing needs sing-box 1.14 or newer; this is {version}"
        );
        let mut store = self.store.clone();
        store.settings.core = path;
        self.save(store)?;
        if !self.connected() {
            self.version = version.clone();
        }
        let mut reply = if self.connected() {
            Reply::success(format!("{version} selected. Restart the core to switch."))
        } else {
            Reply::success(format!("{version} selected"))
        };
        reply.cores = self.core_report(false).await?.cores;
        Ok(reply)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn versions_are_path_safe() {
        assert!(super::valid_version("1.14.0-beta.2"));
        assert!(!super::valid_version("../1.14"));
        assert!(!super::valid_version(""));
    }
}
