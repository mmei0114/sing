//! Policies workspace: proxy groups, ordered routing rules and their sources.
use super::{
    editor, flows, labels,
    modal::{self, Choice, Picker},
    notify, text, theme, App,
};
use crate::{native, runtime::Action};
use crossterm::event::{KeyCode as K, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};
use serde_json::Value;

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    #[default]
    Groups,
    Rules,
    Sources,
}
impl Section {
    const ALL: [Self; 3] = [Self::Groups, Self::Rules, Self::Sources];
    fn name(self) -> &'static str {
        match self {
            Self::Groups => "Groups",
            Self::Rules => "Rules",
            Self::Sources => "Sources",
        }
    }
    fn index(self) -> usize {
        Self::ALL.iter().position(|x| *x == self).unwrap_or(0)
    }
}

#[derive(Default)]
pub struct State {
    pub section: Section,
    pub selected: [usize; 3],
}
pub fn capturing(_: &App) -> bool {
    false
}
pub fn paste(_: &mut App, _: &str) {}

fn groups(app: &App) -> Vec<(usize, &Value)> {
    native::array(app.doc(), "/outbounds")
        .iter()
        .enumerate()
        .filter(|(_, v)| labels::is_group(v))
        .collect()
}
fn rules(app: &App) -> &'_ [Value] {
    native::array(app.doc(), "/route/rules")
}
fn source_count(app: &App) -> usize {
    app.snap.store.subscriptions.len() + app.snap.store.rule_resources.len()
}
fn count(app: &App) -> usize {
    match app.proxies.section {
        Section::Groups => groups(app).len(),
        Section::Rules => rules(app).len(),
        Section::Sources => source_count(app),
    }
}

pub fn key(app: &mut App, k: KeyEvent) {
    let section_i = app.proxies.section.index();
    let n = count(app);
    if app.proxies.section == Section::Rules {
        if let Some(down) = editor::reorder_direction(k) {
            let i = app.proxies.selected[section_i];
            editor::shift(
                app,
                "/route/rules",
                i,
                down,
                Box::new(move |app, moved| {
                    app.proxies.selected[section_i] = moved;
                }),
            );
            return;
        }
    }
    match k.code {
        K::Char(']') | K::Right => {
            app.proxies.section = Section::ALL[(section_i + 1) % Section::ALL.len()]
        }
        K::Char('[') | K::Left => {
            app.proxies.section =
                Section::ALL[(section_i + Section::ALL.len() - 1) % Section::ALL.len()]
        }
        K::Down | K::Char('j') => {
            app.proxies.selected[section_i] =
                (app.proxies.selected[section_i] + 1).min(n.saturating_sub(1))
        }
        K::Up | K::Char('k') => {
            app.proxies.selected[section_i] = app.proxies.selected[section_i].saturating_sub(1)
        }
        K::Enter if app.proxies.section == Section::Groups => {
            if let Some((_, v)) = groups(app).get(app.proxies.selected[section_i]) {
                choose_member(app, native::tag(v).to_string());
            }
        }
        K::Enter | K::Char('e') if app.proxies.section == Section::Rules => {
            let i = app.proxies.selected[section_i];
            if i < rules(app).len() {
                editor::open(app, format!("/route/rules/{i}"));
            }
        }
        K::Char('e') if app.proxies.section == Section::Groups => {
            if let Some((i, _)) = groups(app).get(app.proxies.selected[section_i]) {
                editor::open(app, format!("/outbounds/{i}"));
            }
        }
        K::Char('n') if app.proxies.section == Section::Groups => {
            let choices = vec![
                Choice::new("manual", "Manual group", "you choose the active member"),
                Choice::new(
                    "auto",
                    "Automatic group",
                    "sing-box tests and chooses a member",
                ),
            ];
            app.push(Picker::single("New proxy group", choices, "manual", Box::new(|app, picked| {
                let auto = picked.first().is_some_and(|s| s == "auto");
                let v = if auto {
                    serde_json::json!({"type":"urltest","tag":"group","outbounds":[],"url":"https://www.gstatic.com/generate_204","interval":"3m"})
                } else {
                    serde_json::json!({"type":"selector","tag":"group","outbounds":[]})
                };
                editor::create(app, "/outbounds", v);
            })));
        }
        K::Char('n') if app.proxies.section == Section::Rules => {
            editor::create_from_template(app, "/route/rules")
        }
        K::Char('i') => flows::import_subscription(app),
        K::Char('R') => flows::import_rule_set(app),
        K::Char('l') if app.proxies.section == Section::Groups => {
            if let Some((_, v)) = groups(app).get(app.proxies.selected[section_i]) {
                test_latency(app, native::tag(v).to_string());
            }
        }
        K::Char('x') if app.proxies.section == Section::Groups => {
            if let Some((i, v)) = groups(app).get(app.proxies.selected[section_i]) {
                editor::remove(
                    app,
                    "/outbounds",
                    *i,
                    format!("group {}", app.label(native::tag(v))),
                );
            }
        }
        K::Char('x') if app.proxies.section == Section::Rules => {
            let i = app.proxies.selected[section_i];
            if let Some(v) = rules(app).get(i) {
                editor::remove(app, "/route/rules", i, labels::matcher(&app.snap.store, v));
            }
        }
        K::Char('u') if app.proxies.section == Section::Sources => refresh_source(app),
        K::Char('x') if app.proxies.section == Section::Sources => remove_source(app),
        _ => {}
    }
}

