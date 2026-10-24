//! Selection-specific electrical actions reuse the existing drafting commands and dialogs.
use super::{edit, rooms, symbol};
use crate::{CadApp, menus::Entry};
use cadcraft_engine::doc::{EntityKind, Handle, Space};
use serde_json::{Value, json};
pub(crate) mod menu;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Box,
    Symbol,
    Room,
}

#[derive(Default)]
pub(super) struct State {
    action: String,
    uid: u64,
    revision: u64,
    handles: Vec<String>,
    rooms: Vec<Value>,
    objects: Vec<Value>,
    destination: Option<String>,
    snapshot: Value,
    height: String,
    side: u8,
    error: Option<String>,
}

fn selection(app: &CadApp) -> Vec<(Handle, Kind)> {
    let Ok(doc) = app.session.doc() else { return vec![] };
    if app.session.space() != Space::Model {
        return vec![];
    }
    app.session
        .selection()
        .into_iter()
        .filter_map(|h| {
            let e = doc.entity(h)?;
            if doc.space_of(h) != Some(Space::Model) || !matches!(e.kind, EntityKind::Insert(_)) {
                return None;
            }
            let kind = e.common.xdata.iter().filter(|t| t.code == 1001).find_map(|t| match t.value.as_str() {
                "CC_COMPOSITORE" => Some(Kind::Box),
                "CC_SIMBOLO" => Some(Kind::Symbol),
                "CC_AMBIENTE" => Some(Kind::Room),
                _ => None,
            })?;
            Some((h, kind))
        })
        .collect()
}

pub(crate) fn entries(app: &CadApp) -> Vec<Entry> {
    let picked = app.session.selection();
    if picked.len() == 1
        && let Some(h) = picked.first()
        && let Ok(doc) = app.session.doc()
        && let Some(e) = doc.entity(*h)
    {
        use cadcraft_engine::cmd::electrical::unifilar::connections;
        let item = |id: &str, label: &str| Entry::Item { label: label.into(), id: id.into(), shortcut: None, enabled: app.session.running.is_none() };
        let mut children = Vec::new();
        if cadcraft_engine::project::references::is_reference(e) {
            children.push(item("ui.elettrico.project.follow", "Vai al rimando collegato"));
            children.push(item("ui.elettrico.unifilar.link.series", "Collega simboli..."));
            children.push(item("ui.elettrico.project", "Fogli di progetto..."));
        } else if e.common.xdata.iter().any(|t| t.code == 1001 && t.value == "CC_UNIFILARE_DEVICE") {
            children.push(item("ui.elettrico.unifilar.edit", "Modifica apparecchio..."));
            if connections::has_port(doc, *h, "in") || connections::has_port(doc, *h, "out") {
                children.push(item("ui.elettrico.unifilar.circuit.move", "Sposta circuito..."));
            }
            if connections::has_port(doc, *h, "out") {
                children.push(item("ui.elettrico.unifilar.link.series", "Collega simboli..."));
                children.push(item("ui.elettrico.unifilar.link.bus", "Barra di distribuzione..."));
            }
            if connections::has_port(doc, *h, "in") {
                children.push(item("ui.elettrico.unifilar.link.branch", "Derivazione da filo o barra..."));
            }
            if connections::has_port(doc, *h, "pe") {
                children.push(item("ui.elettrico.unifilar.link.pe", "Collega al PE..."));
            }
        } else if e.common.xdata.iter().any(|t| t.code == 1001 && t.value == "CC_UNIFILARE_LINK") {
            children.push(item("ui.elettrico.unifilar.link.edit", "Modifica collegamento..."));
            if connections::can_branch(doc, *h) {
                children.push(item("ui.elettrico.unifilar.link.branch", "Derivazione da filo o barra..."));
            }
            children
                .push(item("ui.elettrico.unifilar.link.delete", if connections::is_bus(doc, *h) { "Elimina barra..." } else { "Elimina tratto..." }));
        }
        if !children.is_empty() {
            return vec![Entry::Sub { label: "Elettrico".into(), children }];
        }
    }
    if picked.len() > 1
        && app.session.doc().is_ok_and(|doc| {
            picked.iter().any(|h| doc.entity(*h).is_some_and(|e| e.common.xdata.iter().any(|t| t.code == 1001 && t.value == "CC_UNIFILARE_DEVICE")))
        })
    {
        return vec![Entry::Sub {
            label: "Elettrico".into(),
            children: vec![Entry::Item {
                label: "Sposta circuito...".into(),
                id: "ui.elettrico.unifilar.circuit.move".into(),
                shortcut: None,
                enabled: app.session.running.is_none(),
            }],
        }];
    }
    let selected = selection(app);
    if selected.is_empty() || selected.len() != app.session.selection().len() {
        return vec![];
    }
    let item = |id: &str, label: &str| Entry::Item {
        label: label.into(),
        id: format!("ui.elettrico.context.{id}"),
        shortcut: None,
        enabled: app.session.running.is_none() && app.elettrico.editing.snapshot.is_none(),
    };
    let mut children = vec![];
    if selected.len() == 1 {
        match selected.first().map(|(_, k)| k) {
            Some(Kind::Box) => {
                children.push(item("box", "Modifica scatola..."));
                children.push(item("box_height", "Modifica altezza..."));
                children.push(item("box_label", "Posiziona testo altezza..."));
            }
            Some(Kind::Symbol) => {
                children.push(item("symbol", "Sostituisci simbolo..."));
                children.push(item("symbol_height", "Modifica altezza..."));
                children.push(item("symbol_label", "Posiziona testo altezza..."));
            }
            Some(Kind::Room) => {
                children.push(item("room", "Modifica ambiente..."));
                children.push(item("redraw", "Ridisegna perimetro..."));
                children.push(item("members", "Mostra componenti assegnati"));
                children.push(item("delete", "Elimina ambiente..."));
            }
            None => {}
        }
    }
    if selected.iter().all(|(_, k)| *k != Kind::Room) {
        children.push(item("info", "Ambiente di appartenenza..."));
        children.push(item("assign", "Assegna / cambia ambiente..."));
        children.push(item("auto", "Ripristina assegnazione automatica..."));
    }
    // Keep MOVE/ROTATE in the CAD's usual menu section, with their familiar behavior.
    if children.is_empty() { vec![] } else { vec![Entry::Sub { label: "Elettrico".into(), children }] }
}

