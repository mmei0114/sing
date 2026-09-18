//! Session connection history. The core reports a live sample; sing keeps what
//! it has seen so closed requests, per-app totals and hosts remain visible.
use super::text;
use crate::{api, runtime::ConnectionReport};
use std::collections::{BTreeMap, HashMap};

const LIMIT: usize = 5000;

#[derive(Clone, Debug)]
pub struct Entry {
    pub c: api::Connection,
    pub open: bool,
}
impl Entry {
    pub fn host(&self) -> String {
        host(&self.c)
    }
    pub fn app(&self) -> String {
        app_name(&self.c)
    }
    pub fn created(&self) -> u64 {
        text::epoch_seconds(self.c.created_at)
    }
    pub fn total(&self) -> i64 {
        self.c.uplink_total + self.c.downlink_total
    }
}
pub fn host(c: &api::Connection) -> String {
    if !c.domain.is_empty() {
        return crate::model::clean(&c.domain);
    }
    let d = &c.destination;
    // Strip the port from host:port and [v6]:port.
    let h = if let Some(rest) = d.strip_prefix('[') {
        rest.split(']').next().unwrap_or(rest)
    } else if d.matches(':').count() == 1 {
        d.split(':').next().unwrap_or(d)
    } else {
        d
    };
    crate::model::clean(h)
}
pub fn port(c: &api::Connection) -> String {
    c.destination
        .rsplit_once(':')
        .map(|(_, p)| p.to_string())
        .unwrap_or_default()
}
pub fn app_name(c: &api::Connection) -> String {
    super::identity::display(c)
}

#[derive(Default, Clone)]
pub struct History {
    pub entries: Vec<Entry>,
    index: HashMap<String, usize>,
    pub observed_at: u64,
    pub total_live: usize,
}

#[derive(Clone, Debug, Default)]
pub struct AppStat {
    pub key: String,
    pub name: String,
    pub path: String,
    pub connections: usize,
    pub open: usize,
    pub up: i64,
    pub down: i64,
    pub last: u64,
    pub targets: BTreeMap<String, usize>,
}
#[derive(Clone, Debug, Default)]
pub struct HostStat {
    pub host: String,
    pub connections: usize,
    pub open: usize,
    pub traffic: i64,
    pub last: u64,
    pub target: String,
    pub rule: String,
    /// A representative connection for rule suggestions.
    pub sample: Option<api::Connection>,
}

impl History {
    pub fn observe(&mut self, report: ConnectionReport, connected: bool) {
        self.observed_at = report.observed_at;
        self.total_live = report.total;
        for e in &mut self.entries {
            e.open = false;
        }
        if !connected {
            return;
        }
        for c in report.items {
            let open = c.closed_at == 0;
            match self.index.get(&c.id) {
                Some(&i) => {
                    self.entries[i].c = c;
                    self.entries[i].open = open;
                }
                None => {
                    self.index.insert(c.id.clone(), self.entries.len());
                    self.entries.push(Entry { c, open });
                }
            }
        }
        if self.entries.len() > LIMIT {
            self.entries
                .sort_by_key(|e| std::cmp::Reverse((e.open, e.c.created_at)));
            self.entries.truncate(LIMIT);
        }
        self.entries.sort_by(|a, b| {
            b.c.created_at
                .cmp(&a.c.created_at)
                .then(a.c.id.cmp(&b.c.id))
        });
        self.index = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.c.id.clone(), i))
            .collect();
    }
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn apps(&self) -> Vec<AppStat> {
        let mut map: BTreeMap<String, AppStat> = BTreeMap::new();
        for e in &self.entries {
            let name = e.app();
            let key = super::identity::key(&e.c);
            let stat = map.entry(key.clone()).or_insert_with(|| AppStat {
                key,
                name: name.clone(),
                path: e
                    .c
                    .process
                    .as_ref()
                    .map(|p| crate::model::clean(&p.path))
                    .unwrap_or_default(),
                ..Default::default()
            });
            stat.connections += 1;
            stat.open += usize::from(e.open);
            stat.up += e.c.uplink_total;
            stat.down += e.c.downlink_total;
            stat.last = stat.last.max(e.created());
            *stat.targets.entry(route_target(&e.c)).or_default() += 1;
        }
        map.into_values().collect()
    }
    pub fn hosts(&self, app: Option<&str>) -> Vec<HostStat> {
        let mut map: BTreeMap<String, HostStat> = BTreeMap::new();
        for e in self
            .entries
            .iter()
            .filter(|e| app.is_none_or(|a| super::identity::key(&e.c) == a))
        {
            let host = e.host();
            let stat = map.entry(host.clone()).or_insert_with(|| HostStat {
                host: host.clone(),
                ..Default::default()
            });
            stat.connections += 1;
            stat.open += usize::from(e.open);
            stat.traffic += e.total();
            if e.created() >= stat.last {
                stat.last = e.created();
                stat.target = route_target(&e.c);
                stat.rule = e.c.rule.clone();
                stat.sample = Some(e.c.clone());
            }
        }
        let mut hosts: Vec<_> = map.into_values().collect();
        hosts.sort_by(|a, b| b.last.cmp(&a.last).then(b.traffic.cmp(&a.traffic)));
        hosts
    }
}
/// The outbound the matched rule chose (a group comes before its member).
pub fn route_target(c: &api::Connection) -> String {
    c.chain
        .last()
        .cloned()
        .unwrap_or_else(|| c.outbound.clone())
}

