//! Native electrical composer. Symbol artwork remains in the user's external catalog.

use serde_json::{Value, json};

use crate::{CadApp, theme::Tokens};

mod calibration;
mod catalog;
pub(crate) mod context;
mod edit;
mod library_editor;
mod manager;
mod preview;
pub(crate) mod project;
mod rooms;
mod symbol;
mod unifilar;
mod unifilar_browser;
mod unifilar_circuits;
mod unifilar_connections;
mod unifilar_duplication;

fn catalog_environment() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::var("CADCRAFT_ELECTRICAL_CATALOG").ok()
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

pub struct State {
    project: project::State,
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
    catalog: catalog::Catalog,
    catalog_path: Option<String>,
    favorites: std::collections::BTreeSet<String>,
    search: String,
    editing: edit::State,
    replacing: Option<usize>,
    manager: manager::State,
    symbol: symbol::State,
    rooms: rooms::State,
    context: context::State,
    calibration: calibration::State,
    unifilar: unifilar::State,
    unifilar_browser: unifilar_browser::State,
    unifilar_connections: unifilar_connections::State,
    unifilar_duplication: unifilar_duplication::State,
    unifilar_circuits: unifilar_circuits::State,
    library_editor: library_editor::State,
}

impl Default for State {
    fn default() -> Self {
        Self {
            project: project::State::default(),
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
            catalog: catalog::Catalog::default(),
            catalog_path: None,
            favorites: Default::default(),
            search: String::new(),
            editing: edit::State::default(),
            replacing: None,
            manager: manager::State::default(),
            symbol: symbol::State::default(),
            rooms: rooms::State::default(),
            context: context::State::default(),
            calibration: calibration::State::default(),
            unifilar: unifilar::State::default(),
            unifilar_browser: unifilar_browser::State::default(),
            unifilar_connections: unifilar_connections::State::default(),
            unifilar_duplication: unifilar_duplication::State::default(),
            unifilar_circuits: unifilar_circuits::State::default(),
            library_editor: library_editor::State::default(),
        }
    }
}

impl State {
    fn can_choose(&self, modules: u8) -> bool {
        let replaced = self.replacing.and_then(|p| self.items.get(p)).and_then(|i| self.catalog.components.get(*i)).map(|c| c.modules).unwrap_or(0);
        u16::from(self.used()) - u16::from(replaced) + u16::from(modules) <= u16::from(self.capacity)
    }
    fn choose(&mut self, index: usize) {
        if !self.catalog.components.get(index).is_some_and(|c| c.composer && self.can_choose(c.modules)) {
            return;
        }
        if let Some(position) = self.replacing.take() {
            if let Some(item) = self.items.get_mut(position) {
                *item = index;
            }
        } else {
            self.items.push(index);
        }
    }
    fn settings_factor(&self) -> f64 {
        f64::from(self.scale) * f64::from(self.size_percent) / 100.0 / if self.model_meters { 1000.0 } else { 1.0 }
    }
    fn factor(&self) -> f64 {
        self.editing.factor.unwrap_or_else(|| self.settings_factor())
    }
    fn used(&self) -> u8 {
        self.items.iter().filter_map(|i| self.catalog.components.get(*i)).map(|c| c.modules).sum()
    }
}

