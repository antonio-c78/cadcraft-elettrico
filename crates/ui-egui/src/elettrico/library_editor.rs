//! Catalog editing uses an ordinary independent drawing and the same CAD tools as a project.
use crate::CadApp;
use serde_json::{Value, json};

#[derive(Default)]
pub(super) struct State {
    uid: u64,
    name: String,
    copy: bool,
    error: Option<String>,
    confirm_discard: bool,
}
pub(super) fn route(app: &mut CadApp, id: &str, p: &Value) -> Option<Result<Value, String>> {
    if id == "ui.elettrico.library.open" {
        return Some((|| {
            if app.elettrico.editing.snapshot.is_some() {
                return Err("Chiudi prima la modifica della scatola.".into());
            }
            if !super::manager::can_edit_geometry(&app.elettrico.manager, p["catalog"].as_str().unwrap_or("")) {
                return Err("Salva prima le modifiche del gestore catalogo.".into());
            }
            let result = app.session.execute("electrical.library.open", p).map_err(|e| e.to_string())?;
            app.elettrico.library_editor = State {
                uid: result["uid"].as_u64().unwrap_or(0),
                name: format!("{} — copia", result["name"].as_str().unwrap_or("Simbolo")),
                copy: true,
                ..Default::default()
            };
            app.elettrico.open = false;
            app.elettrico.symbol.open = false;
            super::manager::hide(&mut app.elettrico.manager);
            super::unifilar_browser::hide(&mut app.elettrico.unifilar_browser);
            app.ui.start_tab = false;
            Ok(result)
        })());
    }
    let active = cadcraft_engine::cmd::electrical::library_editor::active(&app.session);
    if active && ((id == "qsave" && p.is_null()) || id == "ui.elettrico.library.save") {
        let result = save(app);
        if let Err(e) = &result {
            app.elettrico.library_editor.error = Some(e.clone());
        }
        return Some(result);
    }
    if active && id == "close" && p.is_null() {
        app.elettrico.library_editor.confirm_discard = true;
        return Some(Ok(json!({"confirmation":true})));
    }
    None
}
fn save(app: &mut CadApp) -> Result<Value, String> {
    let state = &app.elettrico.library_editor;
    let result = app.session.execute("electrical.library.save", &json!({"name":state.name,"copy":state.copy})).map_err(|e| e.to_string())?;
    super::manager::invalidate(&mut app.elettrico.manager, result["catalog"].as_str().unwrap_or(""));
    super::unifilar_browser::hide(&mut app.elettrico.unifilar_browser);
    // General catalog reload rebuilds the composer, insertion browser and favorite thumbnails.
    if result["unifilar"] == false
        && app.elettrico.catalog_path.as_deref() == result["catalog"].as_str()
        && let Some(Err(e)) = super::route(app, "ui.elettrico.catalog", &json!({}))
    {
        app.session.echo(format!("Simbolo salvato. Ricarica catalogo: {e}"));
    }
    app.elettrico.library_editor = State::default();
    Ok(result)
}
pub(super) fn show(app: &mut CadApp, ctx: &egui::Context) {
    if !cadcraft_engine::cmd::electrical::library_editor::active(&app.session) {
        return;
    }
    let Ok(data) = app.session.execute("electrical.library.read", &Value::Null) else {
        return;
    };
    if app.elettrico.library_editor.uid != data["uid"].as_u64().unwrap_or(0) {
        app.elettrico.library_editor = State {
            uid: data["uid"].as_u64().unwrap_or(0),
            name: format!("{} — copia", data["name"].as_str().unwrap_or("Simbolo")),
            copy: true,
            ..Default::default()
        };
    }
    let mut state = std::mem::take(&mut app.elettrico.library_editor);
    let mut pick = None;
    let mut save_now = false;
    let mut discard = false;
    egui::Window::new("Editor simbolo — Catalogo")
        .id(egui::Id::new("electrical.library.editor"))
        .default_pos(egui::pos2(20.0, 125.0))
        .default_width(330.0)
        .resizable(false)
        .show(ctx, |ui| {
            ui.heading(data["name"].as_str().unwrap_or("Simbolo"));
            ui.label("Modifica la geometria con i normali strumenti CAD.");
            ui.label("I riferimenti logici non fanno parte della grafica salvata.");
            ui.separator();
            for (id, label) in [("origin", "Origine / inserimento"), ("in", "IN · ingresso"), ("out", "OUT · uscita")] {
                ui.horizontal(|ui| {
                    if ui.add_enabled(app.session.running.is_none(), egui::Button::new(format!("Definisci {label}"))).clicked() {
                        pick = Some(id);
                    }
                    if let Some(a) = data["points"][id].as_array() {
                        ui.monospace(format!(
                            "{:.2}, {:.2}",
                            a.first().and_then(Value::as_f64).unwrap_or(0.),
                            a.get(1).and_then(Value::as_f64).unwrap_or(0.)
                        ));
                    } else {
                        ui.label("Non definito");
                    }
                });
            }
            ui.small("IN e OUT sono facoltativi. Per rimuoverli seleziona il punto e usa Canc.");
            if let Some(error) = data["points_error"].as_str() {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.separator();
            ui.label("Nome nel catalogo");
            ui.add(egui::TextEdit::singleline(&mut state.name).char_limit(200).desired_width(305.));
            ui.checkbox(&mut state.copy, "Salva come nuovo simbolo (copia)");
            if !state.copy {
                ui.label("Aggiorna la voce del catalogo. Gli esemplari nei progetti restano invariati.");
            }
            ui.horizontal_wrapped(|ui| {
                save_now = ui.add_enabled(app.session.running.is_none(), egui::Button::new("Salva nel catalogo e torna")).clicked();
                if ui.button("Annulla modifiche…").clicked() {
                    state.confirm_discard = true;
                }
            });
            if state.confirm_discard {
                ui.separator();
                ui.label("Chiudere l'editor senza salvare nel catalogo?");
                ui.horizontal(|ui| {
                    discard = ui.button("Sì, scarta e torna").clicked();
                    if ui.button("Continua modifica").clicked() {
                        state.confirm_discard = false;
                    }
                });
            }
            if let Some(error) = &state.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        });
    if let Some(xf) = app.canvas.xf {
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new("electrical.editor.markers"))).with_clip_rect(xf.rect);
        for (id, label) in [("origin", "ORIGINE"), ("in", "IN"), ("out", "OUT")] {
            if let Ok(a) = serde_json::from_value::<[f64; 2]>(data["points"][id].clone()) {
                let pos = xf.to_screen(cadcraft_geom::Vec2::new(a[0], a[1]));
                let tokens = crate::theme::Tokens::get();
                let color = if id == "origin" {
                    tokens.snap
                } else if id == "in" {
                    tokens.accent
                } else {
                    tokens.icon_point
                };
                painter.circle_stroke(pos, 6., egui::Stroke::new(2., color));
                painter.text(
                    pos + egui::vec2(if id == "origin" { -10. } else { 10. }, -10.),
                    if id == "origin" { egui::Align2::RIGHT_BOTTOM } else { egui::Align2::LEFT_BOTTOM },
                    label,
                    egui::FontId::proportional(14.),
                    tokens.canvas_ink,
                );
            }
        }
    }
    app.elettrico.library_editor = state;
    let result = if save_now {
        save(app).map(|_| ())
    } else if discard {
        app.session.cancel();
        app.session.execute("electrical.library.cancel", &json!({"discard":true})).map(|_| ()).map_err(|e| e.to_string())
    } else if let Some(point) = pick {
        app.session
            .execute("electrical.library.choose", &json!({"point":point}))
            .map_err(|e| e.to_string())
            .map(|_| app.start("electrical.library.point"))
    } else {
        Ok(())
    };
    if let Err(e) = result {
        app.elettrico.library_editor.error = Some(e);
    }
}
