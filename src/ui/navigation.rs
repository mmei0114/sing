//! Product navigation maps onto the existing native editors, never onto copies of data.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Focus {
    Global,
    Actions,
    Content,
    Connections,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Command {
    Key(char),
    Open(usize, usize),
    Import,
    Setup,
    UpdateAll,
    Convert,
    Group,
    Details,
    References,
    Back,
    Select,
    Tun,
    Actions,
    Observe,
}
pub(super) type Button = (&'static str, Command);
#[derive(Clone)]
pub(super) struct PaletteItem {
    pub section: &'static str,
    pub label: &'static str,
    pub command: Command,
}

impl App {
    // Editor IDs are internal; the five workspaces are the public navigation model.
    pub(super) fn workspace(&self) -> usize {
        match self.page {
            0 => 0,
            2 => 1,
            5 if self.tabs[5] == 0 => 1,
            3 | 5 => 2,
            1 | 4 | 11 => 3,
            7..=9 => 4,
            _ => 5, // Global Settings, not a sixth primary workspace.
        }
    }
    pub(super) fn destinations(&self) -> Vec<(&'static str, usize, usize)> {
        match self.workspace() {
            1 => vec![
                ("Proxy Groups", 2, 0),
                ("Nodes", 2, 1),
                ("Subscriptions", 5, 0),
            ],
            2 => vec![("Rules", 3, 0), ("Rule Sets", 5, 1)],
            3 => vec![("Capture", 11, 0), ("Inbounds", 1, 0), ("DNS", 4, 0)],
            4 => vec![("Connections", 7, 0), ("Logs", 8, 0), ("Diagnostics", 9, 0)],
            5 => vec![
                ("Core", 10, 0),
                ("Interface", 10, 1),
                ("Client", 10, 2),
                ("Advanced Tools", 6, 0),
            ],
            _ => vec![],
        }
    }
    pub(super) fn subtab(&self) -> usize {
        self.destinations()
            .iter()
            .position(|(_, page, tab)| {
                *page == self.page && (*page != 2 && *page != 10 || *tab == self.tabs[*page])
            })
            .unwrap_or(if self.page == 2 { 1 } else { 0 })
    }
    pub(super) fn go(&mut self, page: usize, tab: usize) -> Option<Action> {
        self.resume_connections();
        let previous = (self.page, self.tabs[self.page]);
        if previous != (page, tab) {
            self.locations.insert(
                previous,
                (self.selected[self.page], self.filter.value.clone()),
            );
            let (selected, filter) = self
                .locations
                .get(&(page, tab))
                .cloned()
                .unwrap_or_default();
            self.selected[page] = selected;
            self.filter = Input::new(filter);
        }
        self.page = page;
        self.tabs[page] = tab;
        self.selected[page] = self.selected[page].min(self.rows().len().saturating_sub(1));
        self.control = 0;
        self.refresh_requested = true;
        None
    }
    fn go_workspace(&mut self, workspace: usize) -> Option<Action> {
        let (page, tab) = [(0, 0), (2, 0), (3, 0), (11, 0), (7, 0)][workspace];
        self.go(page, tab)
    }
    pub(super) fn global_buttons(&self) -> Vec<Button> {
        vec![
            (
                if self.snapshot.connected {
                    "Stop"
                } else {
                    "Start"
                },
                Command::Key(if self.snapshot.connected { 'd' } else { 'c' }),
            ),
            ("Mode", Command::Key('M')),
            ("Settings", Command::Open(10, 0)),
            ("Review", Command::Key('A')),
            ("Help", Command::Key('?')),
            ("Actions", Command::Actions),
        ]
    }
    pub(super) fn buttons(&self) -> Vec<Button> {
        use Command::*;
        let mut buttons = match (self.page, self.tabs[self.page]) {
            (0, _) if self.onboarding() => {
                vec![("Import Subscription", Import), ("Connection Setup", Setup)]
            }
            (0, _) => vec![
                ("Test Connection", Key('v')),
                ("Capture Settings", Open(11, 0)),
                ("Diagnostics", Open(9, 0)),
            ],
            (2, 0) => vec![
                ("Import Subscription", Import),
                ("New Group", Group),
                ("Select Member", Select),
            ],
            (2, _) => vec![
                ("Import Subscription", Import),
                ("Test", Key('t')),
                ("Edit", Key('e')),
            ],
            (5, 0) => vec![
                ("Import Subscription", Import),
                ("Update", Key('r')),
                ("Update All", UpdateAll),
            ],
            (3, 0) => vec![
                ("Add Rule", Key('a')),
                ("Import Rule Set", Convert),
                ("Edit", Key('e')),
            ],
            (5, _) => vec![
                ("Import Rule Set", Convert),
                ("Edit", Key('e')),
                ("Update Converted", Key('r')),
            ],
            (11, _) => vec![
                ("System Proxy", Key('s')),
                ("Configure TUN", Tun),
                ("Restore System Proxy", Key('R')),
            ],
            (1, _) => vec![
                ("Add Inbound", Key('a')),
                ("Edit", Key('e')),
                ("Details", Details),
            ],
            (4, tab) => {
                if tab < 2 {
                    vec![
                        (
                            if tab == 0 {
                                "Add Server"
                            } else {
                                "Add DNS Rule"
                            },
                            Key('a'),
                        ),
                        ("Edit", Key('e')),
                        ("Details", Details),
                    ]
                } else {
                    vec![("Edit Options", Key('e')), ("Native JSON", Key('E'))]
                }
            }
            (7, _) => vec![
                ("Refresh", Key('r')),
                ("Details", Details),
                ("Include Closed", Key('h')),
            ],
            _ => self
                .all_buttons()
                .into_iter()
                .filter(|(_, c)| !matches!(c, References | Back))
                .collect(),
        };
        if self.page == 0 && self.snapshot.version.is_empty() {
            buttons.insert(0, ("Install Core", Key('i')));
        }
        if !self.history.is_empty() {
            buttons.insert(0, ("Back to References", Back));
        }
        buttons
    }
    pub(super) fn all_buttons(&self) -> Vec<Button> {
        use Command::*;
        let mut buttons = match (self.page, self.tabs[self.page]) {
            (0, _) => {
                let mut buttons = vec![
                    ("Import Subscription", Import),
                    ("Connection Setup", Setup),
                    ("Change Proxy", Open(2, 0)),
                    ("Capture", Open(11, 0)),
                    ("Test Connection", Key('v')),
                    ("Connections", Observe),
                    ("Diagnostics", Open(9, 0)),
                ];
                if self.snapshot.store.native.is_none() {
                    buttons.push(("Initialize Config", Key('u')));
                }
                buttons
            }
            (2, _) => vec![
                ("Import Subscription", Import),
                ("New Group", Group),
                ("Select Member", Select),
                ("Test", Key('t')),
                ("Edit", Key('e')),
                ("Details", Details),
                ("All Outbounds", Open(2, 2)),
                ("Add Outbound", Key('a')),
                ("Native JSON", Key('E')),
                ("Remove", Key('x')),
            ],
            (5, 0) => vec![
                ("Import Subscription", Import),
                ("Update", Key('r')),
                ("Update All", UpdateAll),
                ("Details", Details),
                ("Remove", Key('x')),
            ],
            (3, 0) => vec![
                ("Add Rule", Key('a')),
                ("Import Rule Set", Convert),
                ("Edit", Key('e')),
                ("Details", Details),
                ("Move Up", Key('K')),
                ("Move Down", Key('J')),
                ("Options", Open(3, 1)),
                ("Native JSON", Key('E')),
                ("Remove", Key('x')),
            ],
            (3, _) => vec![
                ("Edit Options", Key('e')),
                ("Native JSON", Key('E')),
                ("Back to Rules", Open(3, 0)),
            ],
            (5, _) => vec![
                ("Import Rule Set", Convert),
                ("Add Native Rule Set", Key('a')),
                ("Edit", Key('e')),
                ("Update Converted", Key('r')),
                ("Details", Details),
                ("Native JSON", Key('E')),
                ("Remove", Key('x')),
            ],
            (11, _) => vec![
                ("System Proxy", Key('s')),
                ("Configure TUN", Tun),
                ("Inbounds", Open(1, 0)),
                ("Restore System Proxy", Key('R')),
            ],
            (1, _) => vec![
                ("Add Inbound", Key('a')),
                ("Edit", Key('e')),
                ("Details", Details),
                ("Native JSON", Key('E')),
                ("Remove", Key('x')),
            ],
            (4, _) => {
                let mut b = vec![];
                if self.tabs[4] < 2 {
                    b.push((
                        if self.tabs[4] == 0 {
                            "Add Server"
                        } else {
                            "Add DNS Rule"
                        },
                        Key('a'),
                    ));
                }
                b.extend([
                    ("Edit", Key('e')),
                    ("Details", Details),
                    ("Native JSON", Key('E')),
                ]);
                if self.tabs[4] < 2 {
                    b.push(("Remove", Key('x')));
                }
                if self.tabs[4] == 1 {
                    b.extend([("Move Up", Key('K')), ("Move Down", Key('J'))]);
                }
                b.extend([
                    ("Servers", Open(4, 0)),
                    ("Rules", Open(4, 1)),
                    ("Options", Open(4, 2)),
                ]);
                b
            }
            (7, _) => vec![
                ("Refresh", Key('r')),
                ("Details", Details),
                ("Include Closed", Key('h')),
                ("Close Connection", Key('x')),
            ],
            (8, _) => vec![("Read Core Logs", Key('r'))],
            (9, _) => vec![
                ("Inspect Configuration", Key('r')),
                ("Core Check", Key('V')),
                ("Config Preview", Key('p')),
            ],
            (10, 0) => vec![("Edit Core Settings", Key('e')), ("Install Core", Key('i'))],
            (10, 2) => vec![
                ("Restore System Proxy", Key('R')),
                ("Config Preview", Key('p')),
            ],
            (6, _) => vec![
                ("Edit Section", Key('e')),
                ("Full Native JSON", Key('E')),
                ("Previous State", Key('b')),
                ("Core Check", Key('V')),
            ],
            _ => vec![],
        };
        if [1, 2, 3, 4, 6].contains(&self.page) || (self.page == 5 && self.tabs[5] == 1) {
            buttons.push(("References", References));
        }
        if !self.history.is_empty() {
            buttons.insert(0, ("Back to References", Back));
        }
        buttons
    }
    pub(super) fn activate(&mut self, command: Command) -> Result<Option<Action>> {
        if let Some(reason) = self.unavailable(command) {
            self.notice = reason.into();
            return Ok(None);
        }
        self.focus = Focus::Content;
        match command {
            Command::Observe => {
                self.focus = Focus::Connections;
                Ok(None)
            }
            Command::References => {
                self.reference_browser()?;
                Ok(None)
            }
            Command::Back => {
                self.back_to_reference();
                Ok(None)
            }
            Command::Key(c) => self.key(KeyEvent::new(K::Char(c), M::NONE)),
            Command::Open(page, tab) => Ok(self.go(page, tab)),
            Command::Import => {
                self.import(false);
                Ok(None)
            }
            Command::Setup => {
                self.intent = Some(Intent::Setup);
                Ok(Some(Action::ConnectionSetupInfo))
            }
            Command::UpdateAll => {
                self.note("Update all subscriptions?","Fetch every subscription, then review all changes before one Save. Nothing is applied automatically.".into(),Some(Action::RefreshAll));
                Ok(None)
            }
            Command::Convert => Ok(Some(self.open("/route".into(), Intent::RuleImport))),
            Command::Group => {
                self.go(2, 0);
                self.key(KeyEvent::new(K::Char('g'), M::NONE))
            }
            Command::Details => {
                if self.page == 7 {
                    if let Some(c) = self.connection_rows().get(self.selected[7]) {
                        self.note(
                            "Connection · observed details",
                            observations::details(self, c),
                            None,
                        );
                    }
                    return Ok(None);
                }
                if let Some((_, name, value)) = self.current() {
                    self.note(&name, view::details(self, &value), None);
                }
                Ok(None)
            }
            Command::Select => {
                // Enter must reach the selected object, not re-activate this
                // focused button recursively.
                self.focus = Focus::Content;
                self.key(KeyEvent::new(K::Enter, M::NONE))
            }
            Command::Actions => {
                let choices = self.palette();
                self.dialog = Some(Dialog::Commands {
                    choices,
                    query: Input::new(String::new()),
                    selected: 0,
                });
                Ok(None)
            }
            Command::Tun => {
                if native::array(&self.doc(), "/inbounds")
                    .iter()
                    .filter(|v| v["type"] == "tun")
                    .count()
                    > 1
                {
                    self.go(1, 0);
                    self.filter = Input::new("tun".into());
                    self.notice = "Multiple TUN inbounds. Select the one to configure.".into();
                    return Ok(None);
                }
                if let Some((i, _)) = native::array(&self.doc(), "/inbounds")
                    .iter()
                    .enumerate()
                    .find(|(_, v)| v["type"] == "tun")
                {
                    Ok(Some(self.open(format!("/inbounds/{i}"), Intent::Form)))
                } else {
                    self.go(1, 0);
                    let value = templates("/inbounds")
                        .into_iter()
                        .find(|(_, v)| v["type"] == "tun")
                        .context("TUN template unavailable")?
                        .1;
                    Ok(Some(self.open("/inbounds".into(), Intent::Add(value))))
                }
            }
        }
    }
    pub(super) fn palette(&self) -> Vec<PaletteItem> {
        use Command::*;
        let mut choices: Vec<PaletteItem> = vec![];
        for (section, buttons) in [
            ("This Page", self.all_buttons()),
            (
                "Go To",
                vec![
                    ("Overview", Open(0, 0)),
                    ("Subscriptions", Open(5, 0)),
                    ("Proxy Groups", Open(2, 0)),
                    ("Nodes", Open(2, 1)),
                    ("Rules", Open(3, 0)),
                    ("Rule Sets", Open(5, 1)),
                    ("Capture", Open(11, 0)),
                    ("Inbounds", Open(1, 0)),
                    ("DNS", Open(4, 0)),
                    ("All Connections", Open(7, 0)),
                    ("Logs", Open(8, 0)),
                    ("Diagnostics", Open(9, 0)),
                    ("Advanced Tools", Open(6, 0)),
                ],
            ),
            ("Application", {
                let mut b = self.global_buttons();
                b.extend([
                    ("Import Subscription", Import),
                    ("Connection Setup", Setup),
                    ("Quit Interface", Key('q')),
                ]);
                b
            }),
        ] {
            for (label, command) in buttons {
                if command != Actions && !choices.iter().any(|item| item.command == command) {
                    choices.push(PaletteItem {
                        section,
                        label,
                        command,
                    });
                }
            }
        }
        choices
    }
    pub(super) fn unavailable(&self, command: Command) -> Option<&'static str> {
        use Command::*;
        match command {
            Key('v' | 't') if !self.snapshot.connected => Some("Start the core first"),
            Key('r' | 'x') | Details
                if self.page == 7 && self.current().is_none() && command != Key('r') =>
            {
                Some("Select a connection")
            }
            Select
                if !self
                    .current()
                    .is_some_and(|(_, _, v)| v["type"] == "selector") =>
            {
                Some("Select a manual group")
            }
            Details | Key('e' | 'x' | 't' | 'J' | 'K')
                if [1, 2, 5].contains(&self.page) && self.current().is_none() =>
            {
                Some("Select an object")
            }
            Details | Key('e' | 'x' | 'J' | 'K')
                if ((self.page == 3 && self.tabs[3] == 0)
                    || (self.page == 4 && self.tabs[4] < 2))
                    && self.current().is_none() =>
            {
                Some("Select an object")
            }
            Key('r') if self.page == 5 && self.current().is_none() => Some("Select a resource"),
            UpdateAll if self.snapshot.store.subscriptions.is_empty() => Some("No subscriptions"),
            _ => None,
        }
    }
    // None means the key belongs to the content/editor, not navigation.
    pub(super) fn navigation_key(&mut self, key: KeyEvent) -> Result<Option<Option<Action>>> {
        if key.code == K::F(6) {
            self.focus = if self.focus == Focus::Global {
                Focus::Content
            } else {
                Focus::Global
            };
            self.control = 0;
            return Ok(Some(None));
        }
        if matches!(key.code, K::Tab | K::BackTab) {
            self.focus = if self.focus == Focus::Content && !self.buttons().is_empty() {
                Focus::Actions
            } else {
                Focus::Content
            };
            self.control = 0;
            return Ok(Some(None));
        }
        if let K::Char(c @ '1'..='5') = key.code {
            let action = self.go_workspace((c as u8 - b'1') as usize);
            self.focus = Focus::Content;
            return Ok(Some(action));
        }
        if key.code == K::Char(',') {
            self.focus = Focus::Content;
            return Ok(Some(self.go(10, 0)));
        }
        if key.code == K::Esc && self.focus != Focus::Content {
            self.focus = Focus::Content;
            return Ok(Some(None));
        }
        if self.focus == Focus::Content && self.page == 4 && matches!(key.code, K::Left | K::Right)
        {
            let tab = (self.tabs[4] + if key.code == K::Right { 1 } else { 2 }) % 3;
            return Ok(Some(self.go(4, tab)));
        }
        let movement = match key.code {
            K::Left | K::Up => Some(false),
            K::Right | K::Down => Some(true),
            _ => None,
        };
        if matches!(key.code, K::Char('[' | ']')) {
            let d = self.destinations();
            if !d.is_empty() {
                let forward = key.code == K::Char(']');
                let n = (self.subtab() + if forward { 1 } else { d.len() - 1 }) % d.len();
                self.focus = Focus::Content;
                return Ok(Some(self.go(d[n].1, d[n].2)));
            }
        }
        if matches!(self.focus, Focus::Global | Focus::Actions) {
            let b = match self.focus {
                Focus::Global => self.global_buttons(),
                _ => self.buttons(),
            };
            if let Some(forward) = movement {
                if !b.is_empty() {
                    self.control = (self.control.min(b.len() - 1)
                        + if forward { 1 } else { b.len() - 1 })
                        % b.len();
                }
                return Ok(Some(None));
            }
            if key.code == K::Enter {
                if let Some((_, command)) = b.get(self.control) {
                    return Ok(Some(self.activate(*command)?));
                }
                return Ok(Some(None));
            }
        }
        if self.focus != Focus::Content
            && matches!(key.code, K::Enter | K::Up | K::Down | K::Left | K::Right)
        {
            return Ok(Some(None));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_categorized_palette_no_more_alias_and_no_duplicate_commands() {
        let mut app = App::new(sample().unwrap(), true);
        for page in 0..12 {
            app.go(page, 0);
            assert!(app
                .buttons()
                .iter()
                .all(|(label, c)| *label != "More" && *c != Command::Actions));
            let palette = app.palette();
            for (i, item) in palette.iter().enumerate() {
                assert!(!palette[..i]
                    .iter()
                    .any(|other| other.command == item.command));
            }
            assert!(palette.iter().any(|c| c.section == "Go To"));
            assert!(palette.iter().any(|c| c.section == "Application"));
        }
    }
    #[test]
    fn overview_actions_follow_native_readiness_not_subscription_presence() {
        let mut app = App::new(sample().unwrap(), true);
        app.snapshot.store.nodes.clear();
        app.snapshot.store.subscriptions.clear();
        app.snapshot.store.native = Some(
            json!({"inbounds":[{"type":"mixed","listen_port":2080}],"outbounds":[{"type":"direct","tag":"direct"}]}),
        );
        assert!(!app.onboarding());
        assert_eq!(app.buttons()[0].0, "Test Connection");
        assert!(app.unavailable(Command::Key('v')).is_some());
        app.snapshot.store.native = None;
        assert!(app.onboarding());
        assert_eq!(app.buttons()[0].0, "Import Subscription");
        assert!(app.palette().iter().any(|i| i.command == Command::Setup));
    }
    fn press(app: &mut App, code: K) -> Option<Action> {
        app.key(KeyEvent::new(code, M::NONE)).unwrap()
    }
    fn focus_actions(app: &mut App) {
        for _ in 0..7 {
            if app.focus == Focus::Actions {
                return;
            }
            press(app, K::Tab);
        }
        panic!("Actions must be reachable with Tab");
    }
    #[test]
    fn tab_is_local_and_global_controls_are_one_key_away() {
        let mut app = App::new(sample().unwrap(), true);
        for workspace in '1'..='5' {
            press(&mut app, K::Char(workspace));
            press(&mut app, K::Tab);
            assert_eq!(app.focus, Focus::Actions);
            press(&mut app, K::Tab);
            assert_eq!(app.focus, Focus::Content);
            press(&mut app, K::BackTab);
            assert_eq!(app.focus, Focus::Actions);
            press(&mut app, K::F(6));
            assert_eq!(app.focus, Focus::Global);
            press(&mut app, K::Right);
            press(&mut app, K::Right);
            assert_eq!(app.global_buttons()[app.control].0, "Settings");
            press(&mut app, K::Esc);
            assert_eq!(app.focus, Focus::Content);
        }
        press(&mut app, K::Char(','));
        assert_eq!(app.page, 10);
        assert_eq!(app.focus, Focus::Content);
        press(&mut app, K::Char('M'));
        assert!(matches!(app.dialog, Some(Dialog::Form(_))));
    }
    #[test]
    fn destinations_remember_their_own_filter_and_selection_and_clamp_after_changes() {
        let mut app = App::new(sample().unwrap(), true);
        app.go(2, 1);
        app.filter = Input::new("o".into());
        let last = app.rows().len().saturating_sub(1);
        assert!(last > 0);
        app.selected[2] = last;
        app.go(2, 0);
        assert!(app.filter.value.is_empty());
        assert_eq!(app.selected[2], 0);
        app.go(5, 0);
        app.filter = Input::new("subscription".into());
        app.go(5, 1);
        assert!(app.filter.value.is_empty());
        app.go(2, 1);
        assert_eq!(app.filter.value, "o");
        assert_eq!(app.selected[2], last);
        app.go(3, 0);
        app.snapshot.store.native.as_mut().unwrap()["outbounds"] = json!([]);
        app.go(2, 1);
        assert_eq!(app.selected[2], 0);
        app.go(5, 0);
        assert_eq!(app.filter.value, "subscription");
    }
    #[test]
    fn shortcuts_never_escape_text_entry_and_stop_remains_a_confirmation() {
        let mut app = App::new(sample().unwrap(), true);
        press(&mut app, K::Char('I'));
        for c in "cMd,A?:I123[]q".chars() {
            assert!(press(&mut app, K::Char(c)).is_none());
        }
        press(&mut app, K::F(6));
        let Some(Dialog::Form(form)) = &app.dialog else {
            panic!()
        };
        assert_eq!(form.fields[form.selected].input.value, "cMd,A?:I123[]q");
        assert!(!app.quit);
        app.dialog = None;
        press(&mut app, K::Char('/'));
        for c in "cMd,A?:I123[]q".chars() {
            assert!(press(&mut app, K::Char(c)).is_none());
        }
        press(&mut app, K::F(6));
        assert_eq!(app.focus, Focus::Content);
        assert_eq!(app.filter.value, "cMd,A?:I123[]q");
        press(&mut app, K::Esc);
        press(&mut app, K::Char('d'));
        assert!(matches!(
            app.dialog,
            Some(Dialog::Text {
                action: Some(Action::Disconnect),
                ..
            })
        ));
    }
    #[test]
    fn dns_tabs_are_local_and_add_stays_visible() {
        let mut app = App::new(sample().unwrap(), true);
        let before = app.doc();
        app.go(4, 0);
        assert_eq!(app.buttons()[0].0, "Add Server");
        press(&mut app, K::Right);
        assert_eq!(app.path(), "/dns/rules");
        assert_eq!(app.buttons()[0].0, "Add DNS Rule");
        press(&mut app, K::Right);
        assert_eq!(app.path(), "/dns");
        press(&mut app, K::Left);
        assert_eq!(app.path(), "/dns/rules");
        press(&mut app, K::Tab);
        press(&mut app, K::Right);
        assert_eq!(app.path(), "/dns/rules");
        assert_eq!(app.control, 1);
        press(&mut app, K::Char('['));
        assert_eq!(app.page, 1);
        assert_eq!(app.doc(), before);
    }
    #[test]
    fn five_workspaces_map_to_existing_objects_without_mutation() {
        let mut app = App::new(sample().unwrap(), true);
        let before = app.doc();
        for (key, workspace, first_page, names) in [
            ('1', 0, 0, vec![]),
            ('2', 1, 2, vec!["Proxy Groups", "Nodes", "Subscriptions"]),
            ('3', 2, 3, vec!["Rules", "Rule Sets"]),
            ('4', 3, 11, vec!["Capture", "Inbounds", "DNS"]),
            ('5', 4, 7, vec!["Connections", "Logs", "Diagnostics"]),
        ] {
            press(&mut app, K::Char(key));
            assert_eq!(app.workspace(), workspace);
            assert_eq!(app.page, first_page);
            assert_eq!(
                app.destinations().iter().map(|d| d.0).collect::<Vec<_>>(),
                names
            );
            for (_, page, tab) in app.destinations() {
                app.go(page, tab);
                assert_eq!(app.workspace(), workspace);
            }
        }
        assert_eq!(app.doc(), before);
        app.go(4, 0);
        assert_eq!(app.path(), "/dns/servers");
        app.go(5, 1);
        assert_eq!(app.path(), "/route/rule_set");
        app.go(1, 0);
        assert_eq!(app.path(), "/inbounds");
    }
    #[test]
    fn familiar_import_buttons_work_with_tab_and_enter_and_input_is_not_navigation() {
        let mut app = App::new(sample().unwrap(), true);
        app.snapshot.store.native = None;
        focus_actions(&mut app);
        assert_eq!(app.buttons()[0].0, "Import Subscription");
        press(&mut app, K::Enter);
        assert!(matches!(app.dialog, Some(Dialog::Form(_))));
        for c in "12345".chars() {
            press(&mut app, K::Char(c));
        }
        assert_eq!(app.page, 0);
        let Some(Dialog::Form(f)) = &app.dialog else {
            panic!()
        };
        assert_eq!(f.fields[f.selected].input.value, "12345");
        // Tab onto the visible submit button. No F2 or hidden shortcut is used.
        press(&mut app, K::Tab);
        press(&mut app, K::Tab);
        assert!(
            matches!(press(&mut app,K::Enter),Some(Action::Import{source,..}) if source=="12345")
        );
        app.retry = None;
        app.snapshot = sample().unwrap();
        press(&mut app, K::Char('3'));
        focus_actions(&mut app);
        press(&mut app, K::Right);
        assert_eq!(app.buttons()[app.control].0, "Import Rule Set");
        let action = press(&mut app, K::Enter).unwrap();
        demo_action(&mut app, action).unwrap();
        assert!(matches!(app.dialog, Some(Dialog::RuleImport(_))));
    }
    #[test]
    fn group_node_and_all_outbound_views_keep_native_indices() {
        let mut app = App::new(sample().unwrap(), true);
        for tab in 0..3 {
            app.go(2, tab);
            let rows = app.rows();
            assert!(!rows.is_empty());
            for (i, _, v) in rows {
                assert_eq!(app.doc()["outbounds"][i], v);
                if tab == 0 {
                    assert!(v["type"] == "selector" || v["type"] == "urltest");
                }
                if tab == 1 {
                    assert!(v["type"] != "selector" && v["type"] != "direct");
                }
            }
        }
        assert!(app.rows().iter().any(|(_, _, v)| v["type"] == "direct"));
    }
    #[test]
    fn visible_select_member_button_opens_selection_not_group_editing() {
        let mut app = App::new(sample().unwrap(), true);
        app.go(2, 0);
        focus_actions(&mut app);
        press(&mut app, K::Right);
        press(&mut app, K::Right);
        assert_eq!(app.buttons()[app.control].0, "Select Member");
        assert!(press(&mut app, K::Enter).is_none());
        assert!(matches!(app.dialog, Some(Dialog::Select { .. })));
        assert!(matches!(
            press(&mut app, K::Enter),
            Some(Action::SelectNative { .. })
        ));
    }
    #[test]
    fn action_search_reaches_dns_and_advanced_without_config_copies() {
        let mut app = App::new(sample().unwrap(), true);
        let before = app.doc();
        for (query, page) in [("dns", 4), ("advanced tools", 6), ("subscriptions", 5)] {
            press(&mut app, K::Char(':'));
            for c in query.chars() {
                press(&mut app, K::Char(c));
            }
            press(&mut app, K::Enter);
            assert_eq!(app.page, page);
            assert!(app.dialog.is_none());
        }
        assert_eq!(app.doc(), before);
    }
    #[test]
    fn configure_tun_targets_the_existing_inbound() {
        let mut app = App::new(sample().unwrap(), true);
        app.snapshot.store.native.as_mut().unwrap()["inbounds"]
            .as_array_mut()
            .unwrap()
            .push(json!({"tag":"custom-tun","type":"tun","mtu":1400}));
        let before = app.doc();
        let i = native::array(&before, "/inbounds").len() - 1;
        assert!(
            matches!(app.activate(Command::Tun).unwrap(),Some(Action::ReadNative(p)) if p==format!("/inbounds/{i}"))
        );
        assert_eq!(app.doc(), before);
        app.snapshot.store.native.as_mut().unwrap()["inbounds"]
            .as_array_mut()
            .unwrap()
            .push(json!({"tag":"other-tun","type":"tun"}));
        assert!(app.activate(Command::Tun).unwrap().is_none());
        assert_eq!(app.page, 1);
        assert_eq!(app.rows().len(), 2);
    }
}
