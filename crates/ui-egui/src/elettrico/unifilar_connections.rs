//! Native connection guide; geometry, validation and Undo stay in the engine.
use crate::CadApp;
use serde_json::{Value, json};
#[derive(Default)]
pub(super) struct State {
    deleting: Option<Value>,
    error: Option<String>,
    placing: bool,
}
pub(super) fn command(app: &mut CadApp, id: &str, p: &Value) -> Result<Value, String> {
    if app.session.running.is_some() || app.elettrico.editing.snapshot.is_some() {
        return Err("Termina il comando corrente.".into());
    }
    if id == "ui.elettrico.unifilar.link.delete" {
        let object = app.run("electrical.unifilar.link.read", p.clone())?;
        app.elettrico.unifilar_connections = State { deleting: Some(object), error: None, placing: false };
        return Ok(json!({"open":true}));
    }
    let mut params = json!({"model":super::unifilar_browser::path(app)});
    let kind = match id {
        "ui.elettrico.unifilar.link.bus" => "bus",
        "ui.elettrico.unifilar.link.branch" => "branch",
        "ui.elettrico.unifilar.link.pe" => "pe",
        "ui.elettrico.unifilar.link.edit" => {
            let object = app.run("electrical.unifilar.link.read", p.clone())?;
            params["handle"] = object["handle"].clone();
            params["uid"] = object["uid"].clone();
            "series"
        }
        _ => "series",
    };
    params["kind"] = json!(kind);
    app.run("electrical.unifilar.link.prepare", params)?;
    app.start("electrical.unifilar.link.place");
    app.elettrico.unifilar_connections.placing = true;
    app.elettrico.open = false;
    app.elettrico.settings_open = false;
    super::unifilar_browser::hide(&mut app.elettrico.unifilar_browser);
    super::rooms::hide(&mut app.elettrico.rooms);
    super::manager::hide(&mut app.elettrico.manager);
    Ok(json!({"placing":true}))
}
pub(super) fn show(app: &mut CadApp, ctx: &egui::Context) {
    let active = app.session.running.as_ref().is_some_and(|r| r.id == "electrical.unifilar.link.place");
    if !active && app.elettrico.unifilar_connections.placing {
        app.session.unifilar_link = None;
        app.elettrico.unifilar_connections.placing = false;
    }
    if active && let Some(d) = app.session.unifilar_link.clone() {
        let mut action = None;
        let title = match d.link.kind.as_str() {
            "bus" => "Barra di distribuzione — Unifilare",
            "branch" => "Derivazione da barra — Unifilare",
            "pe" => "Collega al PE — Unifilare",
            _ => "Collega simboli — Unifilare",
        };
        egui::Window::new(title)
            .id(egui::Id::new("unifilar.connection.guide"))
            .default_pos(egui::pos2(ctx.content_rect().right() - 430.0, ctx.content_rect().top() + 120.0))
            .resizable(false)
            .default_width(390.0)
            .show(ctx, |ui| {
                ui.heading(d.description());
                ui.label(d.instruction());
                ui.label("I cerchi evidenziano i punti utilizzabili. Esc annulla.");
                if d.phase >= 2 && d.link.kind != "pe" {
                    let y = if d.phase == 2 { app.canvas.cursor.map(|p| p.y).unwrap_or(d.link.y) } else { d.link.y };
                    ui.label(format!("Altezza percorso: {y:.2} mm · scatto verticale F9"));
                }
                if let Some(x) = d.proposed_vertical_x() {
                    ui.label(format!("Passaggio verticale proposto: X {x:.2} mm"));
                }
                if let Some(error) = &d.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(d.phase > 0, egui::Button::new("Indietro")).clicked() {
                        action = Some("Indietro");
                    }
                    if d.phase == 3 {
                        if ui.button("Cambia origine").clicked() {
                            action = Some("Origine");
                        }
                        if matches!(d.link.kind.as_str(), "series" | "branch") && ui.button("Cambia arrivo").clicked() {
                            action = Some("Destinazione");
                        }
                        if d.link.kind != "pe" && ui.button(if d.link.kind == "bus" { "Cambia percorso" } else { "Cambia altezza" }).clicked() {
                            action = Some("Percorso");
                        }
                        if d.proposed_vertical_x().is_some() && ui.button("Cambia passaggio verticale").clicked() {
                            action = Some("Verticale");
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if d.phase == 5 && ui.button("Mantieni posizione proposta").clicked() {
                        action = Some("");
                    }
                    if ui
                        .add_enabled(
                            d.phase == 3 && d.error.is_none(),
                            egui::Button::new(if d.target.is_some() { "Salva modifiche" } else { "Conferma collegamento" }),
                        )
                        .clicked()
                    {
                        action = Some("");
                    }
                    if ui.button("Annulla").clicked() {
                        action = Some("cancel");
                    }
                });
            });
        if let Some(action) = action {
            if action == "cancel" {
                app.session.cancel();
                app.session.unifilar_link = None;
            } else {
                let _ = app.cmdline(action);
            }
        }
    }
    let mut state = std::mem::take(&mut app.elettrico.unifilar_connections);
    if let Some(object) = state.deleting.clone() {
        let mut open = true;
        let mut delete = false;
        let bus = object["link"]["kind"] == "bus";
        egui::Window::new("Elimina collegamento — Unifilare")
            .id(egui::Id::new("unifilar.connection.delete"))
            .open(&mut open)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label(object["description"].as_str().unwrap_or("Collegamento selezionato"));
                if bus {
                    ui.label("Verranno eliminate la barra e tutte le sue derivazioni.");
                }
                ui.label("Verranno eliminati anche gli eventuali marcatori collegati.");
                if let Some(error) = &state.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                delete = ui.button(if bus { "Elimina barra e derivazioni" } else { "Elimina collegamento" }).clicked();
            });
        if delete {
            let mut p = object;
            p["cascade"] = json!(bus);
            match app.run("electrical.unifilar.link.delete", p) {
                Ok(_) => open = false,
                Err(e) => state.error = Some(e),
            }
        }
        if !open {
            state.deleting = None;
            state.error = None;
        }
    }
    app.elettrico.unifilar_connections = state;
}
