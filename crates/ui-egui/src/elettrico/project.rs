//! Project sheet manager and page-reference workflow; all mutations go through engine commands.
use crate::CadApp;
use cadcraft_engine::project;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
#[derive(Default)]
pub(super) struct State {
    name: String,
    selected: String,
    number: String,
    title: String,
    panel: String,
    error: Option<String>,
    remove: Option<String>,
    close: bool,
    quit: bool,
    reference: bool,
    destination: String,
    incoming: bool,
    flow: Option<Flow>,
    previews: BTreeMap<u64, (u64, Arc<cadcraft_render::DisplayList>)>,
}
struct Flow {
    pair: String,
    destination: String,
    phase: u8,
    next_kind: String,
}
fn dock(app: &mut CadApp, panel: &str) -> Result<Value, String> {
    crate::docking::command(app, false, &json!({"operation":"open","panel":panel}))
}
fn save(app: &mut CadApp, as_new: bool) -> Result<Value, String> {
    let p = app.session.project.as_ref().ok_or("Nessun progetto aperto.")?;
    let path = if !as_new { p.path.clone() } else { None };
    let path = path.or_else(|| app.services.pick_save.as_ref().and_then(|pick| pick(&format!("{}.cce", p.meta.name))));
    let Some(mut path) = path else { return Ok(json!({"cancelled":true})) };
    if std::path::Path::new(&path).extension().is_none() {
        path.push_str(".cce");
    }
    app.run("electrical.project.save", json!({"path":path}))
}
pub(super) fn route(app: &mut CadApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    if project::active(&app.session) && matches!(id, "qsave" | "ui.save" | "saveas" | "ui.saveas") && p.is_null() {
        return Some(save(app, matches!(id, "saveas" | "ui.saveas")));
    }
    match id {
        "ui.elettrico.project" => Some(dock(app, "projectSheets")),
        "ui.elettrico.toolbar" => Some(dock(app, "electricalTools")),
        "ui.elettrico.project.save" => Some(save(app, false)),
        "ui.elettrico.project.saveas" => Some(save(app, true)),
        "ui.elettrico.project.reference" => {
            if !project::active(&app.session) {
                return Some(Err("Apri un foglio di progetto prima di inserire un rimando.".into()));
            }
            app.elettrico.project.reference = true;
            Some(Ok(json!({"open":true})))
        }
        "ui.elettrico.project.follow" => Some(app.run("electrical.project.reference.follow", p.clone())),
        _ => None,
    }
}
fn action(app: &mut CadApp, state: &mut State, id: &str, p: Value) {
    match app.run(id, p) {
        Ok(_) => {
            state.error = None;
            app.ui.start_tab = false;
        }
        Err(e) => state.error = Some(e),
    }
}
pub(crate) fn section(app: &mut CadApp, ui: &mut egui::Ui) {
    let mut st = std::mem::take(&mut app.elettrico.project);
    let data = match app.run("electrical.project.list", json!({})) {
        Ok(v) => v,
        Err(e) => {
            st.error = Some(e);
            Value::Null
        }
    };
    ui.horizontal(|ui| {
        if crate::icons::button(ui, crate::icons::Icon::Open, 24.0, "Apri progetto .cce", false).clicked()
            && let Some(path) = app.services.pick_open.as_ref().and_then(|p| p())
        {
            if path.to_lowercase().ends_with(".cce") {
                action(app, &mut st, "electrical.project.open", json!({"path":path}));
            } else {
                st.error = Some("Seleziona un file di progetto .cce. I DXF si aprono normalmente e si aggiungono al progetto.".into());
            }
        }
        if ui.add_enabled(app.session.project.is_some(), egui::Button::new("Salva")).clicked()
            && let Err(e) = save(app, false)
        {
            st.error = Some(e);
        }
        if ui.add_enabled(app.session.project.is_some(), egui::Button::new("Salva con nome…")).clicked()
            && let Err(e) = save(app, true)
        {
            st.error = Some(e);
        }
    });
    if data["open"] != true {
        ui.heading("Nuovo progetto elettrico");
        ui.add(egui::TextEdit::singleline(&mut st.name).hint_text("Nome progetto").char_limit(160));
        if ui.add_enabled(!st.name.trim().is_empty(), egui::Button::new("Crea progetto")).clicked() {
            let name = st.name.trim().to_string();
            action(app, &mut st, "electrical.project.new", json!({"name":name}));
        }
        ui.label("Aggiungi le tavole già aperte oppure crea nuovi schemi unifilari.");
    } else {
        ui.heading(format!("{}{}", data["name"].as_str().unwrap_or("Progetto"), if data["dirty"] == true { " *" } else { "" }));
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(!app.session.docs.is_empty() && !project::active(&app.session), egui::Button::new("Aggiungi tavola aperta")).clicked() {
                let model =
                    if app.session.state().is_ok_and(|s| s.unifilar_grid.is_some()) { Some(super::unifilar_browser::path(app)) } else { None };
                action(app, &mut st, "electrical.project.add", json!({"model":model}));
            }
            if ui.button("Nuovo unifilare…").clicked() {
                action(app, &mut st, "ui.elettrico.unifilar.new", Value::Null);
            }
            if ui.button("Foglio vuoto").clicked() {
                action(app, &mut st, "new", json!({"metric":true}));
                action(app, &mut st, "electrical.project.add", json!({"title":"Nuovo foglio"}));
            }
            if ui.button("Rinumera in ordine").clicked() {
                action(app, &mut st, "electrical.project.sheet", json!({"renumber":true}));
            }
            if ui.button("Chiudi progetto…").clicked() {
                st.close = true;
            }
        });
        ui.separator();
        let sheets = data["sheets"].as_array().cloned().unwrap_or_default();
        egui::ScrollArea::vertical().id_salt("project.sheet.cards").max_height((ui.available_height() * 0.52).max(120.0)).show(ui, |ui| {
            for (position, sheet) in sheets.iter().enumerate() {
                let id = sheet["id"].as_str().unwrap_or("");
                let uid = sheet["uid"].as_u64().unwrap_or(0);
                ui.push_id(id, |ui| {
                    let selected = st.selected == id;
                    let label = format!(
                        "{} · {}{}",
                        sheet["number"].as_str().unwrap_or(""),
                        sheet["title"].as_str().unwrap_or(""),
                        if sheet["dirty"] == true { " *" } else { "" }
                    );
                    let response = ui.selectable_label(selected, label);
                    if response.clicked() || response.double_clicked() {
                        st.selected = id.into();
                        st.number = sheet["number"].as_str().unwrap_or("").into();
                        st.title = sheet["title"].as_str().unwrap_or("").into();
                        st.panel = sheet["panel"].as_str().unwrap_or("").into();
                    }
                    if response.double_clicked() {
                        action(app, &mut st, "electrical.project.switch", json!({"id":id}));
                    }
                    if let Some(d) = app.session.docs.iter().chain(app.session.project.iter().flat_map(|p| p.closed.iter())).find(|d| d.uid == uid) {
                        if st.previews.get(&uid).is_none_or(|(r, _)| *r != d.revision) {
                            let display = cadcraft_render::build(&d.doc, &cadcraft_doc::Space::Model, &Default::default());
                            st.previews.insert(uid, (d.revision, Arc::new(display)));
                        }
                        let (rect, r) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(80.0), 90.0), egui::Sense::click());
                        super::preview::paint_symbol(ui, rect, st.previews.get(&uid).map(|(_, d)| d.as_ref()));
                        if r.clicked() || r.double_clicked() {
                            st.selected = id.into();
                            st.number = sheet["number"].as_str().unwrap_or("").into();
                            st.title = sheet["title"].as_str().unwrap_or("").into();
                            st.panel = sheet["panel"].as_str().unwrap_or("").into();
                        }
                        if r.double_clicked() {
                            action(app, &mut st, "electrical.project.switch", json!({"id":id}));
                        }
                    }
                    if selected {
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Apri").clicked() {
                                action(app, &mut st, "electrical.project.switch", json!({"id":id}));
                            }
                            if ui.button("Duplica").clicked() {
                                action(app, &mut st, "electrical.project.duplicate", json!({"id":id}));
                            }
                            if ui.add_enabled(position > 0, egui::Button::new("↑")).clicked() {
                                action(app, &mut st, "electrical.project.sheet", json!({"id":id,"position":position.saturating_sub(1)}));
                            }
                            if ui.add_enabled(position + 1 < sheets.len(), egui::Button::new("↓")).clicked() {
                                action(app, &mut st, "electrical.project.sheet", json!({"id":id,"position":position+1}));
                            }
                            if ui.button("Elimina…").clicked() {
                                st.remove = Some(id.into());
                            }
                        });
                    }
                    ui.separator();
                });
            }
        });
        if sheets.iter().any(|s| s["id"] == st.selected) {
            ui.label("Proprietà del foglio selezionato");
            let field_width = (ui.available_width() - 70.0).max(100.0);
            egui::Grid::new("project.sheet.properties").show(ui, |ui| {
                for (label, v) in [("Numero", &mut st.number), ("Titolo", &mut st.title), ("Quadro", &mut st.panel)] {
                    ui.label(label);
                    ui.add(egui::TextEdit::singleline(v).desired_width(field_width).char_limit(160));
                    ui.end_row();
                }
            });
            if ui.button("Applica proprietà").clicked() {
                let p = json!({"id":st.selected,"number":st.number,"title":st.title,"panel":st.panel});
                action(app, &mut st, "electrical.project.sheet", p);
            }
        }
        ui.separator();
        ui.heading("Rimandi tra fogli");
        if ui.add_enabled(project::active(&app.session) && sheets.len() > 1, egui::Button::new("Nuovo rimando…")).clicked() {
            st.reference = true;
        }
        egui::ScrollArea::vertical().id_salt("project.references").max_height(170.0).show(ui, |ui| {
            for r in data["references"].as_array().into_iter().flatten() {
                let ends = r["ends"].as_array().cloned().unwrap_or_default();
                let label = ends.iter().map(|e| e["number"].as_str().unwrap_or("?")).collect::<Vec<_>>().join(" ↔ ");
                ui.horizontal_wrapped(|ui| {
                    if r["status"] == "connected" {
                        ui.label(label);
                    } else {
                        ui.colored_label(ui.visuals().warn_fg_color, format!("{label} · da completare"));
                    }
                    if r["status"] == "pending"
                        && project::active(&app.session)
                        && ends.first().is_some_and(|e| e["sheet"] != active_sheet(app))
                        && ui.button("Completa qui").clicked()
                    {
                        action(
                            app,
                            &mut st,
                            "electrical.project.reference.prepare",
                            json!({"pair":r["id"],"kind":if r["ends"][0]["kind"] == "input" {"output"} else {"input"}}),
                        );
                        app.start("electrical.project.reference.place");
                    }
                    for e in &ends {
                        if ui.small_button(format!("Apri {}", e["number"].as_str().unwrap_or(""))).clicked() {
                            action(app, &mut st, "electrical.project.switch", json!({"id":e["sheet"]}));
                        }
                    }
                });
            }
        });
        if sheets.is_empty() {
            ui.label("Nessun foglio. Aggiungi una tavola o crea il primo schema.");
        }
        st.previews.retain(|uid, _| sheets.iter().any(|s| s["uid"].as_u64() == Some(*uid)));
    }
    if let Some(e) = &st.error {
        ui.colored_label(ui.visuals().error_fg_color, e);
    }
    app.elettrico.project = st;
}
fn active_sheet(app: &CadApp) -> String {
    app.session.project.as_ref().and_then(|p| p.sheet(app.session.state().ok()?.uid)).map(|s| s.id.clone()).unwrap_or_default()
}
pub(crate) fn request_quit(app: &mut CadApp) -> bool {
    if !project::dirty(&app.session) {
        return false;
    }
    app.elettrico.project.close = true;
    app.elettrico.project.quit = true;
    true
}
pub(super) fn show(app: &mut CadApp, ctx: &egui::Context) {
    if let Err(e) = project::references::refresh(&mut app.session) {
        app.session.echo(e.to_string());
    }
    let mut st = std::mem::take(&mut app.elettrico.project);
    if let Some(flow) = st.flow.take() {
        if app.session.running.is_none() {
            let refs = project::references::inspect(&app.session).unwrap_or(Value::Null);
            let count =
                refs.as_array().and_then(|a| a.iter().find(|r| r["id"] == flow.pair)).and_then(|r| r["ends"].as_array()).map(Vec::len).unwrap_or(0);
            if flow.phase == 1 && count == 1 {
                action(app, &mut st, "electrical.project.switch", json!({"id":flow.destination}));
                action(app, &mut st, "electrical.project.reference.prepare", json!({"pair":flow.pair,"kind":flow.next_kind}));
                app.start("electrical.project.reference.place");
                app.set_status("Inserisci il rimando sull'altro foglio. Esc lascia la coppia da completare.");
                st.flow = Some(Flow { phase: 2, ..flow });
            } else {
                let _ = project::references::refresh(&mut app.session);
            }
        } else {
            st.flow = Some(flow);
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
    if st.reference {
        let sheets = app.session.project.as_ref().map(|p| p.meta.sheets.clone()).unwrap_or_default();
        let active = active_sheet(app);
        if !sheets.iter().any(|s| s.id == st.destination && s.id != active) {
            st.destination = sheets.iter().find(|s| s.id != active).map(|s| s.id.clone()).unwrap_or_default();
        }
        let mut open = true;
        let mut place = false;
        egui::Window::new("Rimando tra fogli — Elettrico").open(&mut open).resizable(false).show(ctx, |ui| {
            ui.label("Scegli il tipo e il foglio collegato. Poi indica i due punti nei disegni.");
            egui::ComboBox::from_label("Foglio collegato")
                .selected_text(sheets.iter().find(|s| s.id == st.destination).map(|s| format!("{} · {}", s.number, s.title)).unwrap_or_default())
                .show_ui(ui, |ui| {
                    for s in sheets.iter().filter(|s| s.id != active) {
                        ui.selectable_value(&mut st.destination, s.id.clone(), format!("{} · {}", s.number, s.title));
                    }
                });
            ui.horizontal(|ui| {
                ui.label("Tipo rimando");
                ui.selectable_value(&mut st.incoming, false, "Uscita");
                ui.selectable_value(&mut st.incoming, true, "Ingresso");
            });
            ui.label("La freccia segue il filo collegato, in qualsiasi direzione.");
            place = ui.add_enabled(!st.destination.is_empty() && app.session.running.is_none(), egui::Button::new("Inserisci rimando")).clicked();
        });
        st.reference = open;
        if place {
            let pair = project::id();
            let p = json!({"pair":pair,"kind":if st.incoming{"input"}else{"output"}});
            action(app, &mut st, "electrical.project.reference.prepare", p);
            app.start("electrical.project.reference.place");
            st.flow =
                Some(Flow { pair, destination: st.destination.clone(), phase: 1, next_kind: if st.incoming { "output" } else { "input" }.into() });
            st.reference = false;
        }
    }
    if st.remove.is_some() {
        egui::Modal::new(egui::Id::new("project.remove.sheet")).show(ctx, |ui| {
            ui.label("Eliminare il foglio dal progetto?");
            ui.label("I rimandi sulle altre tavole saranno segnalati come incompleti.");
            if ui.button("Elimina foglio").clicked()
                && let Some(id) = st.remove.take()
            {
                action(app, &mut st, "electrical.project.remove", json!({"id":id,"confirm":true}));
            }
            if ui.button("Annulla").clicked() {
                st.remove = None;
            }
        });
    }
    if st.close {
        egui::Modal::new(egui::Id::new("project.close")).show(ctx, |ui| {
            ui.label("Chiudi progetto");
            if let Some(error) = &st.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            if ui.button("Salva e chiudi").clicked() {
                match save(app, false) {
                    Ok(v) if v["cancelled"] != true => {
                        action(app, &mut st, "electrical.project.close", json!({}));
                        st.close = false
                    }
                    Err(e) => st.error = Some(e),
                    _ => {}
                }
            }
            if ui.button("Chiudi senza salvare").clicked() {
                action(app, &mut st, "electrical.project.close", json!({"discard":true}));
                st.close = false;
            }
            if ui.button("Annulla").clicked() {
                st.close = false;
                st.quit = false;
            }
        });
    }
    if st.quit && !st.close && app.session.project.is_none() {
        st.quit = false;
        if !app.request_quit() {
            app.quit_confirmed = true;
            app.quit_requested = true;
        }
    }
    app.elettrico.project = st;
}
