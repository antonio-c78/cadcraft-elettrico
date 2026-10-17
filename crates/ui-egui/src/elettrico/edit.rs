//! Editing keeps its original document/revision until a single confirmed replacement.
use super::*;
use cadcraft_engine::cmd::electrical::Composition;

#[derive(Default)]
pub(super) struct State {
    waiting: bool,
    pub(super) open: bool,
    snapshot: Value,
    composition: Option<Composition>,
    height: String,
    rotation: f64,
    side: u8,
    at: [f64; 2],
    mode: u8,
    selected: usize,
    replacement: usize,
    error: Option<String>,
}

pub(super) fn open(app: &mut CadApp, params: &Value) -> Result<Value, String> {
    if app.session.running.is_some() {
        return Err("Termina il comando corrente prima di modificare la scatola.".into());
    }
    if !app.elettrico.library_attempted {
        app.elettrico.library_attempted = true;
        if let Some(path) = catalog_environment() {
            let _ = route(app, "ui.elettrico.catalog", &json!({"path":path}));
        }
    }
    app.elettrico.open = false;
    if app.session.selection().is_empty() && params.get("handle").is_none() {
        app.elettrico.editing = State { waiting: true, ..Default::default() };
        app.start("electrical.select");
        return Ok(json!({"select":true}));
    }
    load(app, params)
}

fn load(app: &mut CadApp, params: &Value) -> Result<Value, String> {
    let snapshot = app.run("electrical.read", params.clone())?;
    let composition: Composition = serde_json::from_value(snapshot["composition"].clone()).map_err(|e| e.to_string())?;
    let height = composition.installation_height_cm.map(|h| h.to_string()).unwrap_or_default();
    app.elettrico.editing = State {
        open: true,
        height,
        rotation: snapshot["rotation"].as_f64().unwrap_or(0.0),
        side: snapshot["side"].as_u64().unwrap_or(0) as u8,
        at: [snapshot["at"][0].as_f64().unwrap_or(0.0), snapshot["at"][1].as_f64().unwrap_or(0.0)],
        snapshot,
        composition: Some(composition),
        ..Default::default()
    };
    Ok(json!({"open":true}))
}

