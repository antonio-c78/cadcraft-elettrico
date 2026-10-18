//! A single guided window: scope, two snapped points, real length, confirmation, optional check.
use crate::CadApp;
use cadcraft_engine::{doc::Handle, geom::Vec2};
use serde_json::{Value, json};

#[derive(Default)]
pub(super) struct State {
    open: bool,
    uid: u64,
    original: Vec<Handle>,
    selected_only: bool,
    plan: Value,
    points: Vec<Vec2>,
    length: String,
    unit: String,
    model_unit: String,
    phase: u8,
    picking: bool,
    checking: bool,
    check_distance: Option<f64>,
    expected: String,
    result: Value,
    error: Option<String>,
}

fn mm(unit: &str) -> f64 {
    match unit {
        "m" => 1000.0,
        "cm" => 10.0,
        _ => 1.0,
    }
}
fn number(text: &str) -> Option<f64> {
    text.trim().replace(',', ".").parse::<f64>().ok().filter(|n| n.is_finite() && *n > 0.0 && *n <= 1e12)
}
fn units(ui: &mut egui::Ui, label: &str, value: &mut String) {
    egui::ComboBox::from_id_salt(label).selected_text(if value.is_empty() { "Seleziona..." } else { value.as_str() }).show_ui(ui, |ui| {
        for unit in ["mm", "cm", "m"] {
            ui.selectable_value(value, unit.into(), unit);
        }
    });
}

pub(super) fn command(app: &mut CadApp) -> Result<Value, String> {
    if app.session.running.is_some() || app.ui.start_tab || app.session.docs.is_empty() || app.elettrico.editing.snapshot.is_some() {
        return Err("Apri una planimetria e termina la modifica corrente.".into());
    }
    let doc = app.session.state().map_err(|e| e.to_string())?;
    let model_unit = match doc.doc.header.i64("INSUNITS", 0) {
        4 => "mm",
        5 => "cm",
        6 => "m",
        _ => "",
    };
    let state =
        State { open: true, uid: doc.uid, original: app.session.selection(), model_unit: model_unit.into(), unit: "cm".into(), ..Default::default() };
    app.elettrico.open = false;
    app.elettrico.settings_open = false;
    app.elettrico.symbol.open = false;
    super::manager::hide(&mut app.elettrico.manager);
    super::rooms::hide(&mut app.elettrico.rooms);
    app.elettrico.calibration = state;
    Ok(json!({"open":true}))
}

fn pick(app: &mut CadApp, state: &mut State, check: bool) -> Result<(), String> {
    let mut p = json!({"scope":if state.selected_only && !check{"selected"}else{"all"},"check":check});
    if !check && state.phase == 2 && state.plan["handles"].is_array() {
        p["handles"] = state.plan["handles"].clone();
    }
    let plan = app.run("electrical.calibration.prepare", p)?;
    if !check {
        state.plan = plan;
        state.points.clear();
    }
    state.picking = true;
    state.checking = check;
    state.error = None;
    app.start("electrical.calibration.pick");
    Ok(())
}

