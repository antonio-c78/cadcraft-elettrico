//! Standalone insertion shares the catalog gallery but keeps independent drafting settings.
use super::manager;
use crate::CadApp;
use serde_json::{Value, json};

#[derive(Default)]
pub(super) struct State {
    pub open: bool,
    browser: manager::State,
    settings_open: bool,
    scale: u16,
    percent: u16,
    meters: bool,
    pub height: String,
    placing: bool,
    error: Option<String>,
}

pub(super) fn parse_height(text: &str) -> Result<Option<f64>, String> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    if text.len() > 32 {
        return Err("Altezza troppo lunga.".into());
    }
    text.trim()
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && (0.0..=10000.0).contains(v))
        .map(Some)
        .ok_or("Indica un'altezza da 0 a 10000 cm, oppure lascia vuoto.".into())
}

fn inherit(app: &mut CadApp) {
    app.elettrico.symbol.scale = app.elettrico.scale;
    app.elettrico.symbol.percent = app.elettrico.size_percent;
    app.elettrico.symbol.meters = app.elettrico.model_meters;
}

pub(super) fn command(app: &mut CadApp, id: &str, p: &Value) -> Result<Value, String> {
    if app.session.running.is_some() || app.elettrico.editing.snapshot.is_some() {
        return Err("Termina il comando o la modifica della scatola.".into());
    }
    if app.elettrico.catalog_path.is_none() {
        super::route(app, "ui.elettrico.catalog", &json!({})).ok_or("Catalogo non disponibile.")??;
    }
    if id == "ui.elettrico.symbols" {
        if app.elettrico.symbol.scale == 0 {
            inherit(app);
        }
        if p.get("action").and_then(Value::as_str) == Some("read") {
            let s = &app.elettrico.symbol;
            return Ok(json!({"open":s.open,"scale":s.scale,"size_percent":s.percent,"units":if s.meters{"m"}else{"mm"}}));
        }
        if let Some(settings) = p.get("settings") {
            let scale = settings.get("scale").and_then(Value::as_u64).filter(|n| (1..=1000).contains(n)).ok_or("Scala non valida.")?;
            let percent = settings
                .get("size_percent")
                .and_then(Value::as_u64)
                .filter(|n| (5..=300).contains(n) && n.is_multiple_of(5))
                .ok_or("Percentuale non valida.")?;
            let units = settings.get("units").and_then(Value::as_str).filter(|v| ["mm", "m"].contains(v)).ok_or("Unità non valida.")?;
            app.elettrico.symbol.scale = scale as u16;
            app.elettrico.symbol.percent = percent as u16;
            app.elettrico.symbol.meters = units == "m";
        } else {
            inherit(app);
        }
        app.elettrico.symbol.browser = manager::insertion_browser(&app.elettrico);
        app.elettrico.symbol.open = p.get("open").and_then(Value::as_bool).unwrap_or(true);
        app.elettrico.open = false;
        app.elettrico.settings_open = false;
        manager::hide(&mut app.elettrico.manager);
        return Ok(json!({"open":app.elettrico.symbol.open}));
    }
    if app.elettrico.symbol.scale == 0 {
        inherit(app);
    }
    let id = p.get("id").and_then(Value::as_str).ok_or("Indica il simbolo.")?;
    let c = app.elettrico.catalog.components.iter().find(|c| c.id == id && c.standalone).ok_or("Simbolo non disponibile per inserimento singolo.")?;
    let source = app.elettrico.library.as_ref().ok_or("Carica il catalogo.")?.source_path.clone();
    let state = &app.elettrico.symbol;
    let factor = f64::from(state.scale) * f64::from(state.percent) / 100.0 / if state.meters { 1000.0 } else { 1.0 };
    let params = json!({"library":source,"layers":app.elettrico.catalog.extra.get("layers"),"symbol":{"id":c.id,"name":c.name,"block":c.block,"category":c.category,"factor":factor,"width_mm":c.width_mm,"height_mm":c.height_mm,"anchor":c.anchor}});
    app.run("electrical.symbol.prepare", params)?;
    app.elettrico.symbol.open = false;
    app.elettrico.symbol.error = None;
    app.elettrico.symbol.placing = true;
    app.start("electrical.symbol.place");
    Ok(json!({"prepared":true}))
}

