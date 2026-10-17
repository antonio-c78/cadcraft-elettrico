//! Native electrical composer. Symbol artwork remains in the user's external catalog.

use serde_json::{Value, json};

use crate::{CadApp, theme::Tokens};

mod preview;

const COMPONENTS: &[(&str, u8, &str)] = &[
    ("Interruttore", 1, "B03_02_InterruttoreUnipolare"),
    ("Deviatore", 1, "B03_08_Deviatore"),
    ("Invertitore", 1, "Invertitore"),
    ("Pulsante", 1, "B03_16_Pulsante"),
    ("Presa 10 A", 1, "Presa2P+T_10"),
    ("Presa UNEL / Schuko", 2, "B05_16_PresaSchuko"),
    ("Presa TV / SAT", 1, "PresaTVSAT"),
    ("Presa rete RJ45", 1, "PresaRJ45"),
    ("Copriforo", 1, "Copriforo 1 modulo"),
];

pub struct State {
    pub open: bool,
    pub settings_open: bool,
    capacity: u8,
    items: Vec<usize>,
    height: String,
    scale: u16,
    size_percent: u16,
    library: Option<preview::Library>,
    library_error: Option<String>,
    library_attempted: bool,
    model_meters: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            open: false,
            settings_open: false,
            capacity: 3,
            items: Vec::new(),
            height: String::new(),
            scale: 50,
            size_percent: 100,
            library: None,
            library_error: None,
            library_attempted: false,
            model_meters: false,
        }
    }
}

impl State {
    fn used(&self) -> u8 {
        self.items.iter().filter_map(|i| COMPONENTS.get(*i)).map(|c| c.1).sum()
    }
}

pub fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if id == "ui.elettrico.library" {
        let path = if params.is_null() {
            app.services.pick_open.as_ref().and_then(|pick| pick())
        } else {
            params.get("path").and_then(Value::as_str).map(str::to_string)
        };
        let Some(path) = path else {
            return Some(if params.is_null() { Ok(Value::Null) } else { Err("Indica il percorso della libreria DXF.".into()) });
        };
        app.elettrico.library_attempted = true;
        return Some(match preview::Library::load(&path) {
            Ok(library) => {
                let count = library.available();
                app.elettrico.library = Some(library);
                app.elettrico.library_error = None;
                Ok(json!({"symbols": count}))
            }
            Err(error) => {
                app.elettrico.library_error = Some(error.clone());
                Err(error)
            }
        });
    }
    let target = match id {
        "ui.elettrico.compose" => &mut app.elettrico.open,
        "ui.elettrico.settings" => &mut app.elettrico.settings_open,
        _ => return None,
    };
    *target = params.get("open").and_then(Value::as_bool).unwrap_or(true);
    Some(Ok(json!({"open": *target})))
}

