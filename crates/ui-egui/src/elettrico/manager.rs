//! Catalog drafts are isolated from the active catalog until a validated, backed-up save.
use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::{
    catalog::{self, Catalog},
    preview,
};
use crate::CadApp;

mod gallery;

#[derive(Default)]
pub(super) struct State {
    open: bool,
    initialized: bool,
    draft: Catalog,
    favorites: BTreeSet<String>,
    original: Vec<u8>,
    path: String,
    selected: Option<usize>,
    browsing: Vec<String>,
    favorites_only: bool,
    thumbnail: f32,
    search: String,
    category: String,
    dirty: bool,
    delete_confirmation: bool,
    message: Option<String>,
}

impl State {
    fn select(&mut self, index: usize) {
        self.selected = self.draft.components.get(index).map(|_| index);
        self.category = self.draft.components.get(index).map(|c| c.category.join(" / ")).unwrap_or_default();
        self.delete_confirmation = false;
    }

    fn duplicate(&mut self) -> Result<String, String> {
        if self.draft.components.len() >= 512 {
            return Err("Limite di 512 simboli raggiunto.".into());
        }
        let mut component = self.selected.and_then(|i| self.draft.components.get(i)).cloned().ok_or("Seleziona un simbolo.")?;
        // Identity is independent of display name, category and the shared source geometry.
        let mut sequence = self.draft.extra.get("next_variant_id").and_then(Value::as_u64).unwrap_or(1);
        let mut available = None;
        for _ in 0..1024 {
            let id = format!("variant-{sequence}");
            sequence = sequence.checked_add(1).ok_or("Limite identificativi raggiunto.")?;
            if self.draft.components.iter().all(|c| c.id != id) {
                available = Some(id);
                break;
            }
        }
        let id = available.ok_or("Impossibile generare un ID libero.")?;
        self.draft.extra.insert("next_variant_id".into(), json!(sequence));
        component.id = id.clone();
        let mut name: String = component.name.chars().take(60).collect();
        name.push_str(" — copia");
        component.name = name;
        self.draft.components.push(component);
        self.select(self.draft.components.len().saturating_sub(1));
        self.dirty = true;
        Ok(id)
    }

    fn delete(&mut self) -> Result<(), String> {
        let i = self.selected.filter(|i| *i < self.draft.components.len()).ok_or("Seleziona un simbolo.")?;
        let c = self.draft.components.remove(i);
        self.favorites.remove(&c.id);
        self.select(i.min(self.draft.components.len().saturating_sub(1)));
        self.dirty = true;
        Ok(())
    }
}

fn initialize(app: &mut CadApp) -> Result<(), String> {
    if app.session.running.is_some() || app.elettrico.editing.snapshot.is_some() {
        return Err("Termina il comando o la modifica della scatola prima di gestire il catalogo.".into());
    }
    // Reopening/reloading must refresh live metadata and preview too, even for the same path.
    super::route(app, "ui.elettrico.catalog", &json!({})).ok_or("Catalogo non disponibile.")??;
    let path = app.elettrico.catalog_path.clone().ok_or("Carica un catalogo dalle Impostazioni.")?;
    let original = catalog::read(&path, 1024 * 1024)?;
    let draft = Catalog::parse(&original)?;
    let favorites = catalog::load_favorites(&path)?;
    // Use precisely the saved file, not an unrelated library chosen temporarily in the composer.
    let mut state = State { open: app.elettrico.manager.open, initialized: true, draft, favorites, original, path, ..Default::default() };
    state.thumbnail = 140.0;
    app.elettrico.manager = state;
    Ok(())
}

