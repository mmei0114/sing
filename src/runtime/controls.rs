//! The always-visible controls: mode, Global target, system proxy and TUN.
//! Mode and system proxy act on a running core immediately; TUN changes the
//! saved inbounds and asks for an explicit restart.
use super::*;

impl Manager {
    /// Persist a settings change to both the saved and the running store so a
    /// live switch never shows up as an unapplied draft.
    fn live_settings(&mut self, change: impl Fn(&mut Settings)) -> Result<()> {
        let mut store = self.store.clone();
        change(&mut store.settings);
        self.save(store)?;
        if let Some(running) = &mut self.running {
            change(&mut running.settings);
        }
        Ok(())
    }

    pub(super) async fn set_mode(&mut self, mode: String) -> Result<Reply> {
        ensure!(
            ["rule", "global", "direct"].contains(&mode.as_str()),
            "Mode must be Rule, Global or Direct"
        );
        let live = self.connected();
        if live {
            ensure!(
                self.running.as_ref().is_some_and(|r| r.native.is_some()),
                "Apply the native configuration once before switching modes live"
            );
            let mut api = self.api().await?;
            api.set_mode(mode.clone()).await.context(
                "The running core does not support live modes yet. Apply once to enable them.",
            )?;
            let current = api.mode_status().await?.current;
            ensure!(
                current.eq_ignore_ascii_case(&mode),
                "Mode switch was sent but the core reports {current}"
            );
        }
        self.live_settings(|s| s.route_mode = mode.clone())?;
        self.connectivity = ProbeStatus::default();
        let name = match mode.as_str() {
            "global" => "Global",
            "direct" => "Direct",
            _ => "Rule",
        };
        self.log(format!("Mode set to {name}"));
        Ok(Reply::success(if live {
            format!("{name} mode is active. New connections follow it; open ones keep their route.")
        } else {
            format!("{name} mode saved; it is used when the core starts.")
        }))
    }

    pub(super) async fn set_global_target(&mut self, target: String) -> Result<Reply> {
        let doc = self
            .store
            .native
            .as_ref()
            .context("Initialize native configuration first")?;
        ensure!(
            ["/outbounds", "/endpoints"]
                .iter()
                .any(|p| native::array(doc, p)
                    .iter()
                    .any(|v| native::tag(v) == target)),
            "Global target no longer exists"
        );
        let live =
            self.connected()
                && self.groups.group.iter().any(|g| {
                    g.tag == native::GLOBAL_TAG && g.items.iter().any(|i| i.tag == target)
                });
        if live {
            self.groups = self
                .api()
                .await?
                .select_confirmed(native::GLOBAL_TAG.into(), target.clone())
                .await?;
        }
        self.live_settings(|s| s.global_target = target.clone())?;
        Ok(Reply::success(if live {
            "Global target switched live"
        } else {
            "Global target saved; it is loaded on the next start"
        }))
    }

    pub(super) fn set_system_proxy(&mut self, on: bool) -> Result<Reply> {
        ensure!(
            cfg!(target_os = "macos"),
            "System proxy integration is available on macOS only"
        );
        ensure!(
            !on || std::env::var_os("SSH_CONNECTION").is_none(),
            "System proxy is disabled over SSH; it would change the remote host"
        );
        let mut next = self.store.clone();
        next.settings.mode = if on { "system" } else { "port" }.into();
        if on {
            // Same requirement Apply enforces: an unauthenticated loopback mixed inbound.
            native::effective(&next)?;
        }
        let live = self.connected();
        if live && on {
            if proxy_helper::query(&self.dir, proxy_helper::Request::Status).is_err() {
                let mut r = Reply::success("Authorize macOS system proxy management");
                r.needs_auth = true;
                r.auth_kind = "system".into();
                r.after_auth = Some(Action::SetSystemProxy(true));
                return Ok(r);
            }
            let port = self
                .running
                .as_ref()
                .map(|r| r.settings.port)
                .unwrap_or(next.settings.port);
            let running_ok = self.running.as_ref().is_some_and(|r| {
                let mut r = r.clone();
                r.settings.mode = "system".into();
                native::effective(&r).is_ok()
            });
            ensure!(
                running_ok,
                "The running core has no suitable mixed inbound. Apply the draft first."
            );
            self.lease.set(true);
            let status = proxy_helper::enable(&self.dir, port)?;
            self.log(status.detail);
        } else if live && self.dir.join(proxy_helper::MARKER).exists() {
            let status = proxy_helper::restore(&self.dir)?;
            self.lease.set(false);
            self.log(status.detail);
        }
        self.live_settings(|s| s.mode = next.settings.mode.clone())?;
        Ok(Reply::success(match (on, live) {
            (true, true) => "System proxy on. Apps that honor macOS proxy settings now use sing.",
            (false, true) => "System proxy off. Original macOS proxy settings restored.",
            (true, false) => "System proxy will be turned on when the core starts.",
            (false, false) => "System proxy will stay off.",
        }))
    }

    pub(super) fn set_tun(&mut self, enabled: bool, revision: String) -> Result<Reply> {
        ensure!(
            revision == native::revision(&self.store),
            "Draft changed; try the TUN switch again"
        );
        let mut next = self.store.clone();
        let doc = next
            .native
            .as_mut()
            .context("Initialize native configuration first")?;
        let inbounds = doc
            .as_object_mut()
            .context("Native configuration must be a JSON object")?
            .entry("inbounds")
            .or_insert(serde_json::json!([]))
            .as_array_mut()
            .context("inbounds must be an array")?;
        let has_tun = inbounds.iter().any(|i| i["type"] == "tun");
        if enabled == has_tun {
            return Ok(Reply::success(if enabled {
                "TUN is already configured"
            } else {
                "TUN is already off"
            }));
        }
        if enabled {
            let tun = next
                .settings
                .parked_tun
                .take()
                .unwrap_or_else(native::tun_template);
            ensure!(
                !inbounds.iter().any(|i| native::tag(i) == native::tag(&tun)),
                "Inbound tag {} is already used; rename it first",
                native::tag(&tun)
            );
            inbounds.push(tun);
        } else {
            let position = inbounds.iter().position(|i| i["type"] == "tun").unwrap();
            let tun = inbounds.remove(position);
            ensure!(
                !inbounds.iter().any(|i| i["type"] == "tun"),
                "Several TUN inbounds are configured; edit them in Config → Inbounds"
            );
            let before = self.store.native.as_ref().unwrap();
            let mut after = before.clone();
            after["inbounds"] = serde_json::json!(inbounds);
            native::links::check_removals(before, &after)?;
            next.settings.parked_tun = Some(tun);
        }
        self.save(next)?;
        let running = self.connected();
        let mut reply = Reply::success(match (enabled, running) {
            (true, true) => "TUN saved. Restart the core to capture all traffic.",
            (false, true) => "TUN turned off in the draft. Restart the core to release it.",
            (true, false) => {
                "TUN will be used when the core starts (administrator access required)."
            }
            (false, false) => "TUN turned off.",
        });
        if running {
            reply.confirm = Some(Action::ApplyNative {
                revision: native::revision(&self.store),
            });
        }
        Ok(reply)
    }
}
