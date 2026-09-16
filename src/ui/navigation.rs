//! Product navigation maps onto the existing native editors, never onto copies of data.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Focus {
    Global,
    Navigation,
    Subnavigation,
    Actions,
    Content,
    Review,
}

#[derive(Clone, Copy)]
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
}
pub(super) type Button = (&'static str, Command);

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
        self.page = page;
        self.tabs[page] = tab;
        self.selected[page] = 0;
        self.filter = Input::new(String::new());
        self.control = 0;
        (page == 7).then_some(Action::Connections)
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
            ("Rule / Global / Direct", Command::Key('M')),
            ("Settings", Command::Open(10, 0)),
            ("Help", Command::Key('?')),
            ("Actions", Command::Actions),
        ]
    }
    pub(super) fn buttons(&self) -> Vec<Button> {
        let mut buttons = self.all_buttons();
        if buttons.len() > 5 {
            buttons.truncate(4);
            buttons.push(("More", Command::Actions));
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
                let mut b = vec![
                    ("Servers", Open(4, 0)),
                    ("Rules", Open(4, 1)),
                    ("Options", Open(4, 2)),
                ];
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
        self.focus = Focus::Content;
        match command {
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
                let mut choices = self.all_buttons();
                choices.extend(
                    self.global_buttons()
                        .into_iter()
                        .filter(|(_, c)| !matches!(c, Command::Actions)),
                );
                choices.extend([
                    ("Subscriptions", Command::Open(5, 0)),
                    ("Connection Setup", Command::Setup),
                    ("Proxy Groups", Command::Open(2, 0)),
                    ("Rules", Command::Open(3, 0)),
                    ("Rule Sets", Command::Open(5, 1)),
                    ("Capture", Command::Open(11, 0)),
                    ("Inbounds", Command::Open(1, 0)),
                    ("DNS", Command::Open(4, 0)),
                    ("Connections", Command::Open(7, 0)),
                    ("Logs", Command::Open(8, 0)),
                    ("Diagnostics", Command::Open(9, 0)),
                    ("Advanced Tools", Command::Open(6, 0)),
                ]);
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
    // None means the key belongs to the content/editor, not navigation.
    pub(super) fn navigation_key(&mut self, key: KeyEvent) -> Result<Option<Option<Action>>> {
        if matches!(key.code, K::Tab | K::BackTab) {
            let mut order = vec![
                Focus::Content,
                Focus::Review,
                Focus::Global,
                Focus::Navigation,
            ];
            if !self.destinations().is_empty() {
                order.push(Focus::Subnavigation);
            }
            if !self.buttons().is_empty() {
                order.push(Focus::Actions);
            }
            let i = order.iter().position(|f| *f == self.focus).unwrap_or(0);
            self.focus = order[(i + if key.code == K::Tab {
                1
            } else {
                order.len() - 1
            }) % order.len()];
            self.control = 0;
            return Ok(Some(None));
        }
        if let K::Char(c @ '1'..='5') = key.code {
            let action = self.go_workspace((c as u8 - b'1') as usize);
            self.focus = Focus::Content;
            return Ok(Some(action));
        }
        if key.code == K::Char(',') {
            return Ok(Some(self.go(10, 0)));
        }
        if key.code == K::Esc && self.focus != Focus::Content {
            self.focus = Focus::Content;
            return Ok(Some(None));
        }
        let movement = match key.code {
            K::Left | K::Up => Some(false),
            K::Right | K::Down => Some(true),
            _ => None,
        };
        if self.focus == Focus::Navigation {
            if let Some(forward) = movement {
                let i = self.workspace().min(4);
                return Ok(Some(self.go_workspace(if forward {
                    (i + 1) % 5
                } else {
                    (i + 4) % 5
                })));
            }
            if key.code == K::Enter {
                self.focus = Focus::Content;
                return Ok(Some(None));
            }
        }
        if (self.focus == Focus::Subnavigation && movement.is_some())
            || matches!(key.code, K::Char('[' | ']'))
        {
            let d = self.destinations();
            if !d.is_empty() {
                let forward = movement.unwrap_or(key.code == K::Char(']'));
                let n = (self.subtab() + if forward { 1 } else { d.len() - 1 }) % d.len();
                return Ok(Some(self.go(d[n].1, d[n].2)));
            }
        }
        if self.focus == Focus::Subnavigation && key.code == K::Enter {
            self.focus = Focus::Content;
            return Ok(Some(None));
        }
        if matches!(self.focus, Focus::Global | Focus::Actions | Focus::Review) {
            let b = match self.focus {
                Focus::Global => self.global_buttons(),
                Focus::Review => vec![("Review", Command::Key('A'))],
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