fn save(app: &mut CadApp) -> Result<Value, String> {
    if app.session.running.is_some() || app.elettrico.editing.snapshot.is_some() {
        return Err("Termina il comando o la modifica della scatola prima di salvare il catalogo.".into());
    }
    let manager = &app.elettrico.manager;
    if !manager.initialized {
        return Err("Apri prima il gestore catalogo.".into());
    }
    if app.elettrico.catalog_path.as_deref() != Some(manager.path.as_str()) {
        return Err("Il catalogo attivo è cambiato. Scarta le modifiche e ricarica.".into());
    }
    let mut candidate = manager.draft.clone();
    candidate.version = 2;
    candidate.favorites = Some(manager.favorites.clone());
    let candidate = Catalog::parse(&serde_json::to_vec(&candidate).map_err(|e| e.to_string())?)?;
    let items = candidate.remap(&app.elettrico.catalog, &app.elettrico.items, app.elettrico.capacity)?;
    let source = std::path::Path::new(&manager.path).parent().unwrap_or(std::path::Path::new(".")).join(&candidate.library);
    let library = preview::Library::load(&source.to_string_lossy(), &candidate.components)?;
    for (i, c) in candidate.components.iter().enumerate() {
        if (c.composer || c.standalone) && library.symbol(i).is_none() {
            return Err(format!("Simbolo assente nella libreria DXF: {}.", c.name));
        }
    }
    let bytes = catalog::save_catalog(&manager.path, &candidate, &manager.original)?;
    let count = candidate.components.len();
    app.elettrico.items = items;
    app.elettrico.favorites = manager.favorites.clone();
    app.elettrico.library = Some(library);
    app.elettrico.library_error = None;
    app.elettrico.library_attempted = true;
    app.elettrico.catalog = candidate.clone();
    let manager = &mut app.elettrico.manager;
    manager.draft = candidate;
    manager.original = bytes;
    manager.dirty = false;
    manager.message = Some("Catalogo salvato e aggiornato nel compositore. Copia precedente nella cartella backups.".into());
    Ok(json!({"saved":true,"components":count}))
}

/// `action`: open/read/browse/back/select/update/duplicate/delete/save/reload.
pub(super) fn command(app: &mut CadApp, params: &Value) -> Result<Value, String> {
    let action = params.get("action").and_then(Value::as_str).unwrap_or("open");
    if action == "reload" || !app.elettrico.manager.initialized {
        initialize(app)?;
    }
    match action {
        "open" => {
            app.elettrico.manager.open = params.get("open").and_then(Value::as_bool).unwrap_or(true);
        }
        "read" | "reload" => {}
        "back" => {
            app.elettrico.manager.selected = None;
            app.elettrico.manager.delete_confirmation = false;
        }
        "browse" => {
            let path: Vec<String> = serde_json::from_value(params.get("category").cloned().unwrap_or(json!([]))).map_err(|e| e.to_string())?;
            if path.len() > 4 || path.iter().any(|s| s.is_empty() || s.len() > 80) {
                return Err("Categoria non valida.".into());
            }
            let state = &mut app.elettrico.manager;
            state.browsing = path;
            state.favorites_only = params.get("favorites").and_then(Value::as_bool).unwrap_or(false);
            state.selected = None;
            state.delete_confirmation = false;
            if let Some(search) = params.get("search").and_then(Value::as_str) {
                state.search = search.chars().take(255).collect();
            }
        }
        "save" => return save(app),
        "select" | "update" | "duplicate" | "delete" => {
            let state = &mut app.elettrico.manager;
            let id = params.get("id").and_then(Value::as_str).ok_or("Indica l'ID del simbolo.")?;
            let i = state.draft.components.iter().position(|c| c.id == id).ok_or("Simbolo non trovato.")?;
            state.select(i);
            match action {
                "duplicate" => return state.duplicate().map(|id| json!({"id":id})),
                "delete" => state.delete()?,
                "update" => {
                    let fields = params.get("fields").and_then(Value::as_object).ok_or("Indica i campi da modificare.")?;
                    let c = state.draft.components.get(i).ok_or("Simbolo non trovato.")?;
                    let mut value = serde_json::to_value(c).map_err(|e| e.to_string())?;
                    for (key, v) in fields {
                        if !["name", "category", "modules", "composer", "standalone", "notes"].contains(&key.as_str()) {
                            return Err(format!("Campo non modificabile: {key}"));
                        }
                        value[key] = v.clone();
                    }
                    let mut candidate = state.draft.clone();
                    if let Some(c) = candidate.components.get_mut(i) {
                        *c = serde_json::from_value(value).map_err(|e| e.to_string())?;
                    }
                    let candidate = Catalog::parse(&serde_json::to_vec(&candidate).map_err(|e| e.to_string())?)?;
                    state.draft = candidate;
                    if let Some(favorite) = params.get("favorite").and_then(Value::as_bool) {
                        if favorite {
                            state.favorites.insert(id.to_string());
                        } else {
                            state.favorites.remove(id);
                        }
                    }
                    state.select(i);
                    state.dirty = true;
                }
                _ => {}
            }
        }
        _ => return Err("Azione catalogo non riconosciuta.".into()),
    }
    let state = &app.elettrico.manager;
    Ok(json!({"open":state.open,"dirty":state.dirty,"components":state.draft.components,"favorites":state.favorites,
        "view":if state.selected.is_some(){"detail"}else{"gallery"},"category":state.browsing,"search":state.search,
        "visible":gallery::indices(state).iter().filter_map(|i|state.draft.components.get(*i).map(|c|&c.id)).collect::<Vec<_>>(),
        "selected":state.selected.and_then(|i|state.draft.components.get(i)).map(|c|&c.id)}))
}