pub fn show(app: &mut CadApp, ctx: &egui::Context) {
    placement_window(app, ctx);
    if app.elettrico.open && !app.elettrico.library_attempted {
        app.elettrico.library_attempted = true;
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(path) = std::env::var("CADCRAFT_ELECTRICAL_LIBRARY") {
            let _ = route(app, "ui.elettrico.library", &json!({"path": path}));
        }
    }
    let mut choose_library = false;
    let mut insert = false;
    let state = &mut app.elettrico;
    let mut open = state.open;
    let t = Tokens::get();
    egui::Window::new("Componi scatola — Elettrico")
        .id(egui::Id::new("elettrico.composer"))
        .open(&mut open)
        .default_width(820.0)
        .min_width(760.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Scatola · posti");
                let used = state.used();
                for capacity in [3, 4, 6] {
                    ui.add_enabled_ui(used <= capacity, |ui| {
                        ui.selectable_value(&mut state.capacity, capacity, capacity.to_string());
                    });
                }
                if ui.button("Impostazioni").clicked() {
                    state.settings_open = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Altezza installazione (cm)");
                ui.add(egui::TextEdit::singleline(&mut state.height).desired_width(65.0).hint_text("100"));
                ui.label("Facoltativa · vuoto = nessuna scritta");
            });
            ui.separator();
            ui.columns(2, |columns| {
                if let Some(ui) = columns.get_mut(0) {
                    ui.heading("Componenti");
                    for (i, (name, modules, _)) in COMPONENTS.iter().enumerate() {
                        if ui.add_enabled(state.used() + modules <= state.capacity, egui::Button::new(format!("+ {name} · {modules} p"))).clicked() {
                            state.items.push(i);
                        }
                    }
                }
                if let Some(ui) = columns.get_mut(1) {
                    ui.heading("Anteprima CAD");
                    preview::show(ui, state);
                    if let Some(error) = &state.library_error {
                        ui.colored_label(t.warn, error);
                    } else if state.library.is_none() {
                        ui.label("Carica la libreria simboli dalle Impostazioni.");
                    }
                    ui.heading("Composizione");
                    ui.label(format!("{} / {} posti occupati", state.used(), state.capacity));
                    let mut remove = None;
                    for (position, index) in state.items.iter().enumerate() {
                        if let Some((name, modules, _)) = COMPONENTS.get(*index) {
                            ui.horizontal(|ui| {
                                ui.label(format!("{name} · {modules} p"));
                                if ui.small_button("Rimuovi").clicked() {
                                    remove = Some(position);
                                }
                            });
                        }
                    }
                    if let Some(position) = remove {
                        state.items.remove(position);
                    }
                    if ui.button("Svuota scatola").clicked() {
                        state.items.clear();
                    }
                    ui.separator();
                    ui.label("Schema dei posti");
                    ui.horizontal(|ui| {
                        for index in &state.items {
                            if let Some((_, modules, _)) = COMPONENTS.get(*index) {
                                ui.group(|ui| {
                                    ui.label(format!("{modules} p"));
                                });
                            }
                        }
                        for _ in state.used()..state.capacity {
                            ui.group(|ui| {
                                ui.label("libero");
                            });
                        }
                    });
                }
            });
            ui.separator();
            insert = ui.add_enabled(!state.items.is_empty() && state.library.is_some(), egui::Button::new("Inserisci nella planimetria")).clicked();
            ui.colored_label(t.text_dim, "Scegli il punto nel disegno, poi rotazione e posizione dell'altezza.");
        });
    state.open = open;
    egui::Window::new("Impostazioni — Elettrico").open(&mut state.settings_open).show(ctx, |ui| {
        ui.heading("Libreria simboli");
        if let Some(library) = &state.library {
            ui.label(format!("{} · {} simboli disponibili", library.name, library.available()));
        } else {
            ui.label("Nessuna libreria caricata");
        }
        if ui.button("Scegli libreria DXF...").clicked() {
            choose_library = true;
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Scala di stampa 1:");
            ui.add(egui::DragValue::new(&mut state.scale).range(1..=1000));
        });
        egui::ComboBox::from_label("Dimensione simboli").selected_text(format!("{} %", state.size_percent)).show_ui(ui, |ui| {
            for percent in (5..=300).step_by(5) {
                ui.selectable_value(&mut state.size_percent, percent, format!("{percent} %"));
            }
        });
        ui.horizontal(|ui| {
            ui.label("Unità della planimetria");
            ui.selectable_value(&mut state.model_meters, false, "Millimetri");
            ui.selectable_value(&mut state.model_meters, true, "Metri");
        });
        ui.label("Le impostazioni di questa prima versione restano attive durante la sessione.");
    });
    if choose_library {
        let _ = route(app, "ui.elettrico.library", &Value::Null);
    }
    if insert {
        let result = (|| {
            let state = &app.elettrico;
            let height = if state.height.trim().is_empty() {
                None
            } else {
                let value =
                    state.height.trim().replace(',', ".").parse::<f64>().map_err(|_| "Altezza non valida: indica i centimetri.".to_string())?;
                if !value.is_finite() || !(0.0..=10000.0).contains(&value) {
                    return Err("Altezza non valida: indica centimetri da 0 a 10000.".to_string());
                }
                Some(value)
            };
            let library = state.library.as_ref().ok_or("Carica la libreria simboli.")?;
            let items: Vec<_> = state
                .items
                .iter()
                .filter_map(|i| COMPONENTS.get(*i))
                .map(|(name, modules, block)| json!({"name":name,"modules":modules,"block":block}))
                .collect();
            let factor = f64::from(state.scale) * f64::from(state.size_percent) / 100.0 / if state.model_meters { 1000.0 } else { 1.0 };
            let params = json!({"library":library.source_path,"composition":{"capacity":state.capacity,"items":items,"factor":factor,"installation_height_cm":height}});
            app.run("electrical.prepare", params)?;
            app.start("electrical.place");
            Ok::<(), String>(())
        })();
        match result {
            Ok(()) => {
                app.elettrico.open = false;
                app.elettrico.library_error = None;
            }
            Err(e) => app.elettrico.library_error = Some(e),
        }
    }
}