pub(super) fn show(app: &mut CadApp, ctx: &egui::Context) {
    if !app.elettrico.calibration.open {
        return;
    }
    let mut state = std::mem::take(&mut app.elettrico.calibration);
    if state.picking && app.session.running.is_none() {
        state.picking = false;
        let points = app.session.electrical_measure.as_ref().map(|d| d.points.clone()).unwrap_or_default();
        if points.len() == 2 {
            if state.checking {
                state.check_distance = points.first().zip(points.get(1)).map(|(a, b)| a.dist(*b));
            } else {
                state.points = points;
                state.phase = 2;
            }
        } else {
            state.error = Some("Misura annullata. Puoi scegliere nuovamente i due punti.".into());
        }
    }
    let valid_doc = app.session.state().is_ok_and(|d| d.uid == state.uid && state.plan["revision"].as_u64().is_none_or(|r| r == d.revision));
    let mut open = true;
    let mut close = false;
    let mut action = None;
    egui::Window::new("Scala da misura nota — Elettrico")
        .id(egui::Id::new("electrical.calibration"))
        .open(&mut open)
        .default_width(510.0)
        .resizable(false)
        .show(ctx, |ui| {
            if !valid_doc {
                ui.colored_label(super::Tokens::get().warn, "Il disegno è cambiato. Chiudi e riapri lo strumento.");
            }
            if state.picking {
                let count = app.session.electrical_measure.as_ref().map(|d| d.points.len()).unwrap_or(0);
                ui.heading(if count == 0 { "Indica il primo punto" } else { "Indica il secondo punto" });
                ui.label("Fai clic sui due estremi della distanza nota. Usa gli agganci del CAD per essere preciso.");
                if !state.checking {
                    ui.label(format!("Oggetti evidenziati: {}. Saranno scalati alla conferma.", state.plan["count"]));
                }
            } else if state.phase == 0 {
                ui.heading("1 · Scegli la planimetria");
                ui.radio_value(&mut state.selected_only, false, "Tutta la planimetria (spazio modello)");
                ui.radio_value(&mut state.selected_only, true, "Solo gli oggetti selezionati nel CAD");
                ui.label("Se scegli una selezione, preparala nel disegno prima di continuare.");
                ui.label("La scalatura comprende anche gli oggetti su layer nascosti. I layer bloccati devono essere sbloccati.");
                ui.horizontal(|ui| {
                    ui.label("Unità del modello");
                    units(ui, "calibration_model_unit", &mut state.model_unit);
                });
                ui.label("L'unità scelta viene registrata nel disegno alla conferma.");
                if ui.add_enabled(valid_doc && !state.model_unit.is_empty(), egui::Button::new("2 · Evidenzia e scegli due punti")).clicked() {
                    action = Some("pick");
                }
            } else if state.phase == 2 {
                ui.heading("3 · Indica la misura reale");
                let distance = state.points.first().zip(state.points.get(1)).map(|(a, b)| a.dist(*b)).unwrap_or(0.0);
                ui.label(format!("Distanza rilevata: {distance:.6} {}", state.model_unit));
                ui.horizontal(|ui| {
                    ui.label("Misura reale");
                    ui.add(egui::TextEdit::singleline(&mut state.length).char_limit(32).desired_width(130.0));
                    units(ui, "calibration_real_unit", &mut state.unit);
                });
                let factor = number(&state.length)
                    .map(|v| v * mm(&state.unit) / mm(&state.model_unit) / distance)
                    .filter(|f| f.is_finite() && (1e-9..=1e9).contains(f));
                if let Some(factor) = factor {
                    ui.label(format!("Fattore di scala: {factor:.8} · oggetti: {}", state.plan["count"]));
                }
                ui.label("Il primo punto rimane fisso. Le proporzioni vengono mantenute.");
                if state.plan["electrical"].as_u64().unwrap_or(0) > 0 {
                    ui.colored_label(
                        super::Tokens::get().warn,
                        format!(
                            "Oggetti Elettrico inclusi: {} (scatole, simboli o ambienti). Verranno scalati insieme alla planimetria.",
                            state.plan["electrical"]
                        ),
                    );
                }
                ui.label("Alla conferma viene creata una copia di sicurezza. Puoi annullare l'operazione con un solo Annulla.");
                ui.horizontal(|ui| {
                    if ui.add_enabled(valid_doc && factor.is_some(), egui::Button::new("Applica scala")).clicked() {
                        action = Some("apply");
                    }
                    if ui.add_enabled(valid_doc, egui::Button::new("Ripeti misura")).clicked() {
                        action = Some("pick");
                    }
                    if ui.button("Cambia selezione").clicked() {
                        state.phase = 0;
                        state.plan = Value::Null;
                    }
                });
            } else if state.phase == 3 {
                ui.heading("Planimetria scalata");
                ui.label(format!("Fattore applicato: {:.8}", state.result["factor"].as_f64().unwrap_or(0.0)));
                ui.label("Copia di sicurezza creata. Annulla nel CAD ripristina l'operazione.");
                if ui.add_enabled(valid_doc, egui::Button::new("Misura di controllo · scegli due punti")).clicked() {
                    action = Some("check");
                }
                if let Some(distance) = state.check_distance {
                    let measured = distance * mm(&state.model_unit) / mm(&state.unit);
                    ui.horizontal(|ui| {
                        ui.label(format!("Distanza di controllo: {measured:.6}"));
                        units(ui, "calibration_check_unit", &mut state.unit);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Misura attesa (facoltativa)");
                        ui.add(egui::TextEdit::singleline(&mut state.expected).char_limit(32).desired_width(120.0));
                    });
                    if let Some(expected) = number(&state.expected) {
                        ui.label(format!("Differenza: {:+.6} {}", measured - expected, state.unit));
                    }
                }
            }
            if let Some(error) = &state.error {
                ui.colored_label(super::Tokens::get().warn, error);
            }
            close = ui.button(if state.phase == 3 { "Chiudi" } else { "Annulla" }).clicked();
        });
    open &= !close;
    if let Some(action) = action {
        let result = (|| {
            if !valid_doc {
                return Err("Il disegno è cambiato. Riapri lo strumento.".into());
            }
            match action {
                "pick" => pick(app, &mut state, false)?,
                "check" => pick(app, &mut state, true)?,
                "apply" => {
                    let mut params = state.plan.clone();
                    let a = state.points.first().ok_or("Indica il primo punto.")?;
                    let b = state.points.get(1).ok_or("Indica il secondo punto.")?;
                    params["p1"] = json!([a.x, a.y]);
                    params["p2"] = json!([b.x, b.y]);
                    params["length"] = json!(number(&state.length).ok_or("Indica una misura reale positiva.")?);
                    params["unit"] = json!(state.unit);
                    params["model_unit"] = json!(state.model_unit);
                    state.result = app.run("electrical.calibration.apply", params)?;
                    state.phase = 3;
                    let _ = app.run("zoom.extents", json!({}));
                    state.plan["revision"] = json!(app.session.state().map_err(|e| e.to_string())?.revision);
                }
                _ => {}
            }
            Ok::<(), String>(())
        })();
        state.error = result.err();
    }
    if !open {
        if state.picking && app.session.running.as_ref().is_some_and(|r| r.id == "electrical.calibration.pick") {
            app.session.cancel();
        }
        if app.session.state().is_ok_and(|d| d.uid == state.uid) {
            let original = state.original.iter().copied().filter(|h| app.session.doc().is_ok_and(|d| d.entity(*h).is_some())).collect();
            app.session.set_selection(original);
        }
        app.session.electrical_measure = None;
        state = State::default();
    }
    app.elettrico.calibration = state;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn calibration_is_first_in_the_electrical_menu_and_accepts_decimal_comma() {
        let app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        let menu = crate::menus::tree(&app).into_iter().find(|(name, _)| name == "Elettrico").unwrap().1;
        assert!(matches!(menu.first(),Some(crate::menus::Entry::Item{id,..})if id=="ui.elettrico.calibrate"));
        assert_eq!(number("80,5"), Some(80.5));
        for invalid in ["", "NaN", "inf", "0", "-80", "1e300"] {
            assert!(number(invalid).is_none());
        }
    }

    #[test]
    fn cancelled_scope_can_change_but_remeasurement_keeps_targets_and_check_needs_no_selection() {
        let mut app = CadApp::new(cadcraft_engine::Session::new(), crate::Services::default());
        app.ui.start_tab = false;
        app.session.execute("line", &json!({"points":[[0,0],[10,0]]})).unwrap();
        app.session.execute("line", &json!({"points":[[0,10],[10,10]]})).unwrap();
        let handles = app.session.doc().unwrap().model.handles().to_vec();
        let first = *handles.first().unwrap();
        let second = *handles.get(1).unwrap();
        command(&mut app).unwrap();
        let mut state = std::mem::take(&mut app.elettrico.calibration);
        pick(&mut app, &mut state, false).unwrap();
        assert_eq!(state.plan["count"], 2);
        app.session.cancel();
        state.selected_only = true;
        app.session.set_selection(vec![first]);
        pick(&mut app, &mut state, false).unwrap();
        assert_eq!(state.plan["handles"], json!([first.hex()]));
        app.session.cancel();
        state.phase = 2;
        app.session.set_selection(vec![second]);
        pick(&mut app, &mut state, false).unwrap();
        assert_eq!(state.plan["handles"], json!([first.hex()]));
        app.session.cancel();
        state.phase = 3;
        app.session.set_selection(vec![]);
        pick(&mut app, &mut state, true).unwrap();
        assert!(app.session.running.is_some());
        assert!(app.session.selection().is_empty());
        app.session.cancel();
    }
}