pub(super) fn command(app: &mut CadApp, id: &str) -> Result<Value, String> {
    if app.session.running.is_some() || app.elettrico.editing.snapshot.is_some() {
        return Err("Termina il comando o la modifica corrente.".into());
    }
    let selected = selection(app);
    if selected.is_empty() || selected.len() != app.session.selection().len() {
        return Err("Seleziona gli oggetti Elettrico nel disegno.".into());
    }
    let action = id.strip_prefix("ui.elettrico.context.").ok_or("Azione non valida.")?;
    let first = selected.first().ok_or("Seleziona un oggetto.")?;
    let handle = first.0.hex();
    app.elettrico.context = State::default();
    app.elettrico.open = false;
    app.elettrico.settings_open = false;
    app.elettrico.symbol.open = false;
    super::manager::hide(&mut app.elettrico.manager);
    rooms::hide(&mut app.elettrico.rooms);
    if matches!(action, "box" | "box_height" | "box_label") {
        if selected.len() != 1 || first.1 != Kind::Box {
            return Err("Seleziona una sola scatola composta.".into());
        }
        edit::open(app, &json!({"handle":handle}))?;
        app.elettrico.editing.mode = match action {
            "box_height" => edit::Mode::Height,
            "box_label" => edit::Mode::Label,
            _ => edit::Mode::Whole,
        };
        return Ok(json!({"open":true}));
    }
    if matches!(action, "room" | "redraw" | "members" | "delete") {
        if selected.len() != 1 || first.1 != Kind::Room {
            return Err("Seleziona un solo ambiente.".into());
        }
        if matches!(action, "room" | "redraw") {
            rooms::start(app, Some(handle))?;
            if action == "redraw" {
                app.run("electrical.room.options", json!({"action":"redraw"}))?;
            }
            return Ok(json!({"open":true}));
        }
    } else if !matches!(action, "symbol" | "symbol_height" | "symbol_label" | "info" | "assign" | "auto") {
        return Err("Azione non disponibile.".into());
    } else if selected.iter().any(|(_, k)| *k == Kind::Room) {
        return Err("Seleziona scatole o simboli.".into());
    }
    if matches!(action, "symbol" | "symbol_height" | "symbol_label") && (selected.len() != 1 || first.1 != Kind::Symbol) {
        return Err("Seleziona un solo simbolo singolo.".into());
    }
    if action == "symbol" {
        return symbol::edit(app, &json!({"handle":handle}));
    }
    let doc = app.session.state().map_err(|e| e.to_string())?;
    let mut state = State {
        action: action.into(),
        uid: doc.uid,
        revision: doc.revision,
        handles: selected.iter().map(|(h, _)| h.hex()).collect(),
        ..Default::default()
    };
    if matches!(action, "symbol_height" | "symbol_label") {
        state.snapshot = app.run("electrical.symbol.read", json!({"handle":handle}))?;
        state.height = state.snapshot["installation_height_cm"].as_f64().map(|h| h.to_string()).unwrap_or_default();
        state.side = state.snapshot["side"].as_u64().unwrap_or(0) as u8;
    } else {
        state.rooms = app.run("electrical.room.list", json!({}))?["rooms"].as_array().cloned().unwrap_or_default();
        state.objects = app.run("electrical.room.scan", json!({}))?["components"].as_array().cloned().unwrap_or_default();
        if action == "members" {
            let handles = state
                .objects
                .iter()
                .filter(|o| o["room"].as_str() == Some(&handle) && o["status"] != "missing_room")
                .filter_map(|o| o["handle"].as_str().and_then(Handle::parse_hex))
                .collect::<Vec<_>>();
            app.session.echo(format!("{} componenti assegnati all'ambiente.", handles.len()));
            app.session.set_selection(handles);
            return Ok(json!({"selected":true}));
        }
        state.objects.retain(|o| o["handle"].as_str().is_some_and(|h| state.handles.iter().any(|s| s == h)));
        if state.objects.len() == 1 {
            state.destination = state.objects.first().and_then(|o| o["room"].as_str()).map(str::to_string);
        }
        if action == "delete" {
            state.destination = Some(handle);
        }
    }
    app.elettrico.context = state;
    Ok(json!({"open":true}))
}