fn source_in_use(app: &mut App, name: &str, mut uses: Vec<String>) -> bool {
    if uses.is_empty() {
        return false;
    }
    uses.sort();
    uses.dedup();
    app.push(modal::TextView::plain(
        "Source is in use",
        &format!(
            "{name}\n\nChange or remove these references before deleting this source:\n\n{}\n\nNothing has been deleted.",
            uses.join("\n")
        ),
    ));
    true
}

fn source_removed(app: &mut App, reply: crate::runtime::Reply) {
    if !reply.ok {
        return notify(app, reply);
    }
    app.proxies.selected[Section::Sources.index()] = 0;
    app.toast("Source removed from draft · A applies");
}

fn remove_source(app: &mut App) {
    if app.snap.store.native.is_none() {
        return app.error("Initialize native configuration before removing sources.");
    }
    let i = app.proxies.selected[Section::Sources.index()];
    if let Some(s) = app.snap.store.subscriptions.get(i).cloned() {
        let nodes: Vec<_> = app
            .snap
            .store
            .nodes
            .iter()
            .filter(|n| n.provider == s.id)
            .collect();
        let node_count = nodes.len();
        let mut uses: Vec<String> = nodes
            .iter()
            .flat_map(|n| {
                native::reference_paths(app.doc(), &n.tag())
                    .into_iter()
                    .map(|p| format!("{}: {p}", n.name))
            })
            .collect();
        if nodes
            .iter()
            .any(|n| n.tag() == app.snap.store.settings.global_target)
        {
            uses.push("Global mode target: choose another target in Mode first".into());
        }
        if source_in_use(app, &s.name, uses) {
            return;
        }
        app.push(modal::Confirm::new(
            "Remove node source?",
            format!("{}\n\nRemove this subscription and its {node_count} nodes from the draft.\nThe running core stays unchanged until Apply.", s.name),
            "Remove",
            Box::new(move |app| app.request(Action::Delete(s.id), Box::new(source_removed))),
        ).danger());
        return;
    }
    let Some(resource) = app
        .snap
        .store
        .rule_resources
        .get(i.saturating_sub(app.snap.store.subscriptions.len()))
        .cloned()
    else {
        return;
    };
    let tag = resource.tag();
    let uses = native::links::links(app.doc())
        .into_iter()
        .filter(|l| l.kind == native::links::ObjectKind::RuleSet && l.tag == tag)
        .map(|l| l.path)
        .collect();
    if source_in_use(app, &resource.name, uses) {
        return;
    }
    app.push(modal::Confirm::new(
        "Remove rule source?",
        format!("{}\n\nRemove this source and its rule set from the draft.\nThe running core stays unchanged until Apply.", resource.name),
        "Remove",
        Box::new(move |app| {
            app.request(Action::ReadNative("/route/rule_set".into()), Box::new(move |app, r| {
                let Some(mut edit) = r.edit.filter(|_| r.ok) else {
                    return app.error(r.message);
                };
                let Some(items) = edit.value.as_array_mut() else {
                    return app.error("Rule sets are unavailable. Reopen Sources.");
                };
                // Resolve the stable tag again: another interface may have reordered the list.
                let Some(index) = items.iter().position(|v| native::tag(v) == tag) else {
                    return app.error("This rule set no longer exists. Reopen Sources.");
                };
                items.remove(index);
                // The manager rechecks references and the revision before saving atomically.
                app.request(Action::WriteNative(edit), Box::new(source_removed));
            }));
        }),
    ).danger());
}

