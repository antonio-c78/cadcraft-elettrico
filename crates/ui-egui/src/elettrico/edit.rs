//! Editing supplies a saved draft to the same composer used for new boxes.
use super::*;
use cadcraft_engine::cmd::electrical::Composition;

#[derive(Clone, Copy, Default, PartialEq)]
pub(super) enum Mode {
    #[default]
    Whole,
    Height,
    Label,
}
impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Whole => "Intera scatola",
            Self::Height => "Altezza di installazione",
            Self::Label => "Posizione del testo",
        }
    }
}

struct Saved {
    capacity: u8,
    items: Vec<usize>,
    height: String,
    scale: u16,
    percent: u16,
    meters: bool,
    search: String,
    catalog: catalog::Catalog,
    library: Option<preview::Library>,
    error: Option<String>,
}
#[derive(Default)]
pub(super) struct State {
    waiting: bool,
    pub snapshot: Option<Value>,
    pub mode: Mode,
    pub side: u8,
    pub factor: Option<f64>,
    saved: Option<Saved>,
}

pub(super) fn open(app: &mut CadApp, params: &Value) -> Result<Value, String> {
    if app.session.running.is_some() {
        return Err("Termina il comando corrente prima di modificare la scatola.".into());
    }
    finish(&mut app.elettrico);
    if !app.elettrico.library_attempted {
        app.elettrico.library_attempted = true;
        if let Some(path) = catalog_environment() {
            let _ = route(app, "ui.elettrico.catalog", &json!({"path":path}));
        }
    }
    app.elettrico.open = false;
    if app.session.selection().is_empty() && params.get("handle").is_none() {
        app.elettrico.editing.waiting = true;
        app.start("electrical.select");
        return Ok(json!({"select":true}));
    }
    load(app, params)
}

fn load(app: &mut CadApp, params: &Value) -> Result<Value, String> {
    let snapshot = app.run("electrical.read", params.clone())?;
    hydrate(&mut app.elettrico, snapshot)?;
    Ok(json!({"open":true}))
}

fn hydrate(state: &mut super::State, snapshot: Value) -> Result<(), String> {
    let c: Composition = serde_json::from_value(snapshot["composition"].clone()).map_err(|e| e.to_string())?;
    let mut catalog = state.catalog.clone();
    let mut items = Vec::new();
    for (position, item) in c.items.iter().enumerate() {
        let index =
            catalog.components.iter().position(|v| v.block == item.block && v.name == item.name && v.modules == item.modules).unwrap_or_else(|| {
                let index = catalog.components.len();
                catalog.components.push(catalog::Component {
                    id: format!("__saved_{}_{}", snapshot["handle"], position),
                    name: item.name.clone(),
                    block: item.block.clone(),
                    modules: item.modules,
                    category: vec!["Nella scatola selezionata".into()],
                });
                index
            });
        items.push(index);
    }
    let library = state.library.as_ref().map(|library| library.remap(&state.catalog.components, &catalog.components));
    let saved = Saved {
        capacity: state.capacity,
        items: state.items.clone(),
        height: state.height.clone(),
        scale: state.scale,
        percent: state.size_percent,
        meters: state.model_meters,
        search: state.search.clone(),
        catalog: state.catalog.clone(),
        library: state.library.clone(),
        error: state.library_error.clone(),
    };
    state.capacity = c.capacity;
    state.items = items;
    state.height = c.installation_height_cm.map(|h| h.to_string()).unwrap_or_default();
    state.catalog = catalog;
    state.library = library;
    state.library_error = None;
    state.search.clear();
    state.replacing = None;
    let percent = c.factor * if state.model_meters { 1000.0 } else { 1.0 } * 100.0 / f64::from(state.scale);
    if (5.0..=300.0).contains(&percent) && (percent / 5.0 - (percent / 5.0).round()).abs() < 1e-9 {
        state.size_percent = percent.round() as u16;
    }
    state.editing = State {
        side: snapshot["side"].as_u64().unwrap_or(0) as u8,
        factor: Some(c.factor),
        snapshot: Some(snapshot),
        saved: Some(saved),
        ..Default::default()
    };
    state.open = true;
    Ok(())
}

pub(super) fn finish(state: &mut super::State) {
    let editing = std::mem::take(&mut state.editing);
    if let Some(saved) = editing.saved {
        state.capacity = saved.capacity;
        state.items = saved.items;
        state.height = saved.height;
        state.scale = saved.scale;
        state.size_percent = saved.percent;
        state.model_meters = saved.meters;
        state.search = saved.search;
        state.catalog = saved.catalog;
        state.library = saved.library;
        state.library_error = saved.error;
    }
    state.replacing = None;
}

pub(super) fn poll(app: &mut CadApp, ctx: &egui::Context) {
    if !app.elettrico.editing.waiting {
        return;
    }
    if app.session.running.as_ref().is_some_and(|r| r.id == "electrical.select") {
        egui::Window::new("Seleziona scatola — Elettrico").resizable(false).show(ctx, |ui| {
            ui.label("Fai clic sulla scatola da modificare nella planimetria.");
            if ui.button("Annulla").clicked() {
                app.session.cancel();
                app.elettrico.editing.waiting = false;
            }
        });
    } else if app.session.running.is_none() {
        app.elettrico.editing.waiting = false;
        if !app.session.selection().is_empty()
            && let Err(e) = load(app, &json!({}))
        {
            app.session.echo(e);
        }
    }
}

