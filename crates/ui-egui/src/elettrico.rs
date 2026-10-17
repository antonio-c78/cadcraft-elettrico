//! Native electrical composer. Symbol artwork remains in the user's external catalog.

use serde_json::{Value, json};

use crate::{CadApp, theme::Tokens};

const COMPONENTS: &[(&str, u8)] = &[
    ("Interruttore", 1),
    ("Deviatore", 1),
    ("Invertitore", 1),
    ("Pulsante", 1),
    ("Presa 10 A", 1),
    ("Presa UNEL / Schuko", 2),
    ("Presa TV / SAT", 1),
    ("Presa rete RJ45", 1),
    ("Copriforo", 1),
];

pub struct State {
    pub open: bool,
    pub settings_open: bool,
    capacity: u8,
    items: Vec<usize>,
    height: String,
    scale: u16,
    size_percent: u16,
}

impl Default for State {
    fn default() -> Self {
        Self { open: false, settings_open: false, capacity: 3, items: Vec::new(), height: String::new(), scale: 50, size_percent: 100 }
    }
}

impl State {
    fn used(&self) -> u8 {
        self.items.iter().filter_map(|i| COMPONENTS.get(*i)).map(|c| c.1).sum()
    }
}

pub fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    let target = match id {
        "ui.elettrico.compose" => &mut app.elettrico.open,
        "ui.elettrico.settings" => &mut app.elettrico.settings_open,
        _ => return None,
    };
    *target = params.get("open").and_then(Value::as_bool).unwrap_or(true);
    Some(Ok(json!({"open": *target})))
}

pub fn show(app: &mut CadApp, ctx: &egui::Context) {
    let state = &mut app.elettrico;
    let mut open = state.open;
    let t = Tokens::get();
    egui::Window::new("Componi scatola — Elettrico").id(egui::Id::new("elettrico.composer")).open(&mut open).default_width(640.0).show(ctx, |ui| {
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
                for (i, (name, modules)) in COMPONENTS.iter().enumerate() {
                    if ui.add_enabled(state.used() + modules <= state.capacity, egui::Button::new(format!("+ {name} · {modules} p"))).clicked() {
                        state.items.push(i);
                    }
                }
            }
            if let Some(ui) = columns.get_mut(1) {
                ui.heading("Composizione");
                ui.label(format!("{} / {} posti occupati", state.used(), state.capacity));
                let mut remove = None;
                for (position, index) in state.items.iter().enumerate() {
                    if let Some((name, modules)) = COMPONENTS.get(*index) {
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
                        if let Some((_, modules)) = COMPONENTS.get(*index) {
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
        ui.colored_label(
            t.text_dim,
            "Prima versione interna: composizione e controllo dei posti. Collegamento dei simboli e inserimento in preparazione.",
        );
    });
    state.open = open;
    egui::Window::new("Impostazioni — Elettrico").open(&mut state.settings_open).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Scala di stampa 1:");
            ui.add(egui::DragValue::new(&mut state.scale).range(1..=1000));
        });
        egui::ComboBox::from_label("Dimensione simboli").selected_text(format!("{} %", state.size_percent)).show_ui(ui, |ui| {
            for percent in (5..=300).step_by(5) {
                ui.selectable_value(&mut state.size_percent, percent, format!("{percent} %"));
            }
        });
        ui.label("Le impostazioni di questa prima versione restano attive durante la sessione.");
    });
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
