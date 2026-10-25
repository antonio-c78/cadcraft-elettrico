//! The engine owns circuit scope, preview, validation, backup and the single Undo step.
use crate::CadApp;
use serde_json::{Value, json};
#[derive(Default)]
pub(super) struct State {
    open: bool,
    circuit: bool,
    params: Value,
    error: Option<String>,
    uid: u64,
}
pub(super) fn command(app: &mut CadApp, id: &str, p: &Value) -> Result<Value, String> {
    if app.session.running.is_some() || app.elettrico.editing.snapshot.is_some() {
        return Err("Termina il comando corrente.".into());
    }
    let mut params = p.clone();
    if !params.is_object() {
        params = json!({});
    }
    params["model"] = json!(super::unifilar_browser::path(app));
    let circuit = id == "ui.elettrico.unifilar.circuit.move";
    params["mode"] = json!(if circuit { "circuit" } else { "column" });
    app.elettrico.unifilar_circuits = State { open: true, circuit, params, uid: app.session.state().map_err(|e| e.to_string())?.uid, error: None };
    app.elettrico.open = false;
    app.elettrico.settings_open = false;
    super::unifilar_browser::hide(&mut app.elettrico.unifilar_browser);
    Ok(json!({"open":true}))
}
pub(super) fn show(app: &mut CadApp, ctx: &egui::Context) {
    if !app.session.running.as_ref().is_some_and(|r| r.id == "electrical.unifilar.circuit.place") {
        setup(app, ctx);
        return;
    }
    let Some(d) = app.session.unifilar_circuit.as_ref() else { return };
    let mut cancel = false;
    egui::Window::new(if d.circuit { "Sposta circuito — Unifilare" } else { "Sposta colonna — Unifilare" })
        .id(egui::Id::new("unifilar.circuit.guide"))
        .default_pos(egui::pos2(ctx.content_rect().right() - 420., ctx.content_rect().top() + 120.))
        .resizable(false)
        .default_width(380.)
        .show(ctx, |ui| {
            ui.heading(&d.summary);
            ui.label(if d.circuit {
                "Scegli la nuova colonna del simbolo iniziale e controlla l’anteprima."
            } else {
                "Scegli la destinazione della prima colonna selezionata e controlla l’anteprima."
            });
            ui.label("Il gruppo mantiene distanze e altezze. Clicca per confermare.");
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

fn setup(app: &mut CadApp, ctx: &egui::Context) {
    if !app.elettrico.unifilar_circuits.open {
        return;
    }
    let mut st = std::mem::take(&mut app.elettrico.unifilar_circuits);
    if !app.session.state().is_ok_and(|s| s.uid == st.uid) {
        return;
    }
    let picked = app.session.selection();
    let mut open = true;
    let mut start = false;
    let title = if st.circuit { "Sposta circuito — Unifilare" } else { "Sposta colonna — Unifilare" };
    egui::Window::new(title).id(egui::Id::new("unifilar.move.setup")).open(&mut open).resizable(false).default_width(400.).show(ctx, |ui| {
        ui.label(if st.circuit {
            "Seleziona nel disegno il simbolo iniziale: verrà individuato il circuito a valle."
        } else {
            "Seleziona un apparecchio della colonna da spostare. Puoi selezionare apparecchi di più colonne per spostarle insieme."
        });
        for h in picked.iter().take(8) {
            if let Ok(doc) = app.session.doc()
                && let Some(e) = doc.entity(*h)
                && let Some(m) = cadcraft_engine::project::references::data(e, "CC_UNIFILARE_DEVICE")
            {
                ui.label(format!(
                    "{} — {} · colonna {}",
                    m["fields"]["sigla"].as_str().unwrap_or(""),
                    m["name"].as_str().unwrap_or("Apparecchio"),
                    m["column"]
                ));
            }
        }
        ui.small("Seguono testi, dati e fili, compreso il collegamento a monte. Le identità degli oggetti vengono conservate.");
        if let Some(error) = &st.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        start = ui
            .add_enabled(
                !picked.is_empty() && (!st.circuit || picked.len() == 1) && app.session.running.is_none(),
                egui::Button::new("Anteprima e posiziona"),
            )
            .clicked();
    });
    if start {
        // Use the current selection, including changes made with this window open.
        if let Some(p) = st.params.as_object_mut() {
            p.remove("handle");
            p.remove("handles");
        }
        match app.run("electrical.unifilar.circuit.prepare", st.params.clone()) {
            Ok(_) => {
                app.start("electrical.unifilar.circuit.place");
                open = false;
            }
            Err(e) => st.error = Some(e),
        }
    }
    st.open = open;
    app.elettrico.unifilar_circuits = st;
}