fn refresh_source(app: &mut App) {
    let i = app.proxies.selected[Section::Sources.index()];
    if let Some(s) = app.snap.store.subscriptions.get(i) {
        app.request_busy(
            Action::Refresh(s.id.clone()),
            "Refreshing subscription",
            Box::new(|app, r| {
                if !r.ok {
                    return notify(app, r);
                }
                let Some(p) = r.preview else {
                    return app.error("No refresh preview returned");
                };
                let action = Action::CommitSubscriptions {
                    id: p.id,
                    revision: p.revision,
                };
                app.push(super::modal::Confirm::new(
                    "Review source update",
                    format!(
                        "{}\n{} nodes · +{} / -{}\n\nGroups, rules and DNS stay unchanged.",
                        p.name, p.count, p.added, p.removed
                    ),
                    "Save Update",
                    Box::new(move |app| app.send(action)),
                ));
            }),
        );
        return;
    }
    let j = i.saturating_sub(app.snap.store.subscriptions.len());
    if let Some(r) = app.snap.store.rule_resources.get(j) {
        app.request_busy(
            Action::RefreshRules(r.id.clone()),
            "Refreshing rules",
            Box::new(notify),
        );
    }
}

fn tabs(app: &App) -> Line<'static> {
    let mut spans = vec![];
    for section in Section::ALL {
        spans.push(Span::styled(
            section.name().to_string(),
            if section == app.proxies.section {
                theme::bold(theme::accent())
            } else {
                theme::s(theme::dim())
            },
        ));
        spans.push(Span::raw("   "));
    }
    spans.push(Span::styled("← →", theme::s(theme::dim())));
    Line::from(spans)
}

pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let area = area.inner(ratatui::layout::Margin {
        horizontal: 2,
        vertical: 0,
    });
    let [nav, body] = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).areas(area);
    f.render_widget(Paragraph::new(tabs(app)), nav);
    match app.proxies.section {
        Section::Groups => draw_groups(f, body, app),
        Section::Rules => draw_rules(f, body, app),
        Section::Sources => draw_sources(f, body, app),
    }
}

fn split(area: Rect) -> (Rect, Option<Rect>) {
    if area.width < 96 {
        return (area, None);
    }
    let [a, _, b] = Layout::horizontal([
        Constraint::Percentage(62),
        Constraint::Length(2),
        Constraint::Percentage(38),
    ])
    .areas(area);
    (a, Some(b))
}

