//! Same category-and-thumbnail vocabulary as the electrical catalog, with a device-specific form.
use super::{catalog, preview};
use crate::CadApp;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Default)]
pub(super) struct State {
    open: bool,
    model: String,
    data: Value,
    components: Vec<catalog::Component>,
    library: Option<preview::Library>,
    category: String,
    query: String,
    favorites_only: bool,
    pub(super) favorites: BTreeSet<String>,
    selected: Option<usize>,
    fields: BTreeMap<String, String>,
    editing: Option<Value>,
    placing: bool,
    error: Option<String>,
}
pub(super) fn path(app: &CadApp) -> String {
    if !app.elettrico.unifilar.model.is_empty() {
        return app.elettrico.unifilar.model.clone();
    }
    app.elettrico
        .catalog_path
        .clone()
        .or_else(super::catalog_environment)
        .as_deref()
        .and_then(|p| std::path::Path::new(p).parent()?.parent())
        .map(|p| p.join("unifilare/v1/modello.json").to_string_lossy().into_owned())
        .unwrap_or_default()
}
pub(super) fn hide(state: &mut State) {
    state.open = false;
}
pub(super) fn command(app: &mut CadApp, id: &str, p: &Value) -> Result<Value, String> {
    if app.session.running.is_some() || app.elettrico.editing.snapshot.is_some() {
        return Err("Termina il comando corrente.".into());
    }
    let edit = if id == "ui.elettrico.unifilar.edit" { Some(app.run("electrical.unifilar.read", p.clone())?) } else { None };
    let model = p.get("model").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| path(app));
    let data = app.run("electrical.unifilar.catalog", json!({"model":model}))?;
    let symbols = data["symbols"].as_array().ok_or("Catalogo non valido.")?;
    let components: Vec<_> = symbols
        .iter()
        .map(|s| catalog::Component {
            id: s["id"].as_str().unwrap_or("").into(),
            name: s["name"].as_str().unwrap_or("").into(),
            block: s["block"].as_str().unwrap_or("").into(),
            category: vec![s["category"].as_str().unwrap_or("").into()],
            ..Default::default()
        })
        .collect();
    let library = preview::Library::load(data["library"].as_str().ok_or("Libreria non disponibile.")?, &components)?;
    let selected = edit.as_ref().and_then(|e| components.iter().position(|s| s.id == e["symbol"].as_str().unwrap_or("")));
    if edit.is_some() && selected.is_none() {
        return Err("Il simbolo inserito non è più nel catalogo esterno.".into());
    }
    let fields = edit.as_ref().and_then(|e| serde_json::from_value(e["fields"].clone()).ok()).unwrap_or_default();
    let old = std::mem::take(&mut app.elettrico.unifilar_browser);
    app.elettrico.unifilar_browser = State {
        open: true,
        model,
        data,
        components,
        library: Some(library),
        favorites: old.favorites,
        category: old.category,
        query: old.query,
        selected,
        fields,
        editing: edit,
        ..Default::default()
    };
    app.elettrico.open = false;
    app.elettrico.settings_open = false;
    app.elettrico.symbol.open = false;
    super::manager::hide(&mut app.elettrico.manager);
    super::rooms::hide(&mut app.elettrico.rooms);
    Ok(json!({"open":true}))
}
fn params(state: &State) -> Result<Value, String> {
    let c = state.selected.and_then(|i| state.components.get(i)).ok_or("Scegli un simbolo.")?;
    let mut p = json!({"model":state.model,"id":c.id,"fields":state.fields});
    if let Some(old) = &state.editing {
        p["original"] = old.clone();
        for k in ["handle", "uid", "revision", "at"] {
            p[k] = old[k].clone();
        }
    }
    Ok(p)
}
fn form(ui: &mut egui::Ui, state: &mut State) -> Option<&'static str> {
    let i = state.selected?;
    let c = state.components.get(i)?;
    ui.heading(&c.name);
    let symbol = state.data["symbols"].as_array()?.get(i)?.clone();
    ui.columns(2, |cols| {
        preview::show_symbol(&mut cols[0], state.library.as_ref().and_then(|l| l.symbol(i)));
        cols[0].label(if symbol["kind"] == "line_marker" {
            "Si applica a una linea esistente e non occupa una colonna della tabella."
        } else {
            "Il simbolo si aggancia al centro della colonna. L'altezza la scegli nel disegno."
        });
        let fav = state.favorites.contains(&c.id);
        if cols[0].button(if fav { "★ Rimuovi dai preferiti" } else { "☆ Aggiungi ai preferiti" }).clicked() {
            if fav {
                state.favorites.remove(&c.id);
            } else {
                state.favorites.insert(c.id.clone());
            }
        }
        cols[1].label("Dati facoltativi · le celle seguono l'apparecchio");
        egui::ScrollArea::vertical().id_salt("unifilar.fields").max_height(370.0).show(&mut cols[1], |ui| {
            egui::Grid::new("unifilar.device.fields").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                if let Some(fields) = symbol["fields"].as_array() {
                    for key in fields.iter().filter_map(Value::as_str) {
                        let label = state.data["layout"]["table"]["rows"]
                            .as_array()
                            .and_then(|rows| rows.iter().find(|r| r["id"] == key))
                            .and_then(|r| r["label"].as_str())
                            .unwrap_or(key);
                        ui.label(label);
                        let v = state.fields.entry(key.into()).or_default();
                        if key == "descrizione" {
                            ui.add(egui::TextEdit::multiline(v).desired_rows(3).desired_width(205.0).char_limit(256));
                        } else {
                            ui.add(egui::TextEdit::singleline(v).desired_width(205.0).char_limit(128));
                        }
                        ui.end_row();
                    }
                }
            });
        });
    });
    ui.separator();
    if symbol["kind"] != "line_marker" {
        ui.collapsing("Anteprima della colonna in tabella", |ui| {
            egui::ScrollArea::vertical().id_salt("unifilar.cell.preview").max_height(280.0).show(ui, |ui| column_preview(ui, state));
        });
    }
    let mut action = None;
    ui.horizontal(|ui| {
        if state.editing.is_some() {
            if ui.button("Salva modifiche").clicked() {
                action = Some("update");
            }
            if ui.button("Sposta / cambia colonna").clicked() {
                action = Some("place");
            }
        } else if ui.button(if symbol["kind"] == "line_marker" { "Applica alla linea" } else { "Posiziona" }).clicked() {
            action = Some("place");
        }
        if ui.button("Scegli un altro simbolo").clicked() {
            state.selected = None;
        }
    });
    action
}
fn column_preview(ui: &mut egui::Ui, state: &State) {
    use cadcraft_engine::cmd::electrical::unifilar::{Cell, placement::cell_lines};
    let Some(raw) = state.data["layout"]["columns"].as_array().and_then(|c| c.first()) else {
        return;
    };
    let Ok(cells) = serde_json::from_value::<BTreeMap<String, Cell>>(raw["cells"].clone()) else {
        return;
    };
    let top = cells.values().map(|c| c.bounds[3]).fold(f64::NEG_INFINITY, f64::max);
    let bottom = cells.values().map(|c| c.bounds[1]).fold(f64::INFINITY, f64::min);
    let width = cells.values().next().map(|c| c.bounds[2] - c.bounds[0]).unwrap_or(35.5);
    let scale = 4.0;
    let label_width = 190.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(label_width + width as f32 * scale, (top - bottom) as f32 * scale), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let stroke = egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color);
    if let Some(rows) = state.data["layout"]["table"]["rows"].as_array() {
        for row in rows {
            let key = row["id"].as_str().unwrap_or("");
            let Some(c) = cells.get(key) else {
                continue;
            };
            let y = rect.top() + (top - c.bounds[3]) as f32 * scale;
            let height = (c.bounds[3] - c.bounds[1]) as f32 * scale;
            let label_rect = egui::Rect::from_min_size(egui::pos2(rect.left(), y), egui::vec2(label_width, height));
            let value_rect = egui::Rect::from_min_size(egui::pos2(label_rect.right(), y), egui::vec2(width as f32 * scale, height));
            painter.rect_stroke(label_rect, 0.0, stroke, egui::StrokeKind::Inside);
            painter.rect_stroke(value_rect, 0.0, stroke, egui::StrokeKind::Inside);
            painter.text(
                label_rect.left_center() + egui::vec2(5.0, 0.0),
                egui::Align2::LEFT_CENTER,
                row["label"].as_str().unwrap_or(key),
                egui::FontId::proportional(10.0),
                ui.visuals().text_color(),
            );
            let text = state.fields.get(key).map(String::as_str).unwrap_or("");
            match cell_lines(text, c) {
                Ok(lines) => {
                    let start = value_rect.center().y - (lines.len().saturating_sub(1)) as f32 * 6.0;
                    for (i, line) in lines.iter().enumerate() {
                        ui.painter_at(value_rect.shrink(2.0)).text(
                            egui::pos2(value_rect.center().x, start + i as f32 * 12.0),
                            egui::Align2::CENTER_CENTER,
                            line,
                            egui::FontId::proportional(8.4),
                            ui.visuals().text_color(),
                        );
                    }
                }
                Err(e) => {
                    painter.text(
                        value_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Testo da abbreviare",
                        egui::FontId::proportional(10.0),
                        ui.visuals().error_fg_color,
                    );
                    ui.interact(value_rect, ui.id().with(("cell.error", key)), egui::Sense::hover()).on_hover_text(e.to_string());
                }
            }
        }
    }
}
fn gallery(ui: &mut egui::Ui, state: &mut State) {
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(180.0);
            ui.heading("Categorie");
            if ui.selectable_label(state.category.is_empty() && !state.favorites_only, "Tutti i simboli").clicked() {
                state.category.clear();
                state.favorites_only = false;
            }
            if ui.selectable_label(state.favorites_only, "★ Preferiti").clicked() {
                state.category.clear();
                state.favorites_only = true;
            }
            let categories: BTreeSet<_> = state.components.iter().filter_map(|c| c.category.first()).cloned().collect();
            for category in categories {
                if ui.selectable_label(state.category == category && !state.favorites_only, &category).clicked() {
                    state.category = category;
                    state.favorites_only = false;
                }
            }
        });
        ui.separator();
        ui.vertical(|ui| {
            ui.set_min_width(ui.available_width().max(300.0));
            ui.heading(if state.favorites_only {
                "Preferiti"
            } else if state.category.is_empty() {
                "Tutti i simboli"
            } else {
                &state.category
            });
            ui.add(egui::TextEdit::singleline(&mut state.query).hint_text("Cerca un simbolo...").desired_width(280.0));
            let query = state.query.to_lowercase();
            let indices: Vec<_> = state
                .components
                .iter()
                .enumerate()
                .filter(|(i, c)| {
                    state
                        .editing
                        .as_ref()
                        .is_none_or(|old| state.data["symbols"].as_array().and_then(|s| s.get(*i)).is_some_and(|sym| sym["kind"] == old["kind"]))
                        && (state.category.is_empty() || c.category.first() == Some(&state.category))
                        && (!state.favorites_only || state.favorites.contains(&c.id))
                        && (query.is_empty() || c.name.to_lowercase().contains(&query))
                })
                .map(|(i, _)| i)
                .collect();
            ui.label(format!("{} simboli · clicca un'anteprima per scegliere", indices.len()));
            egui::ScrollArea::vertical().id_salt("unifilar.gallery").max_height(470.0).show(ui, |ui| {
                let columns = (ui.available_width() / 155.0).floor().clamp(1.0, 5.0) as usize;
                for row in indices.chunks(columns) {
                    ui.horizontal(|ui| {
                        for i in row {
                            let (rect, response) = ui.allocate_exact_size(egui::vec2(145.0, 175.0), egui::Sense::click());
                            preview::paint_symbol(
                                ui,
                                egui::Rect::from_min_size(rect.min, egui::vec2(145.0, 128.0)),
                                state.library.as_ref().and_then(|l| l.symbol(*i)),
                            );
                            if let Some(c) = state.components.get(*i) {
                                let painter = ui.painter_at(rect);
                                let galley =
                                    painter.layout(c.name.clone(), egui::FontId::proportional(11.0), ui.visuals().text_color(), rect.width() - 10.0);
                                painter.galley(rect.min + egui::vec2(5.0, 132.0), galley, ui.visuals().text_color());
                            }
                            if response.clicked() {
                                state.selected = Some(*i);
                                let allowed = state.data["symbols"]
                                    .as_array()
                                    .and_then(|s| s.get(*i))
                                    .and_then(|s| s["fields"].as_array())
                                    .cloned()
                                    .unwrap_or_default();
                                state.fields.retain(|k, _| allowed.iter().any(|v| v == k));
                                if let Some(sym) = state.data["symbols"].as_array().and_then(|s| s.get(*i))
                                    && state.editing.as_ref().is_none_or(|old| old["symbol"] != sym["id"])
                                    && let Some(defaults) = sym["defaults"].as_object()
                                {
                                    for (key, value) in defaults {
                                        if let Some(value) = value.as_str() {
                                            state.fields.insert(key.clone(), value.into());
                                        }
                                    }
                                }
                                state.error = None;
                            }
                        }
                    });
                }
            });
        });
    });
}
fn placing_guide(app: &mut CadApp, ctx: &egui::Context) {
    if !app.elettrico.unifilar_browser.placing || app.session.running.is_none() {
        return;
    }
    let error = app.session.unifilar_draft.as_ref().and_then(|d| d.error.clone());
    let mut back = false;
    let mut cancel = false;
    egui::Window::new("Posiziona simbolo — Unifilare")
        .id(egui::Id::new("unifilar.placing.guide"))
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, 120.0))
        .resizable(false)
        .show(ctx, |ui| {
            ui.label("Scegli il punto nel disegno. Esc annulla.");
            ui.label("Apparecchi: centro colonna e altezza libera. Marcatori: clic sulla linea.");
            if let Some(error) = &error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.horizontal(|ui| {
                back = ui.button("Torna ai dati").clicked();
                cancel = ui.button("Annulla").clicked();
            });
        });
    if back || cancel {
        app.session.cancel();
        app.session.unifilar_draft = None;
        app.elettrico.unifilar_browser.placing = false;
        app.elettrico.unifilar_browser.open = back;
    }
}
pub(super) fn show(app: &mut CadApp, ctx: &egui::Context) {
    placing_guide(app, ctx);
    if app.elettrico.unifilar_browser.placing && app.session.running.is_none() {
        app.elettrico.unifilar_browser.placing = false;
        app.session.unifilar_draft = None;
    }
    if !app.elettrico.unifilar_browser.open {
        return;
    }
    let mut state = std::mem::take(&mut app.elettrico.unifilar_browser);
    let mut open = true;
    let mut action = None;
    egui::Window::new(if state.editing.is_some() { "Modifica apparecchio — Unifilare" } else { "Inserisci apparecchio — Unifilare" })
        .id(egui::Id::new("unifilar.browser"))
        .open(&mut open)
        .default_width(1060.0)
        .resizable(true)
        .show(ctx, |ui| {
            if state.selected.is_some() {
                action = form(ui, &mut state);
            } else {
                gallery(ui, &mut state);
            }
            if let Some(e) = &state.error {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
        });
    if let Some(action) = action {
        let result = (|| {
            let p = params(&state)?;
            if action == "update" {
                app.run("electrical.unifilar.update", p)?;
            } else {
                app.run("electrical.unifilar.prepare", p)?;
                app.start("electrical.unifilar.place");
                state.placing = true;
            }
            Ok::<_, String>(())
        })();
        match result {
            Ok(()) => open = false,
            Err(e) => state.error = Some(e),
        }
    }
    state.open = open;
    app.elettrico.unifilar_browser = state;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unifilar_favorites_persist_separately_from_the_general_catalog() {
        let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        app.elettrico.unifilar_browser.favorites.insert("synthetic-id".into());
        let prefs = app.prefs_json();
        let mut restored = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        restored.load_prefs(&prefs);
        assert!(restored.elettrico.unifilar_browser.favorites.contains("synthetic-id"));
        assert!(restored.elettrico.favorites.is_empty());
    }
}
