//! Focused electrical shortcut menu. Action applicability stays in `context`;
//! this module owns category order and common operations, independently of painting.
use crate::{CadApp, menus::Entry};
use cadcraft_engine::{doc::Space, find_command};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Category {
    Edit,
    Move,
    Connect,
    Rooms,
    Delete,
    Info,
}
const ORDER: [(Category, &str); 6] = [
    (Category::Edit, "Edit electrical object"),
    (Category::Move, "Move and copy"),
    (Category::Connect, "Electrical connections"),
    (Category::Rooms, "Rooms and assignments"),
    (Category::Delete, "Delete electrical object"),
    (Category::Info, "Object information"),
];

pub(crate) struct Group {
    pub label: &'static str,
    pub entries: Vec<Entry>,
}

/// New electrical actions default to Edit until assigned another semantic category.
fn category(id: &str) -> Category {
    match id {
        "ui.elettrico.unifilar.circuit.move"
        | "ui.elettrico.unifilar.duplicate.column"
        | "ui.elettrico.unifilar.column.move"
        | "ui.elettrico.unifilar.duplicate.circuit"
        | "move"
        | "copy"
        | "rotate" => Category::Move,
        "ui.elettrico.unifilar.link.series"
        | "ui.elettrico.unifilar.link.bus"
        | "ui.elettrico.unifilar.link.branch"
        | "ui.elettrico.unifilar.link.pe" => Category::Connect,
        "ui.elettrico.context.assign" | "ui.elettrico.context.auto" => Category::Rooms,
        "ui.elettrico.unifilar.link.delete" | "ui.elettrico.context.delete" | "erase" => Category::Delete,
        "ui.elettrico.context.info" | "ui.elettrico.context.members" | "properties" => Category::Info,
        _ => Category::Edit,
    }
}

fn flatten(entries: Vec<Entry>, out: &mut Vec<Entry>) {
    for e in entries {
        match e {
            Entry::Sub { children, .. } => flatten(children, out),
            item => out.push(item),
        }
    }
}

pub(crate) fn groups(app: &CadApp) -> Option<Vec<Group>> {
    if app.session.running.is_some()
        || app.elettrico.editing.snapshot.is_some()
        || app.session.space() != Space::Model
        || app.session.state().is_ok_and(|s| s.block_edit.is_some())
    {
        return None;
    }
    let doc = app.session.doc().ok()?;
    let selected = app.session.selection();
    if selected.is_empty() {
        return None;
    }
    let mut unifilar = false;
    let mut room = false;
    for h in selected {
        let e = doc.entity(h)?;
        if doc.space_of(h) != Some(Space::Model) {
            return None;
        }
        let mut native = false;
        for t in e.common.xdata.iter().filter(|t| t.code == 1001) {
            match t.value.as_str() {
                "CC_COMPOSITORE" | "CC_SIMBOLO" => native = true,
                "CC_AMBIENTE" => {
                    native = true;
                    room = true;
                }
                "CC_UNIFILARE_DEVICE" | "CC_UNIFILARE_LINK" => {
                    native = true;
                    unifilar = true;
                }
                _ => {}
            }
        }
        if !native {
            return None;
        }
    }
    let mut actions = Vec::new();
    flatten(super::entries(app), &mut actions);
    // Unifilar geometry owns table data and attached routes: promote its native
    // circuit move, never a generic transform that bypasses those relationships.
    let common: &[&str] = if unifilar {
        &["properties"]
    } else if room {
        &["move", "copy", "rotate", "properties"]
    } else {
        &["move", "copy", "rotate", "erase", "properties"]
    };
    for id in common {
        if let Some(c) = find_command(id) {
            actions.push(Entry::Item {
                label: if *id == "copy" { "Copy Selection" } else { c.label }.into(),
                id: (*id).into(),
                shortcut: c.shortcut.map(str::to_owned),
                enabled: (c.enabled)(&app.session).is_ok(),
            });
        }
    }
    Some(
        ORDER
            .into_iter()
            .filter_map(|(group, label)| {
                let entries: Vec<_> = actions.iter().filter(|e| matches!(e, Entry::Item { id, .. } if category(id) == group)).cloned().collect();
                (!entries.is_empty()).then_some(Group { label, entries })
            })
            .collect(),
    )
}