fn placement_window(app: &mut CadApp, ctx: &egui::Context) {
    if !app.session.running.as_ref().is_some_and(|r| r.id == "electrical.place") {
        if app.session.electrical.as_ref().is_some_and(|p| p.phase > 0) {
            app.session.electrical = None;
        }
        return;
    }
    let Some(p) = &app.session.electrical else { return };
    let phase = p.phase;
    let mut rotation = p.rotation;
    let mut side = p.side;
    let previous = (rotation, side);
    let error = p.error.clone();
    let mut cancel = false;
    let mut next = false;
    let mut open = true;
    egui::Window::new(match phase {
        0 => "Inserisci scatola",
        1 => "Rotazione della scatola",
        _ => "Posizione dell'altezza",
    })
    .id(egui::Id::new("elettrico.placement"))
    .open(&mut open)
    .default_pos(egui::pos2(30.0, 100.0))
    .resizable(false)
    .show(ctx, |ui| {
        match phase {
            0 => {
                ui.label("Scegli il punto nella planimetria.");
            }
            1 => {
                ui.label("Controlla l'anteprima nel disegno.");
                ui.horizontal(|ui| {
                    for angle in [0, 90, 180, 270] {
                        ui.selectable_value(&mut rotation, f64::from(angle), format!("{angle}°"));
                    }
                });
                ui.add(egui::DragValue::new(&mut rotation).range(-360.0..=360.0).suffix("°"));
            }
            _ => {
                ui.label("La scritta rimane orizzontale.");
                ui.horizontal(|ui| {
                    for (value, label) in [(0, "Sopra"), (1, "Sotto"), (2, "Destra"), (3, "Sinistra")] {
                        ui.selectable_value(&mut side, value, label);
                    }
                });
            }
        }
        if let Some(e) = &error {
            ui.colored_label(Tokens::get().warn, e);
        }
        ui.horizontal(|ui| {
            if phase > 0 {
                next = ui.button(if phase == 1 { "Conferma rotazione" } else { "Inserisci scatola" }).clicked();
            }
            cancel = ui.button("Annulla").clicked();
        });
    });
    if previous != (rotation, side) {
        let _ = app.run("electrical.options", json!({"rotation":rotation,"side":side}));
    }
    if cancel || !open {
        app.session.cancel();
        app.session.electrical = None;
        app.elettrico.open = true;
    } else if next {
        let _ = app.session.input(cadcraft_engine::Input::Enter);
        if app.session.running.is_none() {
            app.elettrico.open = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unel_uses_two_slots_and_empty_slots_stay_available() {
        let state = State { capacity: 6, items: vec![5, 5, 5], ..State::default() };
        assert_eq!(state.used(), 6);
        let state = State { capacity: 3, items: vec![5], ..State::default() };
        assert_eq!(state.capacity - state.used(), 1);
    }

    #[test]
    fn composer_opens_without_creating_or_changing_a_drawing() {
        let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        let count = app.session.docs.len();
        assert!(app.run("ui.elettrico.compose", json!({})).is_ok());
        assert!(app.elettrico.open);
        assert_eq!(app.session.docs.len(), count);
        assert!(app.run("ui.elettrico.compose", json!({"open": false})).is_ok());
        assert!(!app.elettrico.open);
    }
}
