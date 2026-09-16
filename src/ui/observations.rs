//! Shared, read-only connection observations. Never infer paths from current groups.
use super::*;

pub(super) fn destination(c: &api::Connection) -> String {
    model::clean(if c.domain.is_empty() {
        &c.destination
    } else {
        &c.domain
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> App {
        let mut a = App::new(sample().unwrap(), true);
        a.snapshot.connected = true;
        a.snapshot.api_ready = true;
        a
    }
    fn connection(id: &str) -> api::Connection {
        api::Connection {
            id: id.into(),
            domain: format!("{id}.example.invalid"),
            outbound: "g".into(),
            chain: vec!["g".into(), "old-node".into()],
            ..Default::default()
        }
    }
    fn report(ids: &[&str]) -> runtime::ConnectionReport {
        runtime::ConnectionReport {
            observed_at: model::now(),
            total: ids.len(),
            items: ids.iter().map(|id| connection(id)).collect(),
        }
    }
    fn press(a: &mut App, key: K) -> Option<Action> {
        a.key(KeyEvent::new(key, M::NONE)).unwrap()
    }
    #[test]
    fn shared_connections_pause_while_browsing_and_keep_id_on_refresh() {
        let mut a = app();
        a.observe_connections(report(&["a", "b", "c"]));
        press(&mut a, K::Char('l'));
        press(&mut a, K::Down);
        assert_eq!(a.connection_rows()[a.selected[7]].id, "b");
        a.observe_connections(report(&["new", "a", "b", "c"]));
        assert_eq!(a.connection_rows()[a.selected[7]].id, "b");
        assert_eq!(a.connections.items.len(), 3);
        assert!(a.connection_status().contains("Paused"));
        press(&mut a, K::Enter);
        let Some(Dialog::Text { text, .. }) = &a.dialog else {
            panic!()
        };
        assert!(text.contains("b.example.invalid"));
        press(&mut a, K::Esc);
        press(&mut a, K::Esc);
        assert_eq!(a.focus, Focus::Content);
        assert_eq!(a.connections.items.len(), 4);
        assert_eq!(a.connection_rows()[a.selected[7]].id, "b");
        a.go(7, 0);
        assert_eq!(a.rows().len(), 4);
    }
    #[test]
    fn refreshed_selection_never_moves_to_a_different_connection_by_index() {
        let mut a = app();
        a.observe_connections(report(&["a", "b"]));
        a.selected[7] = 1;
        a.observe_connections(report(&["new", "a", "b"]));
        assert_eq!(a.selected[7], 2);
        a.observe_connections(report(&["only"]));
        assert_eq!(a.selected[7], 0);
    }
    #[test]
    fn poll_replies_preserve_inflight_editor_transaction_and_never_show_notices() {
        let mut a = app();
        a.import(false);
        a.retry = a.dialog.clone();
        a.intent = Some(Intent::Setup);
        a.busy = true;
        a.notice = "Keep this important message".into();
        let reply:Reply=serde_json::from_value(json!({"ok":true,"message":"Connections refreshed","needs_auth":false,"connections":report(&["one"]),"snapshot":a.snapshot})).unwrap();
        a.receive_poll(Ok(reply));
        assert!(a.busy);
        assert!(a.retry.is_some() && a.dialog.is_some());
        assert!(matches!(a.intent, Some(Intent::Setup)));
        assert_eq!(a.notice, "Keep this important message");
        a.receive_poll(Err(anyhow::anyhow!("Refresh failed")));
        assert!(a.busy && a.retry.is_some() && a.dialog.is_some());
        assert_eq!(a.connections.items.len(), 1);
        assert!(a.connection_status().contains("Refresh failed"));
    }
    #[test]
    fn observation_browsing_does_not_trigger_group_selection_or_delete() {
        let mut a = app();
        a.observe_connections(report(&["a", "b"]));
        press(&mut a, K::Char('l'));
        let group_selection = a.selected[0];
        press(&mut a, K::Down);
        assert_eq!(a.selected[0], group_selection);
        assert!(press(&mut a, K::Char('x')).is_none());
        assert!(a.dialog.is_none());
        press(&mut a, K::Enter);
        assert!(matches!(a.dialog, Some(Dialog::Text { action: None, .. })));
        press(&mut a, K::Esc);
        press(&mut a, K::Char('r'));
        assert!(!a.connections_paused);
        press(&mut a, K::F(6));
        assert_eq!(a.focus, Focus::Global);
    }
    #[test]
    fn historical_path_is_not_reconstructed_from_current_selection() {
        let mut a = app();
        let c = connection("one");
        a.snapshot.groups.group.push(api::Group {
            tag: "g".into(),
            selected: "new-node".into(),
            ..Default::default()
        });
        assert_eq!(path(&a, &c), "g → old-node");
        assert!(!details(&a, &c).contains("new-node"));
        assert!(details(&a, &c).contains("Rule         Unavailable"));
    }
    #[test]
    fn stop_clears_held_connections_and_late_reports_cannot_repopulate_them() {
        let mut a = app();
        a.observe_connections(report(&["one"]));
        a.connections_paused = true;
        a.observe_connections(report(&["two"]));
        let mut stopped = a.snapshot.clone();
        stopped.connected = false;
        a.observe_snapshot(stopped);
        a.observe_connections(report(&["late"]));
        assert!(a.connections.items.is_empty() && a.pending_connections.is_none());
        assert_eq!(a.connection_status(), "Core stopped");
    }
    #[test]
    fn check_becomes_historical_when_selection_changes_but_not_when_latency_changes() {
        let mut a = app();
        a.snapshot.connectivity.checked_at = 10;
        a.snapshot.groups.group.push(api::Group {
            tag: "g".into(),
            selected: "old".into(),
            ..Default::default()
        });
        let mut next = a.snapshot.clone();
        next.groups.group[0].selected = "new".into();
        a.observe_snapshot(next.clone());
        assert!(a.probe_stale);
        next.connectivity.checked_at = 11;
        a.observe_snapshot(next.clone());
        assert!(!a.probe_stale);
        a.observe_snapshot(next);
        assert!(!a.probe_stale);
    }
}
pub(super) fn path(a: &App, c: &api::Connection) -> String {
    // Preserve the core's reported chain order. No reconstruction from the draft.
    if c.chain.is_empty() {
        if c.outbound.is_empty() {
            "Unknown".into()
        } else {
            model::clean(&a.label(&c.outbound))
        }
    } else {
        c.chain
            .iter()
            .map(|tag| model::clean(&a.label(tag)))
            .collect::<Vec<_>>()
            .join(" → ")
    }
}
pub(super) fn details(a: &App, c: &api::Connection) -> String {
    let value = |s: &str| {
        if s.is_empty() {
            "Unavailable".into()
        } else {
            model::clean(s)
        }
    };
    format!("Destination  {}\nDomain       {}\nSource       {}\nNetwork      {}\nProtocol     {}\nInbound      {}\nOutbound     {}\nCore chain   {}\nRule         {}\nProcess      {}\nCreated      {} (core timestamp)\nState        {}\nUploaded     {} bytes\nDownloaded   {} bytes\n\nObserved connection, not a complete URL or browsing history.",
        value(&c.destination), value(&c.domain), value(&c.source), value(&c.network), value(&c.protocol), value(&c.inbound), value(&a.label(&c.outbound)), path(a,c), value(&c.rule), value(c.process.as_ref().map(|p|p.path.as_str()).unwrap_or("")), c.created_at,
        if c.closed_at==0 { "Open at sample time" } else { "Closed" }, c.uplink_total, c.downlink_total)
}

impl App {
    pub(super) fn onboarding(&self) -> bool {
        let doc = self.doc();
        self.snapshot.store.native.is_none()
            || (native::array(&doc, "/inbounds").is_empty()
                && native::array(&doc, "/endpoints").is_empty())
            || (native::array(&doc, "/outbounds").is_empty()
                && native::array(&doc, "/endpoints").is_empty())
    }
    pub(super) fn connection_rows(&self) -> Vec<&api::Connection> {
        let query = if self.page == 7 {
            self.filter.value.to_lowercase()
        } else {
            String::new()
        };
        self.connections
            .items
            .iter()
            .filter(|c| c.closed_at == 0 || (self.page == 7 && self.show_closed))
            .filter(|c| {
                query.is_empty()
                    || format!(
                        "{} {} {} {}",
                        destination(c),
                        path(self, c),
                        c.rule,
                        c.process.as_ref().map(|p| p.path.as_str()).unwrap_or("")
                    )
                    .to_lowercase()
                    .contains(&query)
            })
            .collect()
    }
    pub(super) fn observe_snapshot(&mut self, snapshot: Snapshot) {
        if snapshot.connectivity.checked_at != self.snapshot.connectivity.checked_at
            || snapshot.connectivity.checked_at == 0
        {
            self.probe_stale = false;
        } else if snapshot.connected != self.snapshot.connected
            || snapshot.running_settings != self.snapshot.running_settings
            || snapshot
                .groups
                .group
                .iter()
                .map(|g| (&g.tag, &g.selected))
                .collect::<Vec<_>>()
                != self
                    .snapshot
                    .groups
                    .group
                    .iter()
                    .map(|g| (&g.tag, &g.selected))
                    .collect::<Vec<_>>()
        {
            self.probe_stale = true;
        }
        self.snapshot = snapshot;
        self.snapshot_at = model::now();
        if !self.snapshot.connected {
            self.connections = Default::default();
            self.pending_connections = None;
            self.connections_paused = false;
            self.connection_error.clear();
            self.selected[7] = 0;
        }
    }
    pub(super) fn observe_connections(&mut self, report: runtime::ConnectionReport) {
        if !self.snapshot.connected {
            return;
        }
        self.connection_error.clear();
        if self.connections_paused && self.connections.observed_at != 0 {
            self.pending_connections = Some(report);
            return;
        }
        let selected = self
            .connection_rows()
            .get(self.selected[7])
            .map(|c| c.id.clone());
        self.connections = report;
        self.selected[7] = selected
            .and_then(|id| self.connection_rows().iter().position(|c| c.id == id))
            .unwrap_or(0);
    }
    pub(super) fn resume_connections(&mut self) {
        self.connections_paused = false;
        if let Some(report) = self.pending_connections.take() {
            self.observe_connections(report);
        }
    }
    // Background observations must not clear a foreground transaction's busy,
    // retry, intent, notice, or editor, including when its operation is queued.
    pub(super) fn receive_poll(&mut self, result: Result<Reply>) {
        match result {
            Ok(reply) => {
                if let Some(snapshot) = reply.snapshot {
                    self.observe_snapshot(snapshot);
                }
                if reply.ok {
                    if let Some(report) = reply.connections {
                        self.observe_connections(report);
                    }
                } else {
                    self.connection_error = model::clean(&reply.message);
                }
            }
            Err(error) => self.connection_error = model::clean(&error.to_string()),
        }
    }
    pub(super) fn connection_status(&self) -> String {
        if !self.snapshot.connected {
            return "Core stopped".into();
        }
        if !self.snapshot.api_ready {
            return "API unavailable".into();
        }
        if !self.connection_error.is_empty() {
            return "Refresh failed · last sample".into();
        }
        if self.connections.observed_at == 0 {
            return "Waiting for sample".into();
        }
        let age = model::now().saturating_sub(self.connections.observed_at);
        if self.connections_paused {
            format!("Paused · {age}s ago · r Live")
        } else if age > 6 {
            format!("Stale · {age}s ago")
        } else {
            format!("Sampled {age}s ago")
        }
    }
    pub(super) fn observation_key(&mut self, key: KeyEvent) -> Option<Option<Action>> {
        if self.page == 0 && key.code == K::Char('l') {
            self.focus = Focus::Connections;
            return Some(None);
        }
        let browsing = self.page == 7 && self.focus == Focus::Content
            || self.page == 0 && self.focus == Focus::Connections;
        if !browsing {
            return None;
        }
        match key.code {
            K::Up | K::Down | K::Char('j' | 'k') | K::PageUp | K::PageDown => {
                self.connections_paused = self.connections.observed_at != 0;
                let step = if matches!(key.code, K::PageUp | K::PageDown) {
                    5
                } else {
                    1
                };
                self.selected[7] = if matches!(key.code, K::Up | K::Char('k') | K::PageUp) {
                    self.selected[7].saturating_sub(step)
                } else {
                    (self.selected[7] + step).min(self.connection_rows().len().saturating_sub(1))
                };
                Some(None)
            }
            K::Enter => {
                self.connections_paused = self.connections.observed_at != 0;
                if let Some(c) = self.connection_rows().get(self.selected[7]) {
                    self.note("Connection · observed details", details(self, c), None);
                }
                Some(None)
            }
            K::Char('r') => {
                self.resume_connections();
                self.selected[7] = 0;
                Some(Some(Action::Connections))
            }
            K::Char('/') if self.page == 0 => {
                let action = self.go(7, 0);
                self.focus = Focus::Content;
                self.searching = true;
                Some(action)
            }
            K::Esc if self.page == 0 => {
                self.resume_connections();
                self.focus = Focus::Content;
                Some(None)
            }
            K::Tab | K::BackTab | K::F(6) | K::Char('1'..='5' | '[' | ']' | ',') => {
                self.resume_connections();
                None
            }
            _ => None,
        }
    }
}
