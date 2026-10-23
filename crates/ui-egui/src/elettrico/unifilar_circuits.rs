//! The engine owns circuit scope, preview, validation, backup and the single Undo step.
use crate::CadApp;
use serde_json::{Value, json};
pub(super) fn command(app: &mut CadApp, p: &Value) -> Result<Value, String> {
    if app.session.running.is_some() {
        return Err("Termina il comando corrente.".into());
    }
    let mut params = p.clone();
    if !params.is_object() {
        params = json!({});
    }
    params["model"] = json!(super::unifilar_browser::path(app));
    let scope = app.run("electrical.unifilar.circuit.prepare", params)?;
    app.start("electrical.unifilar.circuit.place");
    app.elettrico.open = false;
    app.elettrico.settings_open = false;
    super::unifilar_browser::hide(&mut app.elettrico.unifilar_browser);
    Ok(scope)
}
pub(super) fn show(app: &mut CadApp, ctx: &egui::Context) {
    if !app.session.running.as_ref().is_some_and(|r| r.id == "electrical.unifilar.circuit.place") {
        return;
    }
    let Some(d) = app.session.unifilar_circuit.as_ref() else { return };
    let mut cancel = false;
    egui::Window::new("Sposta circuito — Unifilare")
        .id(egui::Id::new("unifilar.circuit.guide"))
        .default_pos(egui::pos2(ctx.content_rect().right() - 420., ctx.content_rect().top() + 120.))
        .resizable(false)
        .default_width(380.)
        .show(ctx, |ui| {
            ui.heading(&d.summary);
            ui.label("Sposta il cursore su una colonna libera e controlla l’anteprima.");
            ui.label("Clicca per confermare. L’altezza dei dispositivi rimane invariata.");
            ui.label("Seguono testi, dati e fili, compreso il collegamento a monte. Le altre colonne restano ferme.");
            if let Some(error) = &d.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.separator();
            cancel = ui.button("Annulla").clicked();
        });
    if cancel {
        app.session.cancel();
        app.session.unifilar_circuit = None;
    }
}
