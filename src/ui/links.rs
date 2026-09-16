use super::*;

#[derive(Clone)]
pub(super) struct Browser {
    pub title: String,
    pub items: Vec<(String, Option<String>)>,
    pub selected: usize,
    doc: Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app() -> App {
        let mut a = App::new(sample().unwrap(), true);
        a.snapshot.store.native = Some(
            json!({"route":{"rule_set":[{"type":"inline","tag":"video","rules":[{"domain_suffix":["example.test"]}]}],"rules":[{"rule_set":["video"],"outbound":"proxy"}]},"outbounds":[{"type":"direct","tag":"proxy"}],"dns":{"servers":[{"type":"local","tag":"dns"}],"rules":[{"type":"logical","rules":[{"rule_set":"video"}],"server":"dns"}]}}),
        );
        a.go(5, 1);
        a.filter = Input::new("video".into());
        a
    }
    fn press(a: &mut App, key: K) -> Option<Action> {
        a.key(KeyEvent::new(key, M::NONE)).unwrap()
    }
    #[test]
    fn reference_jump_and_back_restore_filter_selection_and_list() {
        let mut a = app();
        a.activate(Command::References).unwrap();
        let Some(Dialog::References(b)) = &a.dialog else {
            panic!()
        };
        assert_eq!(b.items.len(), 2);
        assert!(b.items[0].0.contains("/dns/rules/0/rules/0/rule_set"));
        press(&mut a, K::Enter);
        assert_eq!((a.page, a.tabs[4]), (4, 1));
        assert!(a.filter.value.is_empty());
        assert_eq!(a.buttons()[0].0, "Back to References");
        a.activate(Command::Back).unwrap();
        assert_eq!((a.page, a.tabs[5]), (5, 1));
        assert_eq!(a.filter.value, "video");
        let Some(Dialog::References(b)) = &a.dialog else {
            panic!()
        };
        assert_eq!(b.selected, 0);
        press(&mut a, K::Down);
        press(&mut a, K::Enter);
        assert_eq!((a.page, a.tabs[3]), (3, 0));
    }
    #[test]
    fn missing_or_changed_references_never_jump_to_a_different_object() {
        let mut a = app();
        a.go(3, 0);
        a.snapshot.store.native.as_mut().unwrap()["route"]["rules"][0]["outbound"] =
            json!("missing");
        a.activate(Command::References).unwrap();
        let Some(Dialog::References(b)) = &mut a.dialog else {
            panic!()
        };
        b.selected = b.items.iter().position(|(_, p)| p.is_none()).unwrap();
        press(&mut a, K::Enter);
        assert!(a.notice.contains("missing"));
        assert!(a.history.is_empty());
        assert!(matches!(a.dialog, Some(Dialog::References(_))));
        a.snapshot.store.native.as_mut().unwrap()["route"]["rules"] = json!([]);
        press(&mut a, K::Enter);
        assert!(a.notice.contains("Draft changed"));
        assert!(a.history.is_empty());
    }
    #[test]
    fn delete_opens_references_and_rejects_a_changed_selected_object() {
        let mut a = app();
        assert!(matches!(
            press(&mut a, K::Char('x')),
            Some(Action::ReadNative(_))
        ));
        let make_reply = |value| -> Reply {
            serde_json::from_value(json!({"ok":true,"message":"","needs_auth":false,"edit":{"revision":"server-revision","pointer":"/route/rule_set","value":value}})).unwrap()
        };
        assert!(a
            .receive(make_reply(a.doc()["route"]["rule_set"].clone()))
            .unwrap()
            .is_none());
        assert!(a.notice.contains("Cannot remove"));
        assert!(matches!(a.dialog, Some(Dialog::References(_))));
        assert!(a.history.is_empty());
        press(&mut a, K::Esc);
        press(&mut a, K::Char('x'));
        assert!(a
            .receive(make_reply(
                json!([{"type":"inline","tag":"different","rules":[]}])
            ))
            .unwrap_err()
            .to_string()
            .contains("Selected object changed"));
        assert!(a.dialog.is_none());
    }
    #[test]
    fn reference_list_renders_and_scrolls_at_small_sizes() {
        let mut a = app();
        a.snapshot.store.native.as_mut().unwrap()["route"]["rules"] = json!((0..250)
            .map(|_| json!({"rule_set":"video","outbound":"proxy"}))
            .collect::<Vec<_>>());
        a.activate(Command::References).unwrap();
        let Some(Dialog::References(b)) = &mut a.dialog else {
            panic!()
        };
        b.selected = b.items.len() - 1;
        let target = b.items.last().unwrap().0.clone();
        for (w, h) in [(54, 18), (80, 24), (140, 40)] {
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            t.draw(|f| view::draw(f, &a)).unwrap();
            let text = t
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            assert!(text.contains("Open Reference") && text.contains(&target));
        }
    }
}
#[derive(Clone)]
pub(super) struct Location {
    page: usize,
    tabs: [usize; 12],
    selected: [usize; 12],
    filter: Input,
    focus: Focus,
    control: usize,
    dialog: Option<Box<Dialog>>,
}