fn draw_groups(f: &mut Frame, area: Rect, app: &App) {
    let rows = groups(app);
    let (list, detail) = split(area);
    if rows.is_empty() {
        f.render_widget(Paragraph::new("\n  No proxy groups yet.\n\n  n creates a manual or automatic group · i imports a subscription."), list);
        return;
    }
    let selected = app.proxies.selected[Section::Groups.index()].min(rows.len() - 1);
    let height = list.height.saturating_sub(1) as usize;
    let start = modal::scroll(selected, height, rows.len());
    let w = list.width as usize;
    let name_w = (w / 3).clamp(14, 28);
    let member_w = w.saturating_sub(name_w + 20).max(12);
    let mut lines = vec![Line::from(vec![
        Span::raw(" "),
        Span::styled(text::cell("GROUP", name_w), theme::s(theme::faint())),
        Span::styled(text::cell("CURRENT", member_w), theme::s(theme::faint())),
        Span::styled(text::right("LATENCY", 10), theme::s(theme::faint())),
    ])];
    for (row_i, (_, v)) in rows.iter().enumerate().skip(start).take(height) {
        let tag = native::tag(v);
        let (member, delay) = current_member(app, tag, v);
        lines.push(modal::row(
            row_i == selected,
            vec![
                Span::styled(
                    text::cell(&app.label(tag), name_w),
                    theme::bold(theme::text()),
                ),
                Span::styled(
                    text::cell(&app.label(&member), member_w),
                    theme::s(theme::target(&member)),
                ),
                Span::styled(
                    text::right(&delay_text(delay), 10),
                    theme::s(delay_color(delay)),
                ),
            ],
        ));
    }
    f.render_widget(Paragraph::new(lines), list);
    if let Some(detail) = detail {
        let (_, v) = rows[selected];
        let tag = native::tag(v);
        let (member, _) = current_member(app, tag, v);
        let members: Vec<String> = native::array(v, "/outbounds")
            .iter()
            .filter_map(Value::as_str)
            .map(|m| format!("{}  {}", if m == member { "●" } else { "○" }, app.label(m)))
            .collect();
        f.render_widget(Paragraph::new(format!("{}\n{}\n\n{} members\n{}\n\nEnter chooses a member\ne edits the group\nl tests latency", app.label(tag), labels::protocol(v["type"].as_str().unwrap_or("")), members.len(), members.join("\n"))).wrap(Wrap { trim: false }), detail);
    }
}

fn draw_rules(f: &mut Frame, area: Rect, app: &App) {
    let rows = rules(app);
    let (list, detail) = split(area);
    if rows.is_empty() {
        f.render_widget(Paragraph::new("\n  No routing rules. Unmatched traffic uses Route › final.\n\n  n creates a rule · R imports a rule set."), list);
        return;
    }
    let selected = app.proxies.selected[Section::Rules.index()].min(rows.len() - 1);
    let height = list.height.saturating_sub(1) as usize;
    let start = modal::scroll(selected, height, rows.len());
    let w = list.width as usize;
    let match_w = w.saturating_sub(24).max(12);
    let mut lines = vec![Line::from(vec![
        Span::raw(" "),
        Span::styled(text::cell("MATCH", match_w), theme::s(theme::faint())),
        Span::styled("ACTION", theme::s(theme::faint())),
    ])];
    for (i, rule) in rows.iter().enumerate().skip(start).take(height) {
        let (_, target) = labels::action(rule);
        let action = labels::route_action_label(rule);
        let action_text = if target.is_empty() {
            action.into()
        } else if target == "reject" {
            "Reject".into()
        } else {
            format!("→ {}", app.label(&target))
        };
        lines.push(modal::row(
            i == selected,
            vec![
                Span::styled(
                    text::cell(
                        &format!(
                            "{:>3}  {}",
                            i + 1,
                            labels::route_matcher(&app.snap.store, rule)
                        ),
                        match_w,
                    ),
                    theme::s(theme::text()),
                ),
                Span::styled(
                    text::fit(&action_text, 20),
                    theme::s(theme::target(&target)),
                ),
            ],
        ));
    }
    f.render_widget(Paragraph::new(lines), list);
    if let Some(detail) = detail {
        let rule = &rows[selected];
        let (action, target) = labels::action(rule);
        f.render_widget(
            Paragraph::new(format!(
                "Rule {}\n\nMatch\n{}\n\nAction\n{} ({action}){}\n\nNative order is preserved.",
                selected + 1,
                labels::route_matcher(&app.snap.store, rule),
                labels::route_action_label(rule),
                if target.is_empty() {
                    String::new()
                } else {
                    format!(" → {}", app.label(&target))
                }
            ))
            .wrap(Wrap { trim: false }),
            detail,
        );
    }
}