fn current(app: &CadApp, state: &State) -> Result<(), String> {
    if !app.session.state().is_ok_and(|d| d.uid == state.uid && d.revision == state.revision) {
        return Err("Il disegno è cambiato. Chiudi e riapri questa finestra.".into());
    }
    if app.session.running.is_some() {
        return Err("Termina il comando corrente.".into());
    }
    Ok(())
}

fn room_label(room: &Value) -> String {
    let name = room["name"].as_str().unwrap_or("");
    let floor = room["floor"].as_str().unwrap_or("").trim();
    if floor.is_empty() { name.into() } else { format!("{name} · {floor}") }
}

pub(super) fn show(app: &mut CadApp, ctx: &egui::Context) {
    if app.elettrico.context.action.is_empty() {
        return;
    }
    let mut state = std::mem::take(&mut app.elettrico.context);
    let mut open = true;
    let mut close = false;
    let mut action = None;
    let title = match state.action.as_str() {
        "symbol_height" => "Altezza di installazione — Elettrico",
        "symbol_label" => "Posizione testo altezza — Elettrico",
        "delete" => "Elimina ambiente — Elettrico",
        "auto" => "Assegnazione automatica — Elettrico",
        _ => "Ambiente di appartenenza — Elettrico",
    };
    egui::Window::new(title).id(egui::Id::new("electrical.context.dialog")).open(&mut open).default_width(440.0).show(ctx, |ui| {
        if state.action.starts_with("symbol_") {
            ui.label(state.snapshot["symbol"]["name"].as_str().unwrap_or("Simbolo"));
            if state.action == "symbol_height" {
                ui.label("Altezza di installazione (cm), facoltativa");
                ui.add(egui::TextEdit::singleline(&mut state.height).char_limit(32));
                ui.label("Vuoto: nessuna scritta.");
            } else {
                ui.label(format!("Altezza: {}", if state.height.is_empty() { "non indicata" } else { &state.height }));
            }
            ui.horizontal(|ui| {
                ui.label("Posizione testo");
                for (side, label) in ["Sopra", "Sotto", "Destra", "Sinistra"].iter().enumerate() {
                    ui.selectable_value(&mut state.side, side as u8, *label);
                }
            });
            if ui.button("Applica").clicked() {
                action = Some("symbol");
            }
        } else if state.action == "delete" {
            let name =
                state.rooms.iter().find(|r| r["handle"].as_str() == state.destination.as_deref()).and_then(|r| r["name"].as_str()).unwrap_or("");
            ui.label(format!("Eliminare l'ambiente «{name}»?"));
            ui.label("I componenti restano nel disegno. Le assegnazioni manuali saranno da verificare.");
            if ui.button("Elimina ambiente").clicked() {
                action = Some("delete");
            }
        } else {
            ui.label(if state.handles.len() == 1 { "1 oggetto selezionato".into() } else { format!("{} oggetti selezionati", state.handles.len()) });
            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                for (index, object) in state.objects.iter().enumerate() {
                    let room = state.rooms.iter().find(|r| r["handle"] == object["room"]);
                    let status = match object["status"].as_str() {
                        Some("manual") => "manuale",
                        Some("automatic") => "automatica",
                        Some("boundary") => "sul confine",
                        Some("overlap") => "ambienti sovrapposti",
                        Some("missing_room") => "ambiente eliminato",
                        _ => "fuori dagli ambienti",
                    };
                    let name = room.map(room_label).unwrap_or("Nessun ambiente valido".into());
                    ui.label(format!("Oggetto {}: {name} · {status}", index + 1));
                }
            });
            if state.action == "auto" {
                ui.label("Rimuove l'assegnazione manuale e usa la posizione nel perimetro degli ambienti.");
                if ui.button("Ripristina automatica").clicked() {
                    action = Some("auto");
                }
            } else {
                ui.separator();
                ui.label("Ambiente");
                egui::ScrollArea::vertical().id_salt("electrical.context.rooms").max_height(220.0).show(ui, |ui| {
                    for room in &state.rooms {
                        let h = room["handle"].as_str().unwrap_or("");
                        if ui.selectable_label(state.destination.as_deref() == Some(h), room_label(room)).clicked() {
                            state.destination = Some(h.into());
                        }
                    }
                });
                let chosen = state.destination.as_ref().is_some_and(|h| state.rooms.iter().any(|r| r["handle"].as_str() == Some(h)));
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(chosen, egui::Button::new("Assegna ambiente")).clicked() {
                        action = Some("assign");
                    }
                    if ui.add_enabled(chosen, egui::Button::new("Evidenzia ambiente")).clicked() {
                        action = Some("highlight");
                    }
                    if ui.add_enabled(chosen, egui::Button::new("Modifica ambiente...")).clicked() {
                        action = Some("edit_room");
                    }
                });
            }
        }
        if let Some(error) = &state.error {
            ui.colored_label(super::Tokens::get().warn, error);
        }
        close = ui.button("Chiudi").clicked();
    });
    open &= !close;
    if let Some(action) = action {
        let result = (|| {
            current(app, &state)?;
            match action {
                "symbol" => {
                    let mut p = state.snapshot.clone();
                    p["height_cm"] = json!(symbol::parse_height(&state.height)?);
                    p["side"] = json!(state.side);
                    app.run("electrical.symbol.update", p)?;
                }
                "assign" | "auto" => {
                    app.run(
                        "electrical.room.assign",
                        json!({"handles":state.handles,"room":if action=="auto"{None}else{state.destination.clone()}}),
                    )?;
                }
                "delete" => {
                    app.run("electrical.room.delete", json!({"handle":state.destination}))?;
                }
                "highlight" => {
                    let h = state.destination.as_deref().and_then(Handle::parse_hex).ok_or("Ambiente non disponibile.")?;
                    app.session.set_selection(vec![h]);
                }
                "edit_room" => rooms::start(app, state.destination.clone())?,
                _ => {}
            }
            Ok::<(), String>(())
        })();
        match result {
            Ok(()) => open = false,
            Err(e) => state.error = Some(e),
        }
    }
    if !open {
        state = State::default();
    }
    app.elettrico.context = state;
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_engine::{
        Session,
        doc::{Common, Insert, RawTag},
        geom::Vec3,
    };

    fn add(app: &mut CadApp, marker: &str) -> Handle {
        let mut common = Common::default();
        common.xdata.push(RawTag { code: 1001, value: marker.into() });
        app.session
            .doc_mut()
            .unwrap()
            .add(
                &Space::Model,
                common,
                EntityKind::Insert(Insert {
                    block: "Synthetic".into(),
                    insert: Vec3::ZERO,
                    scale: Vec3::new(1.0, 1.0, 1.0),
                    rotation: 0.0,
                    attribs: vec![],
                    cols: 1,
                    rows: 1,
                    col_spacing: 0.0,
                    row_spacing: 0.0,
                }),
            )
            .unwrap()
    }
    fn ids(app: &CadApp) -> Vec<String> {
        entries(app)
            .into_iter()
            .flat_map(|entry| match entry {
                Entry::Sub { children, .. } => children,
                _ => vec![],
            })
            .filter_map(|e| match e {
                Entry::Item { id, .. } => Some(id),
                _ => None,
            })
            .collect()
    }
    #[test]
    fn circuit_move_is_available_for_multiple_devices_and_window_selected_wires() {
        let mut app = CadApp::new(Session::new(), crate::Services::default());
        let a = add(&mut app, "CC_UNIFILARE_DEVICE");
        let b = add(&mut app, "CC_UNIFILARE_DEVICE");
        let wire = add(&mut app, "CC_UNIFILARE_LINK");
        app.session.set_selection(vec![a, b, wire]);
        assert!(ids(&app).contains(&"ui.elettrico.unifilar.circuit.move".into()));
    }
    #[test]
    fn menu_is_specific_to_native_objects_and_bulk_actions_are_shared() {
        let mut app = CadApp::new(Session::new(), crate::Services::default());
        let b = add(&mut app, "CC_COMPOSITORE");
        let s = add(&mut app, "CC_SIMBOLO");
        let r = add(&mut app, "CC_AMBIENTE");
        let ordinary = add(&mut app, "OTHER");
        assert!(entries(&app).is_empty());
        for (handle, expected, absent) in [(b, "box", "symbol"), (s, "symbol", "box"), (r, "room", "assign")] {
            app.session.set_selection(vec![handle]);
            let menu = ids(&app);
            assert!(menu.contains(&format!("ui.elettrico.context.{expected}")));
            assert!(!menu.contains(&format!("ui.elettrico.context.{absent}")));
            if handle != r {
                assert!(menu.contains(&"ui.elettrico.context.info".into()));
            }
        }
        app.session.set_selection(vec![b, s]);
        let menu = ids(&app);
        assert_eq!(menu.len(), 3);
        assert!(menu.contains(&"ui.elettrico.context.assign".into()));
        assert!(command(&mut app, "ui.elettrico.context.box").is_err());
        app.session.set_selection(vec![b, ordinary]);
        assert!(entries(&app).is_empty());
        app.session.set_selection(vec![b, r]);
        assert!(entries(&app).is_empty());
        app.session.set_selection(vec![ordinary]);
        assert!(command(&mut app, "ui.elettrico.context.assign").is_err());
        let state = State { uid: app.session.state().unwrap().uid, revision: app.session.state().unwrap().revision, ..Default::default() };
        assert!(current(&app, &state).is_ok());
        app.session.new_drawing(true);
        assert!(current(&app, &state).is_err());
    }
}