pub fn route(app: &mut CadApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if let Some(result) = project::route(app, id, params) {
        return Some(result);
    }
    if let Some(result) = library_editor::route(app, id, params) {
        return Some(result);
    }
    if ["ui.elettrico.unifilar.duplicate.column", "ui.elettrico.unifilar.duplicate.circuit"].contains(&id) {
        return Some(unifilar_duplication::command(app, id, params));
    }
    if matches!(id, "ui.elettrico.unifilar.circuit.move" | "ui.elettrico.unifilar.column.move") {
        return Some(unifilar_circuits::command(app, id, params));
    }
    if id.starts_with("ui.elettrico.unifilar.link.") {
        return Some(unifilar_connections::command(app, id, params));
    }
    if matches!(id, "ui.elettrico.unifilar.symbols" | "ui.elettrico.unifilar.edit") {
        return Some(unifilar_browser::command(app, id, params));
    }
    if id == "ui.elettrico.unifilar.new" {
        return Some(unifilar::command(app, params));
    }
    if id == "ui.elettrico.calibrate" {
        return Some(calibration::command(app));
    }
    if id.starts_with("ui.elettrico.context.") {
        return Some(context::command(app, id));
    }
    if matches!(id, "ui.elettrico.room" | "ui.elettrico.rooms") {
        return Some(rooms::command(app, id));
    }
    if matches!(id, "ui.elettrico.symbols" | "ui.elettrico.symbol.insert") {
        return Some(symbol::command(app, id, params));
    }
    if id == "ui.elettrico.manager" {
        return Some(manager::command(app, params));
    }
    if id == "ui.elettrico.compose" {
        app.elettrico.symbol.open = false;
        edit::finish(&mut app.elettrico);
    }
    if id == "ui.elettrico.edit" {
        return Some(edit::open(app, params));
    }
    if id == "ui.elettrico.catalog" {
        if app.elettrico.editing.snapshot.is_some() {
            return Some(Err("Chiudi la modifica della scatola prima di ricaricare il catalogo.".into()));
        }
        let result = (|| {
            if app.session.running.is_some() {
                return Err("Termina il comando corrente prima di ricaricare il catalogo.".to_string());
            }
            let path = params
                .get("path")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| app.elettrico.catalog_path.clone())
                .or_else(catalog_environment)
                .ok_or("Indica il percorso del catalogo JSON.".to_string())?;
            if path.len() > 32768 {
                return Err("Percorso catalogo troppo lungo.".to_string());
            }
            let catalog = catalog::Catalog::parse(&catalog::read(&path, 1024 * 1024)?)?;
            let selected = catalog.remap(&app.elettrico.catalog, &app.elettrico.items, app.elettrico.capacity)?;
            let favorites = catalog::load_favorites(&path)?;
            let library_path = std::path::Path::new(&path).parent().unwrap_or(std::path::Path::new(".")).join(&catalog.library);
            let library = preview::Library::load(&library_path.to_string_lossy(), &catalog.components)?;
            let count = catalog.components.len();
            let first = app.elettrico.catalog_path.as_deref() != Some(path.as_str());
            if first || catalog.defaults.scale != app.elettrico.catalog.defaults.scale {
                app.elettrico.scale = catalog.defaults.scale;
            }
            if first || catalog.defaults.size_percent != app.elettrico.catalog.defaults.size_percent {
                app.elettrico.size_percent = catalog.defaults.size_percent;
            }
            if first || catalog.defaults.units != app.elettrico.catalog.defaults.units {
                app.elettrico.model_meters = catalog.defaults.units == "m";
            }
            app.elettrico.catalog = catalog;
            app.elettrico.items = selected;
            app.elettrico.favorites = favorites;
            app.elettrico.catalog_path = Some(path);
            app.elettrico.library = Some(library);
            app.elettrico.library_error = None;
            app.elettrico.library_attempted = true;
            Ok(json!({"components":count}))
        })();
        if let Err(e) = &result {
            app.elettrico.library_error = Some(e.clone());
        }
        return Some(result);
    }
    if id == "ui.elettrico.library" {
        if app.elettrico.editing.snapshot.is_some() {
            return Some(Err("Chiudi la modifica della scatola prima di cambiare libreria.".into()));
        }
        let path = if params.is_null() {
            app.services.pick_open.as_ref().and_then(|pick| pick())
        } else {
            params.get("path").and_then(Value::as_str).map(str::to_string)
        };
        let Some(path) = path else {
            return Some(if params.is_null() { Ok(Value::Null) } else { Err("Indica il percorso della libreria DXF.".into()) });
        };
        app.elettrico.library_attempted = true;
        return Some(match preview::Library::load(&path, &app.elettrico.catalog.components) {
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

pub(crate) fn request_editor_close(app: &mut CadApp, index: usize) -> bool {
    if !cadcraft_engine::cmd::electrical::library_editor::is_editor(&app.session, index) {
        return false;
    }
    let _ = app.session.execute("document.switch", &json!({"index":index}));
    let _ = library_editor::route(app, "close", &Value::Null);
    true
}
pub fn show(app: &mut CadApp, ctx: &egui::Context) {
    library_editor::show(app, ctx);
    unifilar::show(app, ctx);
    unifilar_browser::show(app, ctx);
    project::show(app, ctx);
    unifilar_connections::show(app, ctx);
    unifilar_circuits::show(app, ctx);
    unifilar_duplication::show(app, ctx);
    calibration::show(app, ctx);
    context::show(app, ctx);
    rooms::show(app, ctx);
    edit::poll(app, ctx);
    placement_window(app, ctx);
    if app.session.running.as_ref().is_some_and(|r| matches!(r.id.as_str(), "electrical.place" | "electrical.symbol.place")) {
        return;
    }
    if !app.elettrico.library_attempted && (app.elettrico.open || catalog_environment().is_some()) {
        app.elettrico.library_attempted = true;
        if let Some(path) = catalog_environment() {
            let _ = route(app, "ui.elettrico.catalog", &json!({"path": path}));
        } else {
            app.elettrico.library_error =
                Some("Carica un catalogo JSON con ui.elettrico.catalog oppure avvia CADCraft Elettrico dal suo collegamento.".into());
        }
    }
    manager::show(app, ctx);
    symbol::show(app, ctx);
    let mut choose_library = false;
    let mut insert = false;
    let mut reload = false;
    let mut favorites_changed = false;
    let mut placement = false;
    let mut cancel = false;
    let state = &mut app.elettrico;
    let mut open = state.open;
    let editing = state.editing.snapshot.is_some();
    let old_factor = state.settings_factor();
    let t = Tokens::get();
    egui::Window::new(if editing { "Modifica scatola — Elettrico" } else { "Componi scatola — Elettrico" })
        .id(egui::Id::new("elettrico.composer"))
        .open(&mut open)
        .default_width(820.0)
        .min_width(760.0)
        .show(ctx, |ui| {
            if editing {
                egui::ComboBox::from_label("Cosa vuoi modificare?").selected_text(state.editing.mode.label()).show_ui(ui, |ui| {
                    for mode in [edit::Mode::Whole, edit::Mode::Height, edit::Mode::Label] {
                        ui.selectable_value(&mut state.editing.mode, mode, mode.label());
                    }
                });
                ui.separator();
            }
            if !editing || state.editing.mode == edit::Mode::Whole {
                ui.horizontal(|ui| {
                    ui.label("Scatola · posti");
                    let used = state.used();
                    for capacity in [3, 4, 6, 7] {
                        ui.add_enabled_ui(used <= capacity, |ui| {
                            ui.selectable_value(&mut state.capacity, capacity, capacity.to_string());
                        });
                    }
                    if ui.button("Impostazioni").clicked() {
                        state.settings_open = true;
                    }
                });
                height_field(ui, &mut state.height);
                ui.separator();
                ui.columns(2, |columns| {
                    if let Some(ui) = columns.get_mut(0) {
                        ui.heading("Componenti");
                        ui.add(egui::TextEdit::singleline(&mut state.search).hint_text("Cerca nome o simbolo..."));
                        egui::ScrollArea::vertical().max_height(290.0).show(ui, |ui| {
                            ui.push_id("favorites", |ui| {
                                egui::CollapsingHeader::new("Preferiti").default_open(true).show(ui, |ui| {
                                    let indices: Vec<_> = state
                                        .catalog
                                        .components
                                        .iter()
                                        .enumerate()
                                        .filter(|(_, c)| state.favorites.contains(&c.id) && matches_search(c, &state.search))
                                        .map(|(i, _)| i)
                                        .collect();
                                    if indices.is_empty() {
                                        ui.label("Usa la stella per aggiungere un preferito.");
                                    }
                                    for i in indices {
                                        favorites_changed |= component_row(ui, state, i);
                                    }
                                });
                            });
                            favorites_changed |= category_tree(ui, state, &[]);
                        });
                    }
                    if let Some(ui) = columns.get_mut(1) {
                        ui.heading("Anteprima CAD");
                        if let Some(position) = preview::show_edit(ui, state, if editing { state.editing.side } else { 0 }) {
                            state.replacing = Some(position);
                        }
                        if let Some(error) = &state.library_error {
                            ui.colored_label(t.warn, error);
                        } else if state.library.is_none() {
                            ui.label("Carica la libreria simboli dalle Impostazioni.");
                        }
                        ui.heading("Composizione");
                        ui.label(format!("{} / {} posti occupati", state.used(), state.capacity));
                        let mut remove = None;
                        for (position, index) in state.items.iter().enumerate() {
                            if let Some(component) = state.catalog.components.get(*index) {
                                ui.horizontal(|ui| {
                                    ui.label(format!("{} · {} p", component.name, component.modules));
                                    if ui.small_button("Sostituisci").clicked() {
                                        state.replacing = Some(position);
                                    }
                                    if ui.small_button("Rimuovi").clicked() {
                                        remove = Some(position);
                                    }
                                });
                            }
                        }
                        if let Some(position) = remove {
                            state.items.remove(position);
                            state.replacing = None;
                        }
                        if ui.button("Svuota scatola").clicked() {
                            state.items.clear();
                            state.replacing = None;
                        }
                        ui.separator();
                        if let Some(position) = state.replacing {
                            if let Some(component) = state.items.get(position).and_then(|i| state.catalog.components.get(*i)) {
                                ui.label(format!("Scegli nei Componenti il sostituto di {}.", component.name));
                            }
                            if ui.small_button("Annulla sostituzione").clicked() {
                                state.replacing = None;
                            }
                        }
                        ui.label("Schema dei posti");
                        ui.horizontal(|ui| {
                            for index in &state.items {
                                if let Some(component) = state.catalog.components.get(*index) {
                                    ui.group(|ui| {
                                        ui.label(format!("{} p", component.modules));
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
            } else {
                if state.editing.mode == edit::Mode::Height {
                    height_field(ui, &mut state.height);
                } else {
                    ui.horizontal(|ui| {
                        ui.label("Posizione altezza");
                        for (value, label) in [(0, "Sopra"), (1, "Sotto"), (2, "Destra"), (3, "Sinistra")] {
                            ui.selectable_value(&mut state.editing.side, value, label);
                        }
                    });
                }
                edit::partial_preview(ui, state);
            }
            ui.separator();
            if editing {
                let valid = state.editing.mode != edit::Mode::Whole || (!state.items.is_empty() && state.used() <= state.capacity);
                ui.horizontal(|ui| {
                    insert = ui.add_enabled(valid, egui::Button::new("Conferma modifiche")).clicked();
                    if state.editing.mode == edit::Mode::Whole {
                        placement = ui.add_enabled(valid, egui::Button::new("Riposiziona nel disegno")).clicked();
                    }
                    cancel = ui.button("Annulla").clicked();
                });
                if let Some(error) = &state.library_error {
                    ui.colored_label(t.warn, error);
                }
            } else {
                insert =
                    ui.add_enabled(!state.items.is_empty() && state.library.is_some(), egui::Button::new("Inserisci nella planimetria")).clicked();
                ui.colored_label(t.text_dim, "Scegli il punto nel disegno, poi rotazione e posizione dell'altezza.");
            }
        });
    state.open = open && !cancel;
    egui::Window::new("Impostazioni — Elettrico").open(&mut state.settings_open).show(ctx, |ui| {
        ui.heading("Libreria simboli");
        if let Some(path) = &state.catalog_path {
            ui.label(format!("{} componenti nel catalogo esterno", state.catalog.components.len()));
            ui.label(path);
            ui.label("Nomi, posti, categorie, simboli e valori iniziali si modificano nel JSON. Le stelle sono salvate in preferiti.json.");
        }
        reload = ui.add_enabled(!editing, egui::Button::new("Ricarica catalogo e simboli")).clicked();
        if let Some(library) = &state.library {
            ui.label(format!("{} · {} simboli disponibili", library.name, library.available()));
        } else {
            ui.label("Nessuna libreria caricata");
        }
        if ui.add_enabled(!editing, egui::Button::new("Scegli libreria DXF...")).clicked() {
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
    if let Some(factor) = &mut state.editing.factor {
        let new_factor = f64::from(state.scale) * f64::from(state.size_percent) / 100.0 / if state.model_meters { 1000.0 } else { 1.0 };
        if new_factor != old_factor {
            *factor *= new_factor / old_factor;
        }
    }
    if editing && !state.open {
        state.settings_open = false;
        edit::finish(state);
        return;
    }
    if choose_library {
        let _ = route(app, "ui.elettrico.library", &Value::Null);
    }
    if favorites_changed
        && let Some(path) = &app.elettrico.catalog_path
        && let Err(e) = catalog::save_favorites(path, &app.elettrico.favorites)
    {
        app.elettrico.library_error = Some(e);
    }
    if reload {
        let _ = route(app, "ui.elettrico.catalog", &Value::Null);
    }
    if editing && (insert || placement) {
        if let Err(e) = edit::apply(app, placement) {
            app.elettrico.library_error = Some(e);
        }
    } else if insert {
        let result = (|| {
            let state = &app.elettrico;
            let height = parse_height(&state.height)?;
            let library = state.library.as_ref().ok_or("Carica la libreria simboli.")?;
            let items: Vec<_> = state
                .items
                .iter()
                .filter_map(|i| state.catalog.components.get(*i))
                .map(|c| json!({"name":c.name,"modules":c.modules,"block":c.block}))
                .collect();
            let factor = state.factor();
            let params = json!({"library":library.source_path,"layers":state.catalog.extra.get("layers"),"composition":{"capacity":state.capacity,"items":items,"factor":factor,"installation_height_cm":height}});
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

fn parse_height(text: &str) -> Result<Option<f64>, String> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let value = text.trim().replace(',', ".").parse::<f64>().map_err(|_| "Altezza non valida: indica i centimetri.".to_string())?;
    if !value.is_finite() || !(0.0..=10000.0).contains(&value) {
        return Err("Altezza ammessa: 0–10000 cm.".into());
    }
    Ok(Some(value))
}

fn height_field(ui: &mut egui::Ui, height: &mut String) {
    ui.horizontal(|ui| {
        ui.label("Altezza installazione (cm)");
        ui.add(egui::TextEdit::singleline(height).desired_width(65.0).hint_text("100"));
        ui.label("Facoltativa · vuoto = nessuna scritta");
    });
}

fn matches_search(c: &catalog::Component, search: &str) -> bool {
    let search = search.trim().to_lowercase();
    c.composer && (search.is_empty() || c.name.to_lowercase().contains(&search) || c.block.to_lowercase().contains(&search))
}

fn component_row(ui: &mut egui::Ui, state: &mut State, index: usize) -> bool {
    let Some(c) = state.catalog.components.get(index).cloned() else { return false };
    let mut changed = false;
    ui.push_id(index, |ui| {
        ui.horizontal(|ui| {
            let favorite = state.favorites.contains(&c.id);
            if ui
                .add_enabled(!c.id.starts_with("__saved_"), egui::Button::new(if favorite { "★" } else { "☆" }).small())
                .on_hover_text("Aggiungi o rimuovi dai preferiti")
                .clicked()
            {
                if favorite {
                    state.favorites.remove(&c.id);
                } else {
                    state.favorites.insert(c.id.clone());
                }
                changed = true;
            }
            if ui
                .add_enabled(
                    state.can_choose(c.modules),
                    egui::Button::new(format!("{} {} · {} p", if state.replacing.is_some() { "→" } else { "+" }, c.name, c.modules)),
                )
                .clicked()
            {
                state.choose(index);
            }
        });
    });
    changed
}

fn category_tree(ui: &mut egui::Ui, state: &mut State, prefix: &[String]) -> bool {
    if prefix.len() > 4 {
        return false;
    }
    let mut changed = false;
    let rows: Vec<_> = state
        .catalog
        .components
        .iter()
        .enumerate()
        .filter(|(_, c)| c.category == prefix && matches_search(c, &state.search))
        .map(|(i, _)| i)
        .collect();
    for i in rows {
        changed |= component_row(ui, state, i);
    }
    let children: std::collections::BTreeSet<_> = state
        .catalog
        .components
        .iter()
        .filter(|c| c.category.starts_with(prefix) && matches_search(c, &state.search))
        .filter_map(|c| c.category.get(prefix.len()))
        .cloned()
        .collect();
    for child in children {
        let mut path = prefix.to_vec();
        path.push(child.clone());
        egui::CollapsingHeader::new(child).id_salt(&path).default_open(true).show(ui, |ui| {
            changed |= category_tree(ui, state, &path);
        });
    }
    changed
}

fn placement_window(app: &mut CadApp, ctx: &egui::Context) {
    if !app.session.running.as_ref().is_some_and(|r| matches!(r.id.as_str(), "electrical.place" | "electrical.symbol.place")) {
        let cancelled_edit = app.session.electrical.as_ref().is_some_and(|p| p.is_edit());
        if app.session.electrical.as_ref().is_some_and(|p| p.phase > 0) {
            app.session.electrical = None;
        }
        if app.elettrico.editing.snapshot.is_some() && !app.elettrico.open {
            if cancelled_edit {
                app.elettrico.open = true;
            } else {
                edit::finish(&mut app.elettrico);
            }
        }
        return;
    }
    let Some(p) = &app.session.electrical else { return };
    let phase = p.phase;
    let editing = p.is_edit();
    let single = p.is_symbol();
    let previous_height = p.composition.installation_height_cm;
    let mut rotation = p.rotation;
    let mut side = p.side;
    let previous = (rotation, side);
    let error = p.error.clone();
    let mut cancel = false;
    let mut next = false;
    let mut repick = false;
    let mut open = true;
    let mut valid_height = true;
    let mut height_change = None;
    egui::Window::new(match phase {
        0 if single => "Inserisci simbolo",
        1 if single => "Orientamento del simbolo",
        3 => "Altezza di installazione",
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
                if editing {
                    repick = ui.button("Scegli un nuovo punto").clicked();
                }
                ui.horizontal(|ui| {
                    for angle in [0, 90, 180, 270] {
                        ui.selectable_value(&mut rotation, f64::from(angle), format!("{angle}°"));
                    }
                });
                ui.add(egui::DragValue::new(&mut rotation).range(-360.0..=360.0).suffix("°"));
            }
            3 => {
                if symbol::parse_height(&app.elettrico.symbol.height).is_ok_and(|h| h != previous_height) {
                    app.elettrico.symbol.height = previous_height.map(|h| h.to_string()).unwrap_or_default();
                }
                height_field(ui, &mut app.elettrico.symbol.height);
                match symbol::parse_height(&app.elettrico.symbol.height) {
                    Ok(h) => {
                        if h != previous_height {
                            height_change = Some(h);
                        }
                    }
                    Err(e) => {
                        valid_height = false;
                        ui.colored_label(Tokens::get().warn, e);
                    }
                }
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
                next = ui
                    .add_enabled(
                        valid_height,
                        egui::Button::new(if phase == 1 {
                            "Conferma rotazione"
                        } else if phase == 3 {
                            "Conferma altezza"
                        } else if editing {
                            "Conferma modifica"
                        } else {
                            if single { "Inserisci simbolo" } else { "Inserisci scatola" }
                        }),
                    )
                    .clicked();
            }
            cancel = ui.button("Annulla").clicked();
        });
    });
    if previous != (rotation, side) {
        let _ = app.run("electrical.options", json!({"rotation":rotation,"side":side}));
    }
    if let Some(height) = height_change {
        let _ = app.run("electrical.options", json!({"height_cm":height}));
    }
    if repick {
        let _ = app.run("electrical.options", json!({"repick":true}));
    }
    if cancel || !open {
        app.session.cancel();
        app.session.electrical = None;
        if single {
            app.elettrico.symbol.open = true;
        } else {
            app.elettrico.open = true;
        }
    } else if next {
        let _ = app.session.input(cadcraft_engine::Input::Enter);
        if app.session.running.is_none() && !editing && !single {
            app.elettrico.open = true;
        }
    }
}

pub(crate) fn unifilar_preferences(state: &State) -> Value {
    json!(state.unifilar_browser.favorites)
}
pub(crate) fn load_unifilar_preferences(state: &mut State, value: &Value) {
    if let Ok(favorites) = serde_json::from_value::<std::collections::BTreeSet<String>>(value.clone())
        && favorites.len() <= 512
        && favorites.iter().all(|s| s.len() <= 255)
    {
        state.unifilar_browser.favorites = favorites;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_only_symbols_cannot_be_chosen_as_box_components() {
        let catalog = catalog::Catalog::parse(
            br#"{"version":2,"library":"x.dxf","components":[{"id":"junction","name":"Junction","block":"Junction","modules":0,"composer":false}]}"#,
        )
        .unwrap();
        let mut state = State { catalog, ..Default::default() };
        state.choose(0);
        assert!(state.items.is_empty());
        assert!(!matches_search(&state.catalog.components[0], ""));
    }

    #[test]
    fn unel_uses_two_slots_and_empty_slots_stay_available() {
        let config = || {
            catalog::Catalog::parse(
                br#"{"version":1,"library":"synthetic.dxf","components":[{"id":"unel","name":"Socket","block":"Socket","modules":2}]}"#,
            )
            .unwrap()
        };
        let state = State { capacity: 6, items: vec![0, 0, 0], catalog: config(), ..State::default() };
        assert_eq!(state.used(), 6);
        let state = State { capacity: 3, items: vec![0], catalog: config(), ..State::default() };
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

    #[test]
    fn replacing_a_component_keeps_its_position_and_checks_net_occupancy() {
        let catalog=catalog::Catalog::parse(br#"{"version":1,"library":"synthetic.dxf","components":[{"id":"a","name":"Switch","block":"Switch","modules":1},{"id":"b","name":"Socket","block":"Socket","modules":2}]}"#).unwrap();
        let mut state = State { capacity: 3, items: vec![0, 1], catalog, replacing: Some(0), ..Default::default() };
        state.choose(1);
        assert_eq!(state.items, vec![0, 1]);
        state.replacing = Some(1);
        state.choose(0);
        assert_eq!(state.items, vec![0, 0]);
        assert!(state.replacing.is_none());
        state.replacing = Some(0);
        state.choose(1);
        assert_eq!(state.items, vec![1, 0]);
    }
}
