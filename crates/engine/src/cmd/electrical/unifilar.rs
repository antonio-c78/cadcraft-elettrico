//! A new unifilar drawing is fully validated before it becomes an independent unsaved tab.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
pub mod placement;
mod schema;
pub use schema::Cell;
const CMD: &str = "electrical.unifilar.new";

#[derive(Deserialize)]
struct Model {
    format: String,
    version: u32,
    id: String,
    units: String,
    template: String,
    title_fields: Vec<Field>,
}
#[derive(Deserialize)]
struct Field {
    id: String,
    tag: String,
    position: [f64; 2],
    width_mm: f64,
    text_height_mm: f64,
}
pub(super) fn specs() -> Vec<CommandSpec> {
    let mut commands = vec![
        CommandSpec::new(CMD, "New electrical unifilar drawing", create)
            .noundo()
            .params("{model: path_to_modello.json, fields?:{impresa,recapiti,cliente,indirizzo,quadro,titolo,data,revisione,progetto,foglio}}"),
    ];
    commands.extend(placement::specs());
    commands
}
#[cfg(not(target_arch = "wasm32"))]
fn read(path: &std::path::Path, limit: u64) -> Result<Vec<u8>> {
    use std::io::Read;
    let f = std::fs::File::open(path).map_err(|e| bad(CMD, e.to_string()))?;
    let mut bytes = Vec::new();
    f.take(limit + 1).read_to_end(&mut bytes).map_err(|e| bad(CMD, e.to_string()))?;
    if bytes.len() as u64 > limit {
        return Err(bad(CMD, "File modello troppo grande."));
    }
    Ok(bytes)
}
#[cfg(target_arch = "wasm32")]
fn read(_: &std::path::Path, _: u64) -> Result<Vec<u8>> {
    Err(bad(CMD, "I modelli locali richiedono la versione desktop."))
}
fn create(s: &mut Session, p: &Value) -> Result<Value> {
    if s.running.is_some() || s.state().is_ok_and(|d| d.block_edit.is_some()) {
        return Err(bad(CMD, "Termina il comando o la modifica corrente."));
    }
    let path = p["model"].as_str().filter(|p| !p.is_empty() && p.len() <= 32768).ok_or_else(|| bad(CMD, "Indica il modello esterno."))?;
    let model_path = std::path::Path::new(path);
    let model_bytes = read(model_path, 524288)?;
    let raw_model: Value = serde_json::from_slice(&model_bytes).map_err(|e| bad(CMD, e.to_string()))?;
    let model: Model = serde_json::from_slice(&model_bytes).map_err(|e| bad(CMD, e.to_string()))?;
    if model.format != "cadcraft-unifilar-template"
        || model.version != 1
        || model.units != "mm"
        || model.id.is_empty()
        || model.id.len() > 128
        || model.title_fields.len() != 10
    {
        return Err(bad(CMD, "Modello unifilare non supportato."));
    }
    let file = std::path::Path::new(&model.template);
    if file.components().count() != 1 || !matches!(file.components().next(), Some(std::path::Component::Normal(_))) {
        return Err(bad(CMD, "Il DXF deve essere nella cartella del modello."));
    }
    let template = model_path.parent().unwrap_or_else(|| std::path::Path::new(".")).join(file);
    let bytes = read(&template, 10 * 1024 * 1024)?;
    let hooks = crate::cmd::file::io().ok_or_else(|| bad(CMD, "Formati file non disponibili."))?;
    let mut drawing = (hooks.read)(&bytes, &template.to_string_lossy()).map_err(|e| bad(CMD, e))?;
    if drawing.header.i64("INSUNITS", 0) != 4 || drawing.entity_count() > 10000 {
        return Err(bad(CMD, "Il modello deve essere in millimetri e contenere al massimo 10000 oggetti."));
    }
    let fields: BTreeMap<String, String> =
        serde_json::from_value(p.get("fields").cloned().unwrap_or_else(|| json!({}))).map_err(|e| bad(CMD, e.to_string()))?;
    if fields.len() > 10 {
        return Err(bad(CMD, "Troppi campi del cartiglio."));
    }
    let mut ids = BTreeSet::new();
    let mut tags = BTreeSet::new();
    for f in &model.title_fields {
        if f.id.is_empty()
            || f.id.len() > 64
            || f.tag.is_empty()
            || f.tag.len() > 64
            || !ids.insert(f.id.as_str())
            || !tags.insert(f.tag.as_str())
            || !f.width_mm.is_finite()
            || !(10.0..=200.0).contains(&f.width_mm)
            || !f.text_height_mm.is_finite()
            || !(1.0..=6.0).contains(&f.text_height_mm)
            || f.position.iter().any(|v| !v.is_finite() || !(0.0..=420.0).contains(v))
        {
            return Err(bad(CMD, "Definizione dei campi non valida."));
        }
    }
    if fields.keys().any(|k| !ids.contains(k.as_str())) {
        return Err(bad(CMD, "Campo del cartiglio sconosciuto."));
    }
    let references: Vec<_> = drawing
        .model
        .handles()
        .iter()
        .copied()
        .filter(|h| drawing.entity(*h).is_some_and(|e| e.common.xdata.iter().any(|t| t.code == 1001 && t.value == "CC_UNIFILARE_MODELO")))
        .collect();
    let handle =
        references.first().copied().filter(|_| references.len() == 1).ok_or_else(|| bad(CMD, "Cartiglio unifilare mancante o duplicato."))?;
    let mut entity = drawing.entity(handle).ok_or_else(|| bad(CMD, "Cartiglio assente."))?.as_ref().clone();
    let EntityKind::Insert(ref mut insert) = entity.kind else {
        return Err(bad(CMD, "Cartiglio non valido."));
    };
    if insert.attribs.len() != 10 {
        return Err(bad(CMD, "Attributi del cartiglio incompleti."));
    }
    for f in &model.title_fields {
        let value = fields.get(&f.id).map(|v| v.trim()).unwrap_or("");
        if value.chars().count() > 256 || value.chars().any(char::is_control) {
            return Err(bad(CMD, format!("Il campo '{}' deve contenere una sola riga, massimo 256 caratteri.", f.id)));
        }
        if cadcraft_fonts::line_width(value, f.text_height_mm, 1.0) > f.width_mm - 1.5 {
            return Err(bad(
                CMD,
                format!("Il testo del campo '{}' supera lo spazio disponibile. Abbrevialo: il testo non viene ridotto automaticamente.", f.id),
            ));
        }
        let matches: Vec<_> = insert.attribs.iter().enumerate().filter(|(_, a)| a.tag == f.tag).map(|(i, _)| i).collect();
        let i = matches.first().copied().filter(|_| matches.len() == 1).ok_or_else(|| bad(CMD, "Attributo mancante o duplicato."))?;
        let a = insert.attribs.get_mut(i).ok_or_else(|| bad(CMD, "Attributo assente."))?;
        if a.multiline.is_some() {
            return Err(bad(CMD, "Il cartiglio richiede attributi a riga singola."));
        }
        a.text.value = value.into();
        a.text.height = f.text_height_mm;
        a.text.width_factor = 1.0;
        a.text.insert = Vec3::new(f.position[0], f.position[1], 0.0);
        a.text.align_pt = None;
        a.text.rotation = 0.0;
        a.text.halign = HAlign::Left;
        a.text.valign = VAlign::Baseline;
    }
    if raw_model.get("columns").is_some() {
        let layout: schema::Layout = serde_json::from_value(raw_model.clone()).map_err(|e| bad(CMD, e.to_string()))?;
        schema::validate(&layout)?;
        placement::tags(&mut entity.common, "CC_UNIFILARE_LAYOUT", &schema::snapshot(&layout)?);
    }
    let baseline = Arc::new(drawing.clone());
    drawing.modify_entity(handle, |e| *e = entity)?;
    let title = fields.get("quadro").map(|v| v.trim()).filter(|v| !v.is_empty()).unwrap_or("Schema unifilare").to_string();
    let index = s.open_drawing(drawing, &title, None);
    let state = s.state_mut()?;
    state.saved = baseline;
    Ok(json!({"index":index,"uid":state.uid,"title":title,"model":model.id,"columns":10,"unsaved":true}))
}