fn draw_sources(f: &mut Frame, area: Rect, app: &App) {
    let n = source_count(app);
    if n == 0 {
        f.render_widget(Paragraph::new("\n  No sources.\n\n  i imports a node subscription · R imports a routing rule set."), area);
        return;
    }
    let selected = app.proxies.selected[Section::Sources.index()].min(n - 1);
    let height = area.height.saturating_sub(1) as usize;
    let start = modal::scroll(selected, height, n);
    let mut lines = vec![Line::from(vec![
        Span::raw(" "),
        Span::styled(text::cell("SOURCE", 30), theme::s(theme::faint())),
        Span::styled(text::cell("KIND", 16), theme::s(theme::faint())),
        Span::styled("STATUS", theme::s(theme::faint())),
    ])];
    for i in start..(start + height).min(n) {
        let (name, kind, status) = if let Some(s) = app.snap.store.subscriptions.get(i) {
            let nodes: Vec<_> = app
                .snap
                .store
                .nodes
                .iter()
                .filter(|n| n.provider == s.id)
                .collect();
            let mut protocols: Vec<_> = nodes.iter().map(|n| n.kind()).collect();
            protocols.sort_unstable();
            protocols.dedup();
            (
                s.name.clone(),
                format!("Nodes · {}", s.format),
                format!(
                    "{} nodes / {} protocols · {}",
                    nodes.len(),
                    protocols.len(),
                    text::ago(crate::model::now().saturating_sub(s.updated_at))
                ),
            )
        } else {
            let r = &app.snap.store.rule_resources[i - app.snap.store.subscriptions.len()];
            (
                r.name.clone(),
                format!("Rules · {}", r.format),
                format!(
                    "{} / {} entries · {}",
                    r.rules.len(),
                    r.input_count,
                    text::ago(crate::model::now().saturating_sub(r.updated_at))
                ),
            )
        };
        lines.push(modal::row(
            i == selected,
            vec![
                Span::styled(text::cell(&name, 30), theme::s(theme::text())),
                Span::styled(text::cell(&kind, 16), theme::s(theme::dim())),
                Span::styled(status, theme::s(theme::dim())),
            ],
        ));
    }
    f.render_widget(Paragraph::new(lines), area);
}

pub fn hints(app: &App) -> Vec<(&'static str, &'static str)> {
    match app.proxies.section {
        Section::Groups => vec![
            ("↑↓", "group"),
            ("enter", "choose"),
            ("n", "new group"),
            ("e", "edit"),
            ("l", "test"),
            ("i", "import nodes"),
        ],
        Section::Rules => vec![
            ("↑↓", "rule"),
            ("Alt+↑↓", "reorder"),
            ("enter", "edit"),
            ("n", "new rule"),
            ("R", "import rule set"),
        ],
        Section::Sources => vec![
            ("↑↓", "source"),
            ("u", "update"),
            ("x", "remove"),
            ("i", "import nodes"),
            ("R", "import rules"),
        ],
    }
}