pub(super) fn show(app: &mut CadApp, ctx: &egui::Context) {
    if !app.elettrico.manager.open {
        return;
    }
    let mut state = std::mem::take(&mut app.elettrico.manager);
    let mut open = state.open;
    let mut action = None;
    egui::Window::new("Gestione catalogo simboli — Elettrico")
        .id(egui::Id::new("elettrico.manager.gallery"))
        .open(&mut open)
        .default_width(1080.0)
        .min_width(880.0)
        .default_height(720.0)
        .max_height(740.0)
        .vscroll(true)
        .show(ctx, |ui| {
            ui.label("Categorie e proprietà dei simboli. Le modifiche saranno applicate quando salvi il catalogo.");
            ui.separator();
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(egui::vec2(220.0, 490.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_max_width(220.0);
                    gallery::sidebar(ui, &mut state);
                });
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.set_min_width(580.0);
                    if state.selected.is_some() {
                        if ui.button("← Torna alla galleria").clicked() {
                            state.selected = None;
                            state.delete_confirmation = false;
                        }
                        if state.selected.is_some() {
                            ui.heading("Attributi del simbolo");
                            detail(ui, &mut state, &app.elettrico);
                        }
                    } else {
                        gallery::sheet(ui, &mut state, &app.elettrico);
                    }
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.add_enabled(state.selected.is_some(), egui::Button::new("Duplica variante")).clicked()
                    && let Err(e) = state.duplicate()
                {
                    state.message = Some(e);
                }
                if ui.add_enabled(state.selected.is_some(), egui::Button::new("Elimina dal catalogo")).clicked() {
                    state.delete_confirmation = true;
                }
            });
            if state.delete_confirmation {
                ui.label("Il simbolo sarà rimosso dal catalogo. I disegni e il file DXF originale rimangono disponibili.");
                ui.horizontal(|ui| {
                    if ui.button("Conferma eliminazione").clicked()
                        && let Err(e) = state.delete()
                    {
                        state.message = Some(e);
                    }
                    if ui.button("Mantieni simbolo").clicked() {
                        state.delete_confirmation = false;
                    }
                });
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.add_enabled(state.dirty, egui::Button::new("Salva catalogo")).clicked() {
                    action = Some("save");
                }
                if ui.button("Scarta modifiche e ricarica").clicked() {
                    action = Some("reload");
                }
                ui.label(if state.dirty { "Modifiche da salvare" } else { "Catalogo salvato" });
            });
            if let Some(message) = &state.message {
                ui.label(message);
            }
        });
    state.open = open;
    app.elettrico.manager = state;
    if let Some(action) = action
        && let Err(e) = app.run("ui.elettrico.manager", json!({"action":action}))
    {
        app.elettrico.manager.message = Some(e);
    }
}

