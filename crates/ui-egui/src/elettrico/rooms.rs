//! Guided room boundaries and a small manager; all drawing changes use engine commands.
use crate::CadApp;
use cadcraft_engine::doc::Handle;
use serde_json::{Value, json};

#[derive(Default)]
pub(super) struct State {
    open: bool,
    active: bool,
    name: String,
    floor: String,
    sent_name: String,
    sent_floor: String,
    error: Option<String>,
    rows: Vec<Value>,
    assignments: Vec<Value>,
    selected: Option<String>,
    cad_selection: Option<Vec<Handle>>,
    document: Option<(u64, u64)>,
}

impl State {
    fn sync_selection(&mut self, selection: Vec<Handle>) {
        if self.cad_selection.as_ref() == Some(&selection) {
            return;
        }
        let mut rooms = self.rows.iter().filter_map(|row| {
            let text = row["handle"].as_str()?;
            let handle = Handle::parse_hex(text)?;
            selection.contains(&handle).then_some(text)
        });
        if let Some(room) = rooms.next()
            && rooms.next().is_none()
        {
            self.selected = Some(room.into());
        }
        // Keep the destination room when selecting components for manual assignment.
        self.cad_selection = Some(selection);
    }
}

fn refresh(app: &mut CadApp) -> Result<(), String> {
    let current = app.session.state().map_err(|e| e.to_string())?;
    let identity = (current.uid, current.revision);
    if app.elettrico.rooms.document.is_none_or(|old| old.0 != identity.0) {
        app.elettrico.rooms.selected = None;
        app.elettrico.rooms.cad_selection = None;
    }
    app.elettrico.rooms.document = Some(identity);
    app.elettrico.rooms.assignments.clear();
    let rooms = app.run("electrical.room.list", json!({}))?;
    app.elettrico.rooms.rows = rooms["rooms"].as_array().cloned().unwrap_or_default();
    if !app.elettrico.rooms.rows.iter().any(|row| row["handle"].as_str() == app.elettrico.rooms.selected.as_deref()) {
        app.elettrico.rooms.selected = None;
    }
    let scan = app.run("electrical.room.scan", json!({}))?;
    app.elettrico.rooms.assignments = scan["components"].as_array().cloned().unwrap_or_default();
    Ok(())
}

pub(super) fn start(app: &mut CadApp, handle: Option<String>) -> Result<(), String> {
    let params = if let Some(handle) = handle { json!({"handle":handle}) } else { json!({"label_height":2.0*app.elettrico.settings_factor()}) };
    app.run("electrical.room.prepare", params)?;
    app.elettrico.rooms.active = false;
    app.elettrico.rooms.open = false;
    app.elettrico.rooms.error = None;
    app.elettrico.open = false;
    app.elettrico.symbol.open = false;
    super::manager::hide(&mut app.elettrico.manager);
    app.start("electrical.room.place");
    Ok(())
}

pub(super) fn hide(state: &mut State) {
    state.open = false;
}

pub(super) fn command(app: &mut CadApp, id: &str) -> Result<Value, String> {
    if app.session.running.is_some() || app.elettrico.editing.snapshot.is_some() || app.ui.start_tab || app.session.docs.is_empty() {
        return Err("Apri il disegno e termina il comando o la modifica corrente.".into());
    }
    if id == "ui.elettrico.room" {
        start(app, None)?;
    } else {
        refresh(app)?;
        app.elettrico.rooms.cad_selection = None;
        app.elettrico.rooms.open = true;
        app.elettrico.open = false;
        app.elettrico.symbol.open = false;
        super::manager::hide(&mut app.elettrico.manager);
    }
    Ok(json!({"open":true}))
}

