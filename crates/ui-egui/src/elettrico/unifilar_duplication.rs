//! Native duplication setup; scope, UUIDs, geometry and Undo are owned by the engine.
use crate::CadApp;
use serde_json::{Value, json};
#[derive(Default)]
pub(super) struct State {
    open: bool,
    circuit: bool,
    copies: usize,
    step: i64,
    renumber: bool,
    root: Option<String>,
    uid: u64,
    error: Option<String>,
}
pub(super) fn command(app: &mut CadApp, id: &str, p: &Value) -> Result<Value, String> {
    if app.session.running.is_some() || app.elettrico.editing.snapshot.is_some() {
        return Err("Termina il comando corrente.".into());
    }
    app.elettrico.open = false;
    app.elettrico.settings_open = false;
    super::unifilar_browser::hide(&mut app.elettrico.unifilar_browser);
    app.elettrico.unifilar_duplication = State {
        open: true,
        circuit: id.ends_with(".circuit"),
        copies: if id.ends_with(".series") { 3 } else { 1 },
        step: 1,
        renumber: true,
        root: p["handle"].as_str().map(str::to_owned),
        uid: app.session.state().map_err(|e| e.to_string())?.uid,
        error: None,
    };
    Ok(json!({"open":true}))
}
pub(super) fn show(app: &mut CadApp, ctx: &egui::Context) {
    if app.session.running.as_ref().is_some_and(|r| r.id == "electrical.unifilar.duplicate.place") {
        let mut cancel = false;
        if let Some(d) = &app.session.unifilar_duplicate {
            egui::Window::new("Posiziona duplicazione — Unifilare").resizable(false).default_width(360.).show(ctx, |ui| {
                ui.heading(&d.summary);
                ui.label("Gli oggetti sorgente sono evidenziati. Scegli la colonna della prima copia e controlla l'anteprima.");
                ui.label("Clicca per inserire. Annulla ripristina l'intera operazione.");
                if let Some(error) = &d.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                cancel = ui.button("Annulla").clicked();
            });
        }
        if cancel {
            app.session.cancel();
        }
        return;
    }
    if !app.elettrico.unifilar_duplication.open {
        return;
    }
    let mut st = std::mem::take(&mut app.elettrico.unifilar_duplication);
    if app.session.state().is_ok_and(|s| s.uid != st.uid) {
        st.root = None;
        st.uid = app.session.state().map(|s| s.uid).unwrap_or(0);
    }
    let picked = app.session.selection();
    if picked.len() == 1
        && let Some(h) = picked.first()
        && let Ok(doc) = app.session.doc()
        && let Some(e) = doc.entity(*h)
        && let Some(m) = cadcraft_engine::project::references::data(e, "CC_UNIFILARE_DEVICE")
        && m["kind"] == "device"
        && m["column"].is_u64()
        && !cadcraft_engine::project::references::is_reference(e)
    {
        st.root = Some(h.hex());
    }
    let title = if st.circuit { "Duplica circuito — Unifilare" } else { "Duplica colonna — Unifilare" };
    let mut open = true;
    let mut start = false;
    egui::Window::new(title).id(egui::Id::new("unifilar.duplicate.setup")).open(&mut open).default_width(400.).resizable(false).show(ctx, |ui| {
        ui.label(if st.circuit {
            "Seleziona nel disegno il simbolo iniziale: verrà individuato il circuito a valle."
        } else {
            "Seleziona un apparecchio della colonna da copiare. I collegamenti provenienti da altre colonne saranno esclusi."
        });
        if let Some(h) = &st.root {
            let label = app
                .session
                .doc()
                .ok()
                .and_then(|d| cadcraft_engine::doc::Handle::parse_hex(h).and_then(|h| d.entity(h)))
                .and_then(|e| cadcraft_engine::project::references::data(e, "CC_UNIFILARE_DEVICE"))
                .map(|m| {
                    format!(
                        "{} — {} · colonna {}",
                        m["fields"]["sigla"].as_str().unwrap_or(""),
                        m["name"].as_str().unwrap_or("Apparecchio"),
                        m["column"]
                    )
                })
                .unwrap_or_else(|| "Seleziona un apparecchio".into());
            ui.label(label);
        } else {
            ui.colored_label(ui.visuals().warn_fg_color, "Seleziona un apparecchio nel disegno; questa finestra può restare aperta.");
        }
        if !st.circuit {
            ui.horizontal(|ui| {
                ui.label("Numero copie (oltre all'originale)");
                ui.add(egui::DragValue::new(&mut st.copies).range(1..=32));
            });
            if st.copies > 1 {
                ui.horizontal(|ui| {
                    ui.label("Passo in colonne");
                    ui.add(egui::DragValue::new(&mut st.step).range(-128..=128));
                });
                ui.small("Passo positivo: verso destra. Passo negativo: verso sinistra.");
            }
        }
        ui.checkbox(&mut st.renumber, "Rinumera automaticamente le sigle nella tavola");
        ui.small("Ogni copia riceve nuove identità, indipendenti dalle sigle visibili.");
        if let Some(error) = &st.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        start =
            ui.add_enabled(st.root.is_some() && app.session.running.is_none() && st.step != 0, egui::Button::new("Anteprima e posiziona")).clicked();
    });
    if start {
        let params = json!({"model":super::unifilar_browser::path(app),"handle":st.root,"mode":if st.circuit{"circuit"}else{"column"},"copies":st.copies,"step":st.step,"renumber":st.renumber});
        match app.run("electrical.unifilar.duplicate.prepare", params) {
            Ok(_) => {
                app.start("electrical.unifilar.duplicate.place");
                open = false;
            }
            Err(e) => st.error = Some(e),
        }
    }
    st.open = open;
    app.elettrico.unifilar_duplication = st;
}