fn detail(ui: &mut egui::Ui, state: &mut State, live: &super::State) {
    let selected = state.selected;
    let list = selected
        .and_then(|i| state.draft.components.get(i))
        .and_then(|c| live.catalog.components.iter().position(|v| v.block == c.block))
        .and_then(|i| live.library.as_ref().and_then(|l| l.symbol(i)));
    preview::show_symbol(ui, list);
    if let Some(c) = selected.and_then(|i| state.draft.components.get_mut(i)) {
        ui.label("Nome del simbolo");
        state.dirty |= ui.text_edit_singleline(&mut c.name).changed();
        ui.label("Categoria / Sottocategoria (fino a 4 livelli)");
        if ui.text_edit_singleline(&mut state.category).changed() {
            c.category = state.category.split('/').map(str::trim).filter(|s| !s.is_empty()).take(5).map(str::to_string).collect();
            state.dirty = true;
        }
        state.dirty |= ui.checkbox(&mut c.composer, "Visibile nel compositore").changed();
        if c.composer {
            c.modules = c.modules.max(1);
            ui.horizontal(|ui| {
                ui.label("Moduli occupati");
                state.dirty |= ui.add(egui::DragValue::new(&mut c.modules).range(1..=7)).changed();
            });
        }
        state.dirty |= ui.checkbox(&mut c.standalone, "Disponibile per inserimento singolo").changed();
        let mut favorite = state.favorites.contains(&c.id);
        if ui.checkbox(&mut favorite, "★ Preferito").changed() {
            if favorite {
                state.favorites.insert(c.id.clone());
            } else {
                state.favorites.remove(&c.id);
            }
            state.dirty = true;
        }
        ui.label("Note");
        state.dirty |= ui.add(egui::TextEdit::multiline(&mut c.notes).desired_rows(2)).changed();
        ui.small(format!("Blocco: {}", c.block));
        ui.small(format!("ID: {}", c.id));
    } else {
        ui.label("Seleziona un simbolo nell'elenco.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn category_gallery_includes_descendants_and_back_preserves_the_browsing_context() {
        let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        app.elettrico.manager.initialized = true;
        app.elettrico.manager.draft = Catalog::parse(
            br#"{"version":2,"library":"x.dxf","components":[
            {"id":"a","name":"Switch","block":"A","modules":1,"category":["Controls"]},
            {"id":"b","name":"Socket","block":"B","modules":2,"category":["Sockets","Power"]},
            {"id":"c","name":"Data socket","block":"C","modules":1,"category":["Sockets","Data"]},
            {"id":"d","name":"Other socket","block":"D","modules":1,"category":["Sockets special"]}]}"#,
        )
        .unwrap();
        let view = command(&mut app, &json!({"action":"browse","category":["Sockets"],"search":"socket"})).unwrap();
        assert_eq!(view["visible"], json!(["c", "b"]));
        command(&mut app, &json!({"action":"select","id":"b"})).unwrap();
        let back = command(&mut app, &json!({"action":"back"})).unwrap();
        assert_eq!(back["view"], "gallery");
        assert_eq!(back["category"], json!(["Sockets"]));
        assert_eq!(back["search"], "socket");
        assert!(!app.elettrico.manager.dirty);
        app.elettrico.manager.favorites.insert("b".into());
        let favorites = command(&mut app, &json!({"action":"browse","favorites":true,"search":""})).unwrap();
        assert_eq!(favorites["visible"], json!(["b"]));
    }

    #[test]
    fn draft_commands_are_atomic_and_do_not_open_windows_or_change_drawings() {
        let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        app.elettrico.manager.initialized = true;
        app.elettrico.manager.draft =
            Catalog::parse(br#"{"version":1,"library":"x.dxf","components":[{"id":"a","name":"A","block":"Geometry","modules":1}]}"#).unwrap();
        let documents = app.session.docs.len();
        assert!(command(&mut app, &json!({"action":"update","id":"a","fields":{"modules":0}})).is_err());
        assert_eq!(app.elettrico.manager.draft.components[0].modules, 1);
        command(&mut app, &json!({"action":"update","id":"a","fields":{"name":"New","category":["Devices","Switches"]},"favorite":true})).unwrap();
        assert_eq!(app.elettrico.manager.draft.components[0].name, "New");
        assert!(app.elettrico.manager.favorites.contains("a"));
        assert!(app.elettrico.catalog.components.is_empty());
        assert!(!app.elettrico.manager.open);
        assert_eq!(app.session.docs.len(), documents);
    }

    #[test]
    fn variants_share_only_geometry_and_keep_independent_identity_properties_and_favorites() {
        let draft =
            Catalog::parse(br#"{"version":1,"library":"x.dxf","components":[{"id":"a","name":"A","block":"Geometry","modules":1}]}"#).unwrap();
        let mut state = State { draft, favorites: BTreeSet::from(["a".into()]), ..Default::default() };
        state.select(0);
        let id = state.duplicate().unwrap();
        state.draft.components[1].modules = 2;
        assert_ne!(id, "a");
        assert_eq!(state.draft.components[0].modules, 1);
        assert_eq!(state.draft.components[1].block, "Geometry");
        assert!(!state.favorites.contains(&id));
        state.delete().unwrap();
        assert_eq!(state.draft.components.len(), 1);
        assert!(state.favorites.contains("a"));
        assert_ne!(state.duplicate().unwrap(), id);
    }
}