pub(super) fn show(app: &mut CadApp, ctx: &egui::Context) {
    if app.elettrico.editing.waiting && app.session.running.as_ref().is_some_and(|r| r.id == "electrical.select") {
        egui::Window::new("Seleziona scatola — Elettrico").resizable(false).show(ctx, |ui| {
            ui.label("Fai clic sulla scatola da modificare nella planimetria.");
            if ui.button("Annulla").clicked() {
                app.session.cancel();
                app.elettrico.editing.waiting = false;
            }
        });
    }
    if app.elettrico.editing.waiting && app.session.running.is_none() {
        app.elettrico.editing.waiting = false;
        if !app.session.selection().is_empty()
            && let Err(e) = load(app, &json!({}))
        {
            app.elettrico.editing.error = Some(e);
            app.elettrico.editing.open = true;
        }
    }
    let mut state = std::mem::take(&mut app.elettrico.editing);
    if !state.open {
        app.elettrico.editing = state;
        return;
    }
    let mut open = state.open;
    let mut apply = false;
    let mut placement = false;
    let mut cancel = false;
    egui::Window::new("Modifica scatola — Elettrico").open(&mut open).default_width(780.0).show(ctx, |ui| {
        ui.label("L'originale rimane nel disegno fino alla conferma. Annulla lascia tutto invariato.");
        ui.label("Cambiando scheda riparti dalla scatola originale.");
        ui.horizontal_wrapped(|ui| {
            for (value, name) in [(0, "Tutta la scatola"), (1, "Un componente"), (2, "Solo altezza"), (3, "Posizione del testo")] {
                if ui.selectable_value(&mut state.mode, value, name).changed() {
                    state.error = None;
                    state.selected = 0;
                    // Each shortcut starts from the saved box, so hidden edits cannot leak into another mode.
                    state.composition = serde_json::from_value(state.snapshot["composition"].clone()).ok();
                    state.height = state.composition.as_ref().and_then(|c| c.installation_height_cm).map(|h| h.to_string()).unwrap_or_default();
                    state.rotation = state.snapshot["rotation"].as_f64().unwrap_or(0.0);
                    state.side = state.snapshot["side"].as_u64().unwrap_or(0) as u8;
                    state.at = [state.snapshot["at"][0].as_f64().unwrap_or(0.0), state.snapshot["at"][1].as_f64().unwrap_or(0.0)];
                }
            }
        });
        ui.separator();
        if let Some(c) = &mut state.composition {
            let used: u16 = c.items.iter().map(|i| u16::from(i.modules)).sum();
            if state.mode == 0 {
                ui.horizontal(|ui| {
                    ui.label("Posti disponibili nel supporto");
                    for n in [3, 4, 6, 7] {
                        ui.selectable_value(&mut c.capacity, n, n.to_string());
                    }
                });
                ui.label("6 e 7 posti restano distinti: la compatibilità della scatola da incasso dipende dalla serie.");
            }
            if state.mode <= 1 {
                ui.label(format!("{used} / {} posti occupati · seleziona il componente da cambiare", c.capacity));
                let mut remove = None;
                for (index, item) in c.items.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut state.selected, index, format!("{}. {} · {} p", index + 1, item.name, item.modules));
                        if state.mode == 0 && ui.small_button(format!("Rimuovi {}", index + 1)).clicked() {
                            remove = Some(index);
                        }
                    });
                }
                if let Some(index) = remove {
                    c.items.remove(index);
                    state.selected = state.selected.min(c.items.len().saturating_sub(1));
                }
                egui::ComboBox::from_label("Nuovo componente")
                    .selected_text(app.elettrico.catalog.components.get(state.replacement).map(|c| c.name.as_str()).unwrap_or("Carica il catalogo"))
                    .show_ui(ui, |ui| {
                        for (index, component) in app.elettrico.catalog.components.iter().enumerate() {
                            ui.selectable_value(&mut state.replacement, index, format!("{} · {} p", component.name, component.modules));
                        }
                    });
                if let Some(component) = app.elettrico.catalog.components.get(state.replacement) {
                    let occupied: u16 = c.items.iter().map(|i| u16::from(i.modules)).sum();
                    let replaced = c.items.get(state.selected).map(|i| u16::from(i.modules)).unwrap_or(0);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !c.items.is_empty() && occupied - replaced + u16::from(component.modules) <= u16::from(c.capacity),
                                egui::Button::new("Sostituisci selezionato"),
                            )
                            .clicked()
                            && let Some(item) = c.items.get_mut(state.selected)
                        {
                            *item = cadcraft_engine::cmd::electrical::Component {
                                name: component.name.clone(),
                                block: component.block.clone(),
                                modules: component.modules,
                            };
                        }
                        if state.mode == 0
                            && ui
                                .add_enabled(
                                    occupied + u16::from(component.modules) <= u16::from(c.capacity),
                                    egui::Button::new("Aggiungi componente"),
                                )
                                .clicked()
                        {
                            c.items.push(cadcraft_engine::cmd::electrical::Component {
                                name: component.name.clone(),
                                block: component.block.clone(),
                                modules: component.modules,
                            });
                        }
                        if state.mode == 0 && ui.button("Riparti da scatola vuota").clicked() {
                            c.items.clear();
                        }
                    });
                }
            }
            if state.mode == 0 || state.mode == 2 {
                ui.horizontal(|ui| {
                    ui.label("Altezza installazione (cm)");
                    ui.add(egui::TextEdit::singleline(&mut state.height).desired_width(80.0));
                    ui.label("Vuoto = nessuna scritta");
                });
            }
            if state.mode == 0 || state.mode == 3 {
                ui.horizontal(|ui| {
                    ui.label("Posizione altezza");
                    for (n, name) in [(0, "Sopra"), (1, "Sotto"), (2, "Destra"), (3, "Sinistra")] {
                        ui.selectable_value(&mut state.side, n, name);
                    }
                });
            }
            if state.mode == 0 {
                ui.horizontal(|ui| {
                    ui.label("Rotazione");
                    ui.add(egui::DragValue::new(&mut state.rotation).range(-360.0..=360.0).suffix("°"));
                });
                ui.horizontal(|ui| {
                    ui.label("Punto X / Y");
                    for v in &mut state.at {
                        ui.add(egui::DragValue::new(v));
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Dimensione di un posto (unità del disegno)");
                    let mut width = c.factor * 3.0;
                    if ui.add(egui::DragValue::new(&mut width).range(0.000015..=9000.0)).changed() {
                        c.factor = width / 3.0;
                    }
                });
            }
            ui.label("Anteprima composizione · clicca un simbolo per selezionarlo; verifica la rotazione nel disegno");
            // Reuse the composer's actual CAD symbol cache for the proposed composition.
            let indices: Option<Vec<usize>> =
                c.items.iter().map(|i| app.elettrico.catalog.components.iter().position(|v| v.block == i.block && v.modules == i.modules)).collect();
            if let Some(items) = indices {
                let preview_state = super::State {
                    capacity: c.capacity,
                    items,
                    height: state.height.clone(),
                    catalog: app.elettrico.catalog.clone(),
                    library: app.elettrico.library.clone(),
                    ..Default::default()
                };
                if let Some(selected) = preview::show_edit(ui, &preview_state, state.side) {
                    state.selected = selected;
                }
            } else {
                ui.label("Alcuni simboli non sono nel catalogo attuale. Le modifiche alla sola altezza conservano il disegno originale.");
            }
            let used: u16 = c.items.iter().map(|i| u16::from(i.modules)).sum();
            if used > u16::from(c.capacity) {
                ui.colored_label(Tokens::get().warn, "Troppi posti occupati: rimuovi o sostituisci componenti prima di confermare.");
            }
            let valid = !c.items.is_empty() && used <= u16::from(c.capacity);
            ui.horizontal(|ui| {
                apply = ui.add_enabled(valid, egui::Button::new("Applica modifica")).clicked();
                placement = ui.add_enabled(valid && state.mode == 0, egui::Button::new("Rivedi inserimento nel disegno")).clicked();
                cancel = ui.button("Annulla").clicked();
            });
        }
        if let Some(error) = &state.error {
            ui.colored_label(Tokens::get().warn, error);
        }
    });
    state.open = open && !cancel;
    if apply || placement {
        let result = (|| {
            let mut c = state.composition.clone().ok_or("Scatola non disponibile.")?;
            c.installation_height_cm = if state.height.trim().is_empty() {
                None
            } else {
                let value = state.height.trim().replace(',', ".").parse::<f64>().map_err(|_| "Altezza non valida.")?;
                if !value.is_finite() || !(0.0..=10000.0).contains(&value) {
                    return Err("Altezza ammessa: 0–10000 cm.".to_string());
                }
                Some(value)
            };
            let mut params = state.snapshot.clone();
            params["composition"] = json!(c);
            params["rotation"] = json!(state.rotation);
            params["side"] = json!(state.side);
            params["at"] = json!(state.at);
            if let Some(library) = &app.elettrico.library {
                params["library"] = json!(library.source_path);
            }
            app.run(if placement { "electrical.edit.prepare" } else { "electrical.update" }, params)?;
            if placement {
                app.start("electrical.place");
            }
            Ok::<(), String>(())
        })();
        match result {
            Ok(()) => state.open = false,
            Err(e) => state.error = Some(e),
        }
    }
    app.elettrico.editing = state;
}