pub(super) fn parameters(state: &super::State) -> Result<Value, String> {
    let mut params = state.editing.snapshot.clone().ok_or("Seleziona una scatola.")?;
    let mut c: Composition = serde_json::from_value(params["composition"].clone()).map_err(|e| e.to_string())?;
    match state.editing.mode {
        Mode::Whole => {
            c.capacity = state.capacity;
            c.items = state
                .items
                .iter()
                .map(|i| {
                    state.catalog.components.get(*i).ok_or("Componente assente dal catalogo.").map(|c| cadcraft_engine::cmd::electrical::Component {
                        name: c.name.clone(),
                        block: c.block.clone(),
                        modules: c.modules,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            c.factor = state.factor();
            c.installation_height_cm = parse_height(&state.height)?;
            params["side"] = json!(state.editing.side);
        }
        Mode::Height => {
            c.installation_height_cm = parse_height(&state.height)?;
        }
        Mode::Label => {
            params["side"] = json!(state.editing.side);
        }
    }
    params["composition"] = json!(c);
    if let Some(library) = &state.library {
        params["library"] = json!(library.source_path);
    }
    Ok(params)
}

pub(super) fn apply(app: &mut CadApp, placement: bool) -> Result<(), String> {
    let params = parameters(&app.elettrico)?;
    app.run(if placement { "electrical.edit.prepare" } else { "electrical.update" }, params)?;
    app.elettrico.open = false;
    app.elettrico.settings_open = false;
    if placement {
        app.start("electrical.place");
    } else {
        finish(&mut app.elettrico);
    }
    Ok(())
}

pub(super) fn partial_preview(ui: &mut egui::Ui, state: &super::State) {
    let Some(snapshot) = &state.editing.snapshot else { return };
    let Ok(c) = serde_json::from_value::<Composition>(snapshot["composition"].clone()) else { return };
    let items: Vec<_> = c
        .items
        .iter()
        .filter_map(|item| state.catalog.components.iter().position(|v| v.block == item.block && v.name == item.name && v.modules == item.modules))
        .collect();
    let height =
        if state.editing.mode == Mode::Height { state.height.clone() } else { c.installation_height_cm.map(|h| h.to_string()).unwrap_or_default() };
    let side = if state.editing.mode == Mode::Label { state.editing.side } else { snapshot["side"].as_u64().unwrap_or(0) as u8 };
    let preview_state =
        super::State { capacity: c.capacity, items, height, catalog: state.catalog.clone(), library: state.library.clone(), ..Default::default() };
    let _ = preview::show_edit(ui, &preview_state, side);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> Value {
        json!({"handle":"1A","uid":1,"revision":2,"at":[120,230],"rotation":90,"side":2,
            "composition":{"capacity":7,"factor":0.123456,"installation_height_cm":100,
                "items":[{"name":"Old switch","block":"Switch","modules":2}]}})
    }
    #[test]
    fn shared_composer_preserves_snapshot_and_restores_the_new_box_draft() {
        let mut state = super::super::State { capacity: 4, height: "80".into(), ..Default::default() };
        state.catalog.components.push(catalog::Component {
            id: "switch".into(),
            name: "Renamed switch".into(),
            block: "Switch".into(),
            modules: 1,
            category: vec![],
        });
        state.items = vec![0];
        hydrate(&mut state, snapshot()).unwrap();
        assert_eq!(state.capacity, 7);
        assert_eq!(state.used(), 2);
        let params = parameters(&state).unwrap();
        assert_eq!(params["composition"]["factor"], 0.123456);
        assert_eq!(params["composition"]["items"][0]["name"], "Old switch");
        assert_eq!(params["at"], json!([120, 230]));
        assert_eq!(params["rotation"], 90);
        finish(&mut state);
        assert_eq!(state.capacity, 4);
        assert_eq!(state.height, "80");
        assert_eq!(state.catalog.components.len(), 1);
        assert_eq!(state.used(), 1);
        assert!(state.editing.snapshot.is_none());
    }
    #[test]
    fn partial_modes_do_not_apply_hidden_composition_or_label_changes() {
        let mut state = super::super::State::default();
        hydrate(&mut state, snapshot()).unwrap();
        state.capacity = 3;
        state.items.clear();
        state.height = "120".into();
        state.editing.side = 1;
        state.editing.mode = Mode::Height;
        let params = parameters(&state).unwrap();
        assert_eq!(params["composition"]["capacity"], 7);
        assert_eq!(params["composition"]["items"].as_array().unwrap().len(), 1);
        assert_eq!(params["composition"]["installation_height_cm"], 120.0);
        assert_eq!(params["side"], 2);
        state.editing.mode = Mode::Label;
        state.height = "invalid".into();
        let params = parameters(&state).unwrap();
        assert_eq!(params["composition"]["installation_height_cm"], 100.0);
        assert_eq!(params["side"], 1);
    }
}