fn within(path: &str, parent: &str) -> bool {
    path == parent
        || path
            .strip_prefix(parent)
            .is_some_and(|s| s.starts_with('/'))
}

impl App {
    pub(super) fn reference_browser(&mut self) -> Result<()> {
        let (index, name, _) = self.current().context("Select an object")?;
        let path = if (self.page == 3 && self.tabs[3] == 1) || (self.page == 4 && self.tabs[4] == 2)
        {
            self.path().to_string()
        } else if self.page == 6 {
            format!("/{}", name.replace('~', "~0").replace('/', "~1"))
        } else {
            format!("{}/{index}", self.path())
        };
        let doc = self.doc();
        let defs = native::links::definitions(&doc);
        let refs = native::links::links(&doc);
        let mut items = vec![];
        for reference in &refs {
            if defs.iter().any(|d| {
                within(&d.path, &path) && d.kind == reference.kind && d.tag == reference.tag
            }) {
                items.push((
                    format!("Used by {}", reference.path),
                    Some(reference.path.clone()),
                ));
            }
            if within(&reference.path, &path) {
                let target = defs
                    .iter()
                    .find(|d| d.kind == reference.kind && d.tag == reference.tag);
                items.push((
                    format!(
                        "Uses {} → {}{}",
                        reference.path,
                        model::clean(&reference.tag),
                        if target.is_none() { " [missing]" } else { "" }
                    ),
                    target.map(|d| d.path.clone()),
                ));
            }
        }
        items.sort();
        items.dedup();
        self.dialog = Some(Dialog::References(Browser {
            title: format!("References · {}", model::clean(&name)),
            items,
            selected: 0,
            doc,
        }));
        Ok(())
    }
    pub(super) fn follow_reference(&mut self, browser: Browser) -> Result<Option<Action>> {
        ensure!(
            browser.doc == self.doc(),
            "Draft changed. Close and reopen References before jumping."
        );
        let Some((_, target)) = browser.items.get(browser.selected) else {
            return Ok(None);
        };
        let target = target
            .clone()
            .context("Referenced object is missing. Return and repair its tag in the editor.")?;
        self.history.push(Location {
            page: self.page,
            tabs: self.tabs,
            selected: self.selected,
            filter: self.filter.clone(),
            focus: self.focus,
            control: self.control,
            dialog: Some(Box::new(Dialog::References(browser))),
        });
        // Longest object prefixes first; options must not swallow rules/resources.
        for (path, page, tab) in [
            ("/route/rule_set", 5, 1),
            ("/route/rules", 3, 0),
            ("/dns/servers", 4, 0),
            ("/dns/rules", 4, 1),
            ("/outbounds", 2, 2),
            ("/inbounds", 1, 0),
        ] {
            if let Some(suffix) = target.strip_prefix(&format!("{path}/")) {
                if let Some(index) = suffix
                    .split('/')
                    .next()
                    .and_then(|s| s.parse::<usize>().ok())
                {
                    self.go(page, tab);
                    self.selected[page] = index.min(self.rows().len().saturating_sub(1));
                    self.focus = Focus::Content;
                    self.notice = format!("Reference: {target}");
                    return Ok(None);
                }
            }
        }
        for (path, page, tab) in [("/route", 3, 1), ("/dns", 4, 2)] {
            if within(&target, path) {
                self.go(page, tab);
                self.notice = format!("Reference: {target}");
                return Ok(None);
            }
        }
        self.go(6, 0);
        let root = target.split('/').nth(1).unwrap_or("");
        self.selected[6] = self
            .rows()
            .iter()
            .position(|(_, name, _)| name == root)
            .unwrap_or(0);
        self.notice = format!("Reference: {target} · open Native JSON to inspect advanced fields.");
        Ok(None)
    }
    pub(super) fn back_to_reference(&mut self) {
        if let Some(location) = self.history.pop() {
            self.page = location.page;
            self.tabs = location.tabs;
            self.selected = location.selected;
            self.filter = location.filter;
            self.focus = location.focus;
            self.control = location.control;
            self.dialog = location.dialog.map(|d| *d);
        }
    }
}