pub(crate) fn show(ui: &mut egui::Ui, groups: &[Group], clicked: &mut Option<String>) {
    for (i, group) in groups.iter().enumerate() {
        if i > 0 {
            ui.separator();
        }
        ui.label(egui::RichText::new(crate::i18n::t(group.label)).small().weak());
        for e in &group.entries {
            crate::menus::entry_ui(ui, e, clicked);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_engine::{
        Session,
        doc::{Common, EntityKind, Insert, RawTag},
        geom::Vec3,
    };
    fn app(tags: &[&str]) -> CadApp {
        let mut app = CadApp::new(Session::new(), crate::Services::default());
        let handles = tags
            .iter()
            .map(|tag| {
                let mut c = Common::default();
                c.xdata.push(RawTag { code: 1001, value: (*tag).into() });
                app.session
                    .doc_mut()
                    .unwrap()
                    .add(
                        &Space::Model,
                        c,
                        EntityKind::Insert(Insert {
                            block: "Synthetic".into(),
                            insert: Vec3::ZERO,
                            scale: Vec3::new(1., 1., 1.),
                            rotation: 0.,
                            attribs: vec![],
                            cols: 1,
                            rows: 1,
                            col_spacing: 0.,
                            row_spacing: 0.,
                        }),
                    )
                    .unwrap()
            })
            .collect();
        app.session.set_selection(handles);
        app
    }
    fn ids(groups: &[Group]) -> Vec<&str> {
        groups.iter().flat_map(|g| &g.entries).filter_map(|e| if let Entry::Item { id, .. } = e { Some(id.as_str()) } else { None }).collect()
    }
    #[test]
    fn focused_menu_orders_actions_and_ordinary_or_mixed_selections_keep_cad() {
        for tags in [vec![], vec!["OTHER"], vec!["CC_SIMBOLO", "OTHER"]] {
            assert!(groups(&app(&tags)).is_none());
        }
        let g = groups(&app(&["CC_COMPOSITORE"])).unwrap();
        assert_eq!(
            g.iter().map(|g| g.label).collect::<Vec<_>>(),
            vec!["Edit electrical object", "Move and copy", "Rooms and assignments", "Delete electrical object", "Object information"]
        );
        let ids = ids(&g);
        for id in
            ["ui.elettrico.context.box", "ui.elettrico.context.box_height", "ui.elettrico.context.box_label", "move", "copy", "erase", "properties"]
        {
            assert!(ids.contains(&id));
        }
        assert!(!ids.contains(&"ui.elettrico.context.symbol"));
        let room = groups(&app(&["CC_AMBIENTE"])).unwrap();
        assert!(ids_for(&room, "ui.elettrico.context.delete"));
        assert!(!ids_for(&room, "erase"));
    }
    fn ids_for(g: &[Group], id: &str) -> bool {
        ids(g).contains(&id)
    }
    #[test]
    fn unifilar_bulk_scope_promotes_native_move_and_wire_actions_stay_specific() {
        let g = groups(&app(&["CC_UNIFILARE_DEVICE", "CC_UNIFILARE_LINK"])).unwrap();
        assert!(ids_for(&g, "ui.elettrico.unifilar.column.move"));
        for id in ["move", "copy", "rotate", "erase"] {
            assert!(!ids_for(&g, id));
        }
        let g = groups(&app(&["CC_UNIFILARE_LINK"])).unwrap();
        assert!(ids_for(&g, "ui.elettrico.unifilar.link.edit"));
        assert!(ids_for(&g, "ui.elettrico.unifilar.link.delete"));
        assert!(!ids_for(&g, "ui.elettrico.context.box"));
    }
}
