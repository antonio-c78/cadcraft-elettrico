//! New unifilar sheet: a familiar form backed by the external template.
use crate::CadApp;
use serde_json::{Value, json};

const FIELDS: [(&str, &str); 10] = [
    ("impresa", "Impresa / installatore"),
    ("recapiti", "Recapiti"),
    ("cliente", "Committente"),
    ("indirizzo", "Indirizzo impianto"),
    ("quadro", "Sigla e nome quadro"),
    ("titolo", "Titolo elaborato"),
    ("data", "Data"),
    ("revisione", "Revisione"),
    ("progetto", "Codice progetto"),
    ("foglio", "Foglio"),
];
#[derive(Default)]
pub(super) struct State {
    open: bool,
    pub(super) model: String,
    fields: std::collections::BTreeMap<String, String>,
    error: Option<String>,
}
pub(super) fn command(app: &mut CadApp, params: &Value) -> Result<Value, String> {
    if app.session.running.is_some() || app.elettrico.editing.snapshot.is_some() || app.session.state().is_ok_and(|d| d.block_edit.is_some()) {
        return Err("Termina il comando o la modifica corrente prima di creare uno schema.".into());
    }
    let state = &mut app.elettrico.unifilar;
    if let Some(path) = params.get("model").and_then(Value::as_str) {
        state.model = path.into();
    }
    if let Some(path) = cadcraft_engine::project::model(&app.session) {
        state.model = path.into();
    }
    if state.model.is_empty() {
        let catalog = app.elettrico.catalog_path.clone().or_else(super::catalog_environment);
        state.model = catalog
            .as_deref()
            .and_then(|p| std::path::Path::new(p).parent()?.parent())
            .map(|p| p.join("unifilare/v1/modello.json").to_string_lossy().into_owned())
            .unwrap_or_default();
    }
    if state.fields.is_empty() {
        state.fields.insert("titolo".into(), "Schema unifilare".into());
        state.fields.insert("revisione".into(), "00".into());
        state.fields.insert("foglio".into(), "01 / 01".into());
    }
    state.error = None;
    state.open = true;
    app.elettrico.open = false;
    app.elettrico.settings_open = false;
    app.elettrico.symbol.open = false;
    super::manager::hide(&mut app.elettrico.manager);
    super::rooms::hide(&mut app.elettrico.rooms);
    Ok(json!({"open":true}))
}
pub(super) fn show(app: &mut CadApp, ctx: &egui::Context) {
    if !app.elettrico.unifilar.open {
        return;
    }
    let mut state = std::mem::take(&mut app.elettrico.unifilar);
    let mut open = true;
    let mut create = false;
    let mut cancel = false;
    egui::Window::new("Nuovo schema unifilare — Elettrico")
        .id(egui::Id::new("electrical.unifilar.new"))
        .open(&mut open)
        .default_width(560.0)
        .resizable(false)
        .show(ctx, |ui| {
            ui.heading("Dati della tavola");
            ui.label("A3 orizzontale · 10 colonne · descrizioni su tre righe · fascia PE");
            ui.label("I campi sono facoltativi. Lo schema viene creato in una nuova scheda.");
            ui.separator();
            egui::Grid::new("unifilar.title.fields").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                for (key, label) in FIELDS {
                    ui.label(label);
                    let value = state.fields.entry(key.into()).or_default();
                    ui.add(egui::TextEdit::singleline(value).id_salt(("unifilar.field", key)).desired_width(360.0).char_limit(256));
                    ui.end_row();
                }
            });
            ui.separator();
            ui.collapsing("Modello esterno", |ui| {
                ui.label("File di configurazione del modello (modello.json)");
                ui.add(egui::TextEdit::singleline(&mut state.model).desired_width(f32::INFINITY));
            });
            if let Some(error) = &state.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.label("I testi troppo lunghi vengono segnalati: non saranno tagliati o rimpiccioliti.");
            ui.horizontal(|ui| {
                let available = app.session.running.is_none()
                    && !app.session.state().is_ok_and(|d| d.block_edit.is_some())
                    && app.elettrico.editing.snapshot.is_none();
                create = ui.add_enabled(available && !state.model.trim().is_empty(), egui::Button::new("Crea schema")).clicked();
                cancel = ui.button("Annulla").clicked();
            });
        });
    if create {
        match app.run("electrical.unifilar.new", json!({"model":state.model.trim(),"fields":state.fields})) {
            Ok(_) => {
                open = false;
                app.ui.start_tab = false;
                let _ = app.run("zoom.extents", json!({}));
            }
            Err(e) => state.error = Some(e),
        }
    }
    state.open = open && !cancel;
    app.elettrico.unifilar = state;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn form_uses_external_model_and_does_not_require_an_open_drawing() {
        let mut app = CadApp::new(cadcraft_engine::Session::empty(), crate::Services::default());
        command(&mut app, &json!({"model":"external/modello.json"})).unwrap();
        assert!(app.elettrico.unifilar.open);
        assert_eq!(app.elettrico.unifilar.model, "external/modello.json");
        assert_eq!(app.elettrico.unifilar.fields.get("foglio").map(String::as_str), Some("01 / 01"));
        assert!(crate::menus::UI_COMMANDS.iter().any(|(id, _, path, _)| *id == "ui.elettrico.unifilar.new" && path.contains(&"Schemi unifilari")));
    }
}