pub(super) fn show(app: &mut CadApp, ctx: &egui::Context) {
    if app.elettrico.symbol.placing && app.session.running.is_none() {
        app.elettrico.symbol.placing = false;
        app.elettrico.symbol.open = true;
        if app.session.electrical.as_ref().is_some_and(|p| p.is_symbol()) {
            app.session.electrical = None;
        }
    }
    if !app.elettrico.symbol.open {
        return;
    }
    let mut state = std::mem::take(&mut app.elettrico.symbol);
    let mut open = state.open;
    let mut choose = None;
    let mut reset = false;
    egui::Window::new("Inserisci simbolo — Elettrico")
        .id(egui::Id::new("electrical.symbol.gallery"))
        .open(&mut open)
        .default_width(1080.0)
        .min_width(880.0)
        .default_height(720.0)
        .max_height(740.0)
        .vscroll(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Impostazioni").clicked() {
                    state.settings_open = true;
                }
                ui.label(format!("Scala 1:{} · dimensione {}% · {}", state.scale, state.percent, if state.meters { "m" } else { "mm" }));
            });
            ui.separator();
            choose = manager::browser(ui, &mut state.browser, &app.elettrico);
            if let Some(e) = &state.error {
                ui.colored_label(super::Tokens::get().warn, e);
            }
        });
    egui::Window::new("Impostazioni simboli singoli").open(&mut state.settings_open).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Scala di stampa 1:");
            ui.add(egui::DragValue::new(&mut state.scale).range(1..=1000));
        });
        egui::ComboBox::from_label("Dimensione simboli").selected_text(format!("{} %", state.percent)).show_ui(ui, |ui| {
            for value in (5..=300).step_by(5) {
                ui.selectable_value(&mut state.percent, value, format!("{value} %"));
            }
        });
        ui.horizontal(|ui| {
            ui.label("Unità modello");
            ui.selectable_value(&mut state.meters, false, "mm");
            ui.selectable_value(&mut state.meters, true, "m");
        });
        reset = ui.button("Usa impostazioni del compositore").clicked();
    });
    state.open = open;
    app.elettrico.symbol = state;
    if reset {
        inherit(app);
    }
    if let Some(id) = choose
        && let Err(e) = app.run("ui.elettrico.symbol.insert", json!({"id":id}))
    {
        app.elettrico.symbol.error = Some(e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standalone_settings_inherit_but_do_not_change_the_composer_and_height_is_optional() {
        let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        app.elettrico.catalog_path = Some("synthetic.json".into());
        app.elettrico.scale = 75;
        app.elettrico.size_percent = 25;
        app.elettrico.model_meters = true;
        command(&mut app, "ui.elettrico.symbols", &json!({})).unwrap();
        assert_eq!(app.elettrico.symbol.scale, 75);
        assert_eq!(app.elettrico.symbol.percent, 25);
        assert!(app.elettrico.symbol.meters);
        command(&mut app, "ui.elettrico.symbols", &json!({"settings":{"scale":100,"size_percent":10,"units":"mm"}})).unwrap();
        assert_eq!(app.elettrico.symbol.scale, 100);
        assert_eq!(app.elettrico.symbol.percent, 10);
        assert_eq!(app.elettrico.scale, 75);
        assert_eq!(app.elettrico.size_percent, 25);
        assert!(app.elettrico.model_meters);
        assert_eq!(parse_height("").unwrap(), None);
        assert_eq!(parse_height("125,5").unwrap(), Some(125.5));
        assert!(parse_height("NaN").is_err());
        assert!(parse_height("-1").is_err());
    }
}