pub fn route_path(c: &api::Connection, label: impl Fn(&str) -> String) -> String {
    if c.chain.is_empty() {
        if c.outbound.is_empty() {
            "Not reported".into()
        } else {
            label(&c.outbound)
        }
    } else {
        c.chain
            .iter()
            .rev()
            .map(|t| label(t))
            .collect::<Vec<_>>()
            .join(" → ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn conn(id: &str, app: &str, host: &str, created: i64, open: bool) -> api::Connection {
        api::Connection {
            id: id.into(),
            domain: host.into(),
            destination: format!("{host}:443"),
            created_at: created,
            closed_at: if open { 0 } else { created + 1 },
            uplink_total: 10,
            downlink_total: 90,
            outbound: "proxy".into(),
            process: Some(api::ProcessInfo {
                path: format!("/Applications/{app}.app/Contents/MacOS/{app}"),
                ..Default::default()
            }),
            ..Default::default()
        }
    }
    #[test]
    fn closed_connections_stay_and_apps_aggregate() {
        let mut h = History::default();
        h.observe(
            ConnectionReport {
                observed_at: 1,
                total: 2,
                items: vec![
                    conn("a", "Safari", "a.example", 10, true),
                    conn("b", "Safari", "b.example", 20, true),
                ],
            },
            true,
        );
        h.observe(
            ConnectionReport {
                observed_at: 2,
                total: 1,
                items: vec![conn("c", "Mail", "a.example", 30, true)],
            },
            true,
        );
        assert_eq!(h.entries.len(), 3);
        assert_eq!(h.entries[0].c.id, "c");
        assert!(!h.entries[1].open);
        let apps = h.apps();
        let safari = apps.iter().find(|a| a.name == "Safari").unwrap();
        assert_eq!((safari.connections, safari.open, safari.down), (2, 0, 180));
        assert_eq!(h.hosts(Some(&safari.key)).len(), 2);
        assert_eq!(
            h.hosts(None)
                .iter()
                .find(|x| x.host == "a.example")
                .unwrap()
                .connections,
            2
        );
    }
    #[test]
    fn hosts_strip_ports() {
        let mut c = conn("x", "curl", "", 1, true);
        c.destination = "[2001:db8::1]:443".into();
        assert_eq!(host(&c), "2001:db8::1");
        c.destination = "192.0.2.1:80".into();
        assert_eq!(host(&c), "192.0.2.1");
    }
    #[test]
    fn chain_is_reversed_for_policy_to_node_display() {
        let c = api::Connection {
            chain: vec!["node".into(), "HK".into(), "proxy".into()],
            outbound: "node".into(),
            ..Default::default()
        };
        assert_eq!(route_target(&c), "proxy");
        assert_eq!(route_path(&c, str::to_string), "proxy → HK → node");
    }
    #[test]
    fn same_name_in_different_paths_does_not_merge_rule_evidence() {
        let a = conn("a", "Browser", "a.example", 10, true);
        let mut b = a.clone();
        b.id = "b".into();
        b.process.as_mut().unwrap().path = "/tmp/Browser".into();
        let mut h = History::default();
        h.observe(
            ConnectionReport {
                observed_at: 1,
                total: 2,
                items: vec![a, b],
            },
            true,
        );
        let apps = h.apps();
        assert_eq!(apps.len(), 2);
        for app in &apps {
            assert_eq!(h.hosts(Some(&app.key))[0].connections, 1);
        }
    }
}