/// (member in use, its latency). Live API values while running; the saved default otherwise.
pub fn current_member(app: &App, tag: &str, v: &Value) -> (String, i32) {
    if let Some(g) = app.live_group(tag) {
        let delay = g
            .items
            .iter()
            .find(|i| i.tag == g.selected)
            .map(|i| i.delay)
            .unwrap_or(0);
        return (g.selected.clone(), delay);
    }
    let member = v["default"]
        .as_str()
        .or_else(|| v["outbounds"][0].as_str())
        .unwrap_or("")
        .to_string();
    (member, 0)
}
pub fn member_delay(app: &App, group: &str, member: &str) -> i32 {
    app.snap
        .groups
        .group
        .iter()
        .filter(|g| g.tag == group || group.is_empty())
        .flat_map(|g| g.items.iter())
        .filter(|i| i.tag == member)
        .map(|i| i.delay)
        .max()
        .unwrap_or(0)
}
pub fn delay_text(delay: i32) -> String {
    match delay {
        d if d <= 0 => String::new(),
        65535.. => "timeout".into(),
        d => format!("{d} ms"),
    }
}
pub fn delay_color(delay: i32) -> ratatui::style::Color {
    match delay {
        d if d <= 0 => theme::dim(),
        1..=150 => theme::good(),
        151..=400 => theme::warn(),
        _ => theme::bad(),
    }
}
pub fn test_latency(app: &mut App, tag: String) {
    if !app.snap.connected {
        return app.error("Start the core to test latency");
    }
    app.request_busy(Action::Test(tag), "Testing", Box::new(notify));
}
pub fn choose_member(app: &mut App, tag: String) {
    let Some(v) = native::array(app.doc(), "/outbounds")
        .iter()
        .find(|v| native::tag(v) == tag)
        .cloned()
    else {
        return;
    };
    if v["type"] == "urltest" {
        return app.error(
            "Automatic groups choose from latency tests. Press l to test or e to edit members.",
        );
    }
    let (current, _) = current_member(app, &tag, &v);
    let choices: Vec<Choice> = native::array(&v, "/outbounds")
        .iter()
        .filter_map(Value::as_str)
        .map(|m| {
            let kind = native::array(app.doc(), "/outbounds")
                .iter()
                .chain(native::array(app.doc(), "/endpoints"))
                .find(|o| native::tag(o) == m)
                .map(|o| labels::protocol(o["type"].as_str().unwrap_or("")).to_string())
                .unwrap_or_default();
            Choice::new(
                m,
                app.label(m),
                format!("{kind}  {}", delay_text(member_delay(app, &tag, m))),
            )
        })
        .collect();
    if choices.is_empty() {
        return app.error("This group has no members. Press e to add some.");
    }
    let running = app.snap.connected && app.live_group(&tag).is_some();
    app.push(Picker::single(
        &format!("{} · choose proxy", app.label(&tag)),
        choices,
        &current,
        Box::new(move |app, picked| {
            let Some(member) = picked.into_iter().next() else {
                return;
            };
            if running {
                app.request(
                    Action::SelectNative { group: tag, member },
                    Box::new(notify),
                );
            } else {
                set_default(app, tag, member);
            }
        }),
    ));
}
fn set_default(app: &mut App, tag: String, member: String) {
    let Some(i) = native::array(app.doc(), "/outbounds")
        .iter()
        .position(|v| native::tag(v) == tag)
    else {
        return;
    };
    app.request(
        Action::ReadNative(format!("/outbounds/{i}")),
        Box::new(move |app, r| {
            let Some(mut edit) = r.edit.filter(|e| r.ok && native::tag(&e.value) == tag) else {
                return app.error("Group changed; try again");
            };
            edit.value["default"] = serde_json::json!(member);
            app.request(
                Action::WriteNative(edit),
                Box::new(|app, r| {
                    if r.ok {
                        app.toast("Default saved · A applies")
                    } else {
                        notify(app, r)
                    }
                }),
            );
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn policies_sections_stay_small() {
        assert_eq!(
            Section::ALL.map(Section::name),
            ["Groups", "Rules", "Sources"]
        );
    }
}