pub(super) fn show(app: &mut CadApp, ctx: &egui::Context) {
    let running = app.session.running.as_ref().is_some_and(|r| r.id == "electrical.room.place");
    if running {
        let Some(d) = &app.session.electrical_room else { return };
        let naming = d.naming;
        let count = d.points.len();
        let height = d.label_height;
        let mut state = std::mem::take(&mut app.elettrico.rooms);
        if !state.active || d.name != state.sent_name || d.floor != state.sent_floor {
            state.name = d.name.clone();
            state.floor = d.floor.clone();
            state.sent_name = d.name.clone();
            state.sent_floor = d.floor.clone();
        }
        state.active = true;
        let mut open = true;
        let mut action = None;
        let mut save = false;
        let mut cancel = false;
        let mut label_height = height;
        egui::Window::new(if naming { "Dati ambiente" } else { "Definisci ambiente" })
            .id(egui::Id::new("electrical.room.prompt"))
            .default_pos(egui::pos2(30.0, 145.0))
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                if naming {
                    ui.label("Nome locale");
                    ui.add(egui::TextEdit::singleline(&mut state.name).char_limit(160));
                    ui.label("Piano (facoltativo)");
                    ui.add(egui::TextEdit::singleline(&mut state.floor).char_limit(80));
                    ui.horizontal(|ui| {
                        ui.label("Altezza testo nel disegno");
                        ui.add(egui::DragValue::new(&mut label_height).range(0.00001..=1e6));
                    });
                    ui.label("Contorno e nome su EL_AMBIENTI, inizialmente escluso dalla stampa.");
                    if ui.button("Ridisegna perimetro").clicked() {
                        action = Some("redraw");
                    }
                    save = ui.add_enabled(!state.name.trim().is_empty(), egui::Button::new("Salva ambiente")).clicked();
                } else {
                    ui.label("Fai clic sui vertici seguendo il perimetro del locale.");
                    ui.label(format!("{count} vertici · Invio o clic sul primo punto per chiudere."));
                    ui.horizontal(|ui| {
                        if ui.add_enabled(count >= 3, egui::Button::new("Chiudi perimetro")).clicked() {
                            action = Some("close");
                        }
                        if ui.add_enabled(count > 0, egui::Button::new("Annulla ultimo vertice")).clicked() {
                            action = Some("undo");
                        }
                    });
                }
                if let Some(e) = &state.error {
                    ui.colored_label(super::Tokens::get().warn, e);
                }
                cancel = ui.button("Annulla").clicked();
            });
        let changed = state.name != state.sent_name || state.floor != state.sent_floor || label_height != height;
        let fields = json!({"name":state.name,"floor":state.floor,"label_height":label_height});
        state.sent_name = state.name.clone();
        state.sent_floor = state.floor.clone();
        app.elettrico.rooms = state;
        let result = (|| {
            if cancel || !open {
                app.session.cancel();
                return Ok(());
            }
            if changed {
                app.run("electrical.room.options", fields)?;
            }
            if let Some(action) = action {
                app.run("electrical.room.options", json!({"action":action}))?;
            }
            if save {
                app.session.input(cadcraft_engine::Input::Enter).map_err(|e| e.to_string())?;
            }
            Ok::<(), String>(())
        })();
        match result {
            Err(e) => app.elettrico.rooms.error = Some(e),
            Ok(()) if changed || action.is_some() || save => app.elettrico.rooms.error = None,
            _ => {}
        }
        return;
    }
    if app.elettrico.rooms.active {
        app.elettrico.rooms.active = false;
        app.session.electrical_room = None;
        app.elettrico.rooms.open = true;
        if let Err(e) = refresh(app) {
            app.elettrico.rooms.error = Some(e);
        }
    }
    if !app.elettrico.rooms.open {
        return;
    }
    if app.session.state().is_ok_and(|d| app.elettrico.rooms.document != Some((d.uid, d.revision)))
        && let Err(e) = refresh(app)
    {
        app.elettrico.rooms.error = Some(e);
    }
    let mut state = std::mem::take(&mut app.elettrico.rooms);
    state.sync_selection(app.session.selection());
    let mut open = true;
    let mut action = None;
    let mut pick = None;
    egui::Window::new("Ambienti — Elettrico").open(&mut open).default_width(650.0).show(ctx, |ui| {
        ui.label("Seleziona un ambiente per modificarlo o assegnargli i componenti selezionati nel CAD.");
        egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
            for row in &state.rows {
                let h = row["handle"].as_str().unwrap_or("");
                let count = state.assignments.iter().filter(|v| v["room"].as_str() == Some(h) && v["status"] != "missing_room").count();
                let label = format!(
                    "{}   {}   · {count} {}",
                    row["name"].as_str().unwrap_or(""),
                    row["floor"].as_str().unwrap_or(""),
                    if count == 1 { "oggetto" } else { "oggetti" }
                );
                if ui.selectable_label(state.selected.as_deref() == Some(h), label).clicked() {
                    state.selected = Some(h.into());
                }
            }
        });
        let unresolved = state.assignments.iter().filter(|v| !matches!(v["status"].as_str(), Some("automatic" | "manual"))).count();
        ui.label(format!(
            "{} {} · {unresolved} {} da assegnare o verificare",
            state.rows.len(),
            if state.rows.len() == 1 { "ambiente" } else { "ambienti" },
            if unresolved == 1 { "oggetto" } else { "oggetti" }
        ));
        ui.label("I conteggi indicano scatole e simboli, non i singoli frutti contenuti.");
        ui.collapsing("Oggetti da verificare", |ui| {
            egui::ScrollArea::vertical().id_salt("room_unresolved").max_height(150.0).show(ui, |ui| {
                for object in &state.assignments {
                    let reason = match object["status"].as_str() {
                        Some("boundary") => "sul confine",
                        Some("overlap") => "ambienti sovrapposti",
                        Some("outside") => "fuori dagli ambienti",
                        Some("missing_room") => "ambiente eliminato",
                        _ => continue,
                    };
                    let handle = object["handle"].as_str().unwrap_or("");
                    ui.horizontal(|ui| {
                        ui.label(format!("Componente: {reason}"));
                        if ui.button("Seleziona").clicked() {
                            pick = Some(handle.to_string());
                        }
                    });
                }
            });
        });
        ui.horizontal(|ui| {
            if ui.button("Nuovo ambiente").clicked() {
                action = Some("new");
            }
            if ui.add_enabled(state.selected.is_some(), egui::Button::new("Modifica ambiente")).clicked() {
                action = Some("edit");
            }
            if ui.button("Aggiorna lettura").clicked() {
                action = Some("refresh");
            }
        });
        ui.horizontal(|ui| {
            if ui.add_enabled(state.selected.is_some(), egui::Button::new("Assegna selezionati")).clicked() {
                action = Some("assign");
            }
            if ui.button("Assegnazione automatica").clicked() {
                action = Some("auto");
            }
            if ui.add_enabled(state.selected.is_some(), egui::Button::new("Elimina ambiente")).clicked() {
                action = Some("delete");
            }
        });
        if let Some(e) = &state.error {
            ui.colored_label(super::Tokens::get().warn, e);
        }
    });
    state.open = open;
    let selected = state.selected.clone();
    app.elettrico.rooms = state;
    if let Some(handle) = pick.and_then(|h| cadcraft_engine::doc::Handle::parse_hex(&h)) {
        app.session.set_selection(vec![handle]);
    }
    if let Some(action) = action {
        let result = (|| {
            if app.session.running.is_some() {
                return Err("Termina il comando corrente.".into());
            }
            match action {
                "new" => return start(app, None),
                "edit" => return start(app, selected),
                "assign" => {
                    app.run("electrical.room.assign", json!({"room":selected}))?;
                }
                "auto" => {
                    app.run("electrical.room.assign", json!({"room":null}))?;
                }
                "delete" => {
                    app.run("electrical.room.delete", json!({"handle":selected}))?;
                    app.elettrico.rooms.selected = None;
                }
                _ => {}
            }
            refresh(app)
        })();
        app.elettrico.rooms.error = result.err();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cad_room_selection_respects_list_choice_and_component_assignment() {
        let first = Handle::parse_hex("102").expect("test handle");
        let second = Handle::parse_hex("105").expect("test handle");
        let component = Handle::parse_hex("12E").expect("test handle");
        let mut state = State { rows: vec![json!({"handle":"102"}), json!({"handle":"105"})], ..State::default() };
        state.sync_selection(vec![first]);
        assert_eq!(state.selected.as_deref(), Some("102"));
        state.selected = Some("105".into());
        state.sync_selection(vec![first]);
        assert_eq!(state.selected.as_deref(), Some("105"));
        state.sync_selection(vec![component]);
        assert_eq!(state.selected.as_deref(), Some("105"));
        state.sync_selection(vec![first, second]);
        assert_eq!(state.selected.as_deref(), Some("105"));
        state.sync_selection(vec![first]);
        assert_eq!(state.selected.as_deref(), Some("102"));
        state.selected = None;
        state.cad_selection = None;
        state.sync_selection(vec![first]);
        assert_eq!(state.selected.as_deref(), Some("102"));
    }
}
