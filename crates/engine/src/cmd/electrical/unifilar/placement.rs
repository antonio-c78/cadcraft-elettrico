//! Atomic unifilar placement: one device owns its graphics, label, table cells and metadata.
use super::super::*;
use super::schema::{self, Cell, Column, Layout, Symbol};
use crate::EngineError;
use cadcraft_doc::Handle;
use std::collections::BTreeMap;
const CMD: &str = "electrical.unifilar.prepare";
const APP: &str = "CC_UNIFILARE_DEVICE";
#[derive(Clone)]
pub struct Draft {
    uid: u64,
    baseline: Arc<cadcraft_doc::Drawing>,
    layout: Layout,
    symbol: Symbol,
    geometry: Vec<EntityKind>,
    fields: BTreeMap<String, String>,
    legacy_fields: BTreeMap<String, String>,
    target: Option<Handle>,
    backup_dir: Option<String>,
    pub error: Option<String>,
}
pub(super) fn data(e: &Entity, app: &str) -> Result<Option<Value>> {
    let mut active = false;
    let mut text = String::new();
    for t in &e.common.xdata {
        if t.code == 1001 {
            active = t.value == app;
        } else if active && t.code == 1000 {
            if text.len() + t.value.len() > 32768 {
                return Err(bad(CMD, "Dati unifilari troppo grandi."));
            }
            text.push_str(&t.value);
        }
    }
    if text.is_empty() { Ok(None) } else { serde_json::from_str(&text).map(Some).map_err(|e| bad(CMD, e.to_string())) }
}
pub(super) fn tags(common: &mut Common, app: &str, value: &Value) {
    let mut active = false;
    common.xdata.retain(|t| {
        if t.code == 1001 {
            active = t.value == app;
        }
        !active
    });
    common.xdata.push(RawTag { code: 1001, value: app.into() });
    let mut chunk = String::new();
    for ch in value.to_string().chars() {
        if chunk.len() + ch.len_utf8() > 200 {
            common.xdata.push(RawTag { code: 1000, value: std::mem::take(&mut chunk) });
        }
        chunk.push(ch);
    }
    if !chunk.is_empty() {
        common.xdata.push(RawTag { code: 1000, value: chunk });
    }
}
pub(super) fn sheet(s: &Session, fallback: &Layout) -> Result<Layout> {
    if s.space() != Space::Model || s.state()?.block_edit.is_some() {
        return Err(bad(CMD, "Passa allo spazio modello dello schema unifilare."));
    }
    let doc = s.doc()?;
    if doc.header.i64("INSUNITS", 0) != 4 {
        return Err(bad(CMD, "La tavola unifilare deve essere in millimetri."));
    }
    if doc.entity_count() > 10000 {
        return Err(bad(CMD, "Tavola troppo complessa."));
    }
    let refs: Vec<_> = doc.model.iter().filter(|e| e.common.xdata.iter().any(|t| t.code == 1001 && t.value == "CC_UNIFILARE_MODELO")).collect();
    let e = refs.first().filter(|_| refs.len() == 1).ok_or_else(|| bad(CMD, "Apri una tavola creata con Nuovo schema unifilare."))?;
    let EntityKind::Insert(i) = &e.kind else {
        return Err(bad(CMD, "Cartiglio non valido."));
    };
    if i.insert != Vec3::ZERO || i.scale != Vec3::new(1.0, 1.0, 1.0) || i.rotation != 0.0 {
        return Err(bad(CMD, "Il modello è stato spostato o scalato. Ripristinalo prima di inserire apparecchi."));
    }
    let model_id = e
        .common
        .xdata
        .iter()
        .skip_while(|t| t.code != 1001 || t.value != "CC_UNIFILARE_MODELO")
        .skip(1)
        .find(|t| t.code == 1000)
        .map(|t| t.value.as_str());
    if model_id != Some(fallback.id.as_str()) {
        return Err(bad(CMD, "Il modello scelto non corrisponde alla tavola."));
    }
    let layout = if let Some(raw) = data(e, "CC_UNIFILARE_LAYOUT")? { schema::restore(raw)? } else { fallback.clone() };
    schema::validate(&layout)?;
    Ok(layout)
}
fn handle(s: &Session, p: &Value) -> Result<Handle> {
    if let Some(value) = p.get("handle") {
        return value.as_str().and_then(Handle::parse_hex).ok_or_else(|| bad(CMD, "Riferimento non valido."));
    }
    let h = s.selection();
    h.first().copied().filter(|_| h.len() == 1).ok_or_else(|| bad(CMD, "Seleziona un apparecchio unifilare."))
}

fn metadata(s: &Session, h: Handle) -> Result<Value> {
    let e = s
        .doc()?
        .entity(h)
        .filter(|_| s.doc().is_ok_and(|d| d.space_of(h) == Some(Space::Model)))
        .ok_or_else(|| bad(CMD, "Apparecchio non disponibile."))?;
    let m = data(e, APP)?.ok_or_else(|| bad(CMD, "Non è un apparecchio unifilare."))?;
    let EntityKind::Insert(i) = &e.kind else {
        return Err(bad(CMD, "Apparecchio non valido."));
    };
    let at: [f64; 2] = serde_json::from_value(m["at"].clone()).map_err(|e| bad(CMD, e.to_string()))?;
    if i.insert.xy() != Vec2::new(at[0], at[1]) || i.rotation != 0.0 || i.scale != Vec3::new(1.0, 1.0, 1.0) {
        return Err(bad(CMD, "L'oggetto è stato trasformato con comandi generici. Annulla la trasformazione e usa Sposta apparecchio."));
    }
    Ok(m)
}
pub(super) fn read_object(s: &mut Session, p: &Value) -> Result<Value> {
    let h = handle(s, p)?;
    let mut m = metadata(s, h)?;
    m["handle"] = json!(h.hex());
    m["uid"] = json!(s.state()?.uid);
    m["revision"] = json!(s.state()?.revision);
    Ok(m)
}
fn lines(d: &cadcraft_doc::Drawing, entities: impl Iterator<Item = impl AsRef<Entity>>) -> Result<Vec<EntityKind>> {
    let entities: Vec<_> = entities.map(|e| e.as_ref().clone()).collect();
    let display = cadcraft_render::build_entities(d, entities.iter(), &Default::default());
    let mut out = Vec::new();
    for p in &display.prims {
        let start = p.start as usize;
        let end = start.checked_add(p.len as usize).ok_or_else(|| bad(CMD, "Geometria troppo complessa."))?;
        if p.kind == cadcraft_render::Kind::Polyline {
            if let Some(points) = display.verts.get(start..end) {
                for q in points.windows(2) {
                    out.push(line(q[0], q[1]));
                }
            }
        } else if p.kind == cadcraft_render::Kind::Tris
            && let Some(points) = display.tris.get(start..end)
        {
            for q in points.as_chunks::<3>().0 {
                out.extend([line(q[0], q[1]), line(q[1], q[2]), line(q[2], q[0])]);
            }
        }
        if out.len() > 20000 {
            return Err(bad(CMD, "Simbolo troppo complesso."));
        }
    }
    if out.iter().any(|k| matches!(k, EntityKind::Line(l) if !l.a.xy().is_finite() || !l.b.xy().is_finite() || l.a.x.abs()>1000.0 || l.a.y.abs()>1000.0 || l.b.x.abs()>1000.0 || l.b.y.abs()>1000.0)) {return Err(bad(CMD,"Geometria del simbolo non valida."));}
    if out.is_empty() {
        return Err(bad(CMD, "Simbolo senza geometria."));
    }
    Ok(out)
}
pub(super) fn prepare(s: &mut Session, p: &Value) -> Result<Value> {
    if s.running.is_some() {
        return Err(bad(CMD, "Termina il comando corrente."));
    }
    let (layout, symbols, library) = schema::load(p["model"].as_str().ok_or_else(|| bad(CMD, "Indica il modello."))?)?;
    let layout = sheet(s, &layout)?;
    let target = if p.get("handle").is_some() {
        let h = handle(s, p)?;
        let actual = metadata(s, h)?;
        let same_original = p.get("original").is_some_and(|old| {
            let mut old = old.clone();
            if let Some(object) = old.as_object_mut() {
                for key in ["handle", "uid", "revision"] {
                    object.remove(key);
                }
            }
            old == actual
        });
        if p["uid"].as_u64() != Some(s.state()?.uid) || (p["revision"].as_u64() != Some(s.state()?.revision) && !same_original) {
            return Err(bad(CMD, "Il disegno è cambiato. Riapri la scheda."));
        }
        Some(h)
    } else {
        None
    };
    let id = p["id"].as_str().ok_or_else(|| bad(CMD, "Scegli un simbolo."))?;
    let symbol = symbols.into_iter().find(|v| v.id == id).ok_or_else(|| bad(CMD, "Simbolo non presente nel catalogo."))?;
    if let Some(h) = target
        && metadata(s, h)?["kind"] != symbol.kind
    {
        return Err(bad(CMD, "Scegli un simbolo dello stesso gruppo: apparecchio o marcatore di linea."));
    }
    let mut fields: BTreeMap<String, String> =
        serde_json::from_value(p.get("fields").cloned().unwrap_or_else(|| json!({}))).map_err(|e| bad(CMD, e.to_string()))?;
    let mut legacy_fields = BTreeMap::new();
    if let Some(h) = target {
        let old = metadata(s, h)?;
        for source in ["legacy_fields", "fields"] {
            if let Some(values) = old[source].as_object() {
                for (key, value) in values {
                    if !symbol.fields.contains(key)
                        && let Some(value) = value.as_str()
                    {
                        legacy_fields.insert(key.clone(), value.to_string());
                    }
                }
            }
        }
        fields.retain(|k, v| symbol.fields.contains(k) || legacy_fields.get(k) != Some(v));
    }
    for (key, value) in &symbol.defaults {
        fields.entry(key.clone()).or_insert_with(|| value.clone());
    }
    if fields.len() > 32 || fields.iter().any(|(k, v)| !symbol.fields.contains(k) || v.len() > 1024 || v.chars().any(|c| c.is_control() && c != '\n'))
    {
        return Err(bad(CMD, "Campi del dispositivo non validi."));
    }
    if json!([fields, legacy_fields]).to_string().len() > 24000 {
        return Err(bad(CMD, "Troppi dati nel dispositivo. Abbrevia i testi."));
    }
    let source = read_drawing(&library)?;
    let block = source.block(&symbol.block).ok_or_else(|| bad(CMD, "Simbolo assente dalla libreria."))?;
    let geometry = lines(&source, block.entities.iter())?;
    let backup_dir = p.get("backup_dir").and_then(Value::as_str).map(str::to_string);
    if backup_dir.as_ref().is_some_and(|v| v.len() > 32768) {
        return Err(bad(CMD, "Percorso backup troppo lungo."));
    }
    let draft = Draft {
        uid: s.state()?.uid,
        baseline: s.state()?.doc.clone(),
        layout,
        symbol,
        geometry,
        fields,
        legacy_fields,
        target,
        backup_dir,
        error: None,
    };
    // Validate every populated cell before leaving the form, so a click cannot reveal a text overflow.
    for col in draft.layout.columns.iter().filter(|_| draft.symbol.data_position == "table") {
        for (key, value) in &draft.fields {
            if let Some(cell) = col.cells.get(key) {
                let _ = cell_lines(value, cell)?;
            }
        }
    }
    s.unifilar_draft = Some(draft);
    Ok(json!({"prepared":true}))
}
fn current(s: &Session) -> Result<&Draft> {
    let d = s.unifilar_draft.as_ref().ok_or_else(|| bad(CMD, "Scegli un simbolo dal catalogo unifilare."))?;
    if s.space() != Space::Model || s.state()?.uid != d.uid || !Arc::ptr_eq(&s.state()?.doc, &d.baseline) {
        return Err(bad(CMD, "Il disegno è cambiato. Riapri la scheda del simbolo."));
    }
    Ok(d)
}
pub fn cell_lines(text: &str, c: &Cell) -> Result<Vec<String>> {
    let width = c.bounds[2] - c.bounds[0] - 2.0 * c.padding_mm;
    let mut lines = Vec::new();
    if text.trim().is_empty() {
        return Ok(lines);
    }
    for paragraph in text.trim().split('\n') {
        let mut row = String::new();
        for word in paragraph.split_whitespace() {
            if cadcraft_fonts::line_width(word, 2.1, 1.0) > width {
                return Err(bad(CMD, "Una parola o un codice supera la larghezza della cella. Abbrevialo."));
            }
            let next = if row.is_empty() { word.into() } else { format!("{row} {word}") };
            if cadcraft_fonts::line_width(&next, 2.1, 1.0) > width {
                lines.push(std::mem::take(&mut row));
                row = word.into();
            } else {
                row = next;
            }
        }
        lines.push(row);
    }
    if lines.len() > c.max_lines || lines.len() as f64 * 3.0 > c.bounds[3] - c.bounds[1] - 1.0 {
        return Err(bad(CMD, format!("Il testo supera le {} righe disponibili nella cella. Abbrevialo.", c.max_lines)));
    }
    Ok(lines)
}
pub(super) fn text(at: Vec2, value: String, height: f64) -> EntityKind {
    EntityKind::Text(Text {
        insert: Vec3::new(at.x, at.y, 0.0),
        align_pt: None,
        height,
        value,
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::Left,
        valign: VAlign::Baseline,
    })
}
struct Candidate {
    at: Vec2,
    column: Option<usize>,
    geometry: Vec<EntityKind>,
    label: Option<EntityKind>,
    cells: Vec<EntityKind>,
    line: Option<Handle>,
    attachment: Option<Value>,
}
fn column(d: &Draft, p: Vec2) -> Result<&Column> {
    d.layout.columns.iter().find(|c| p.x >= c.bounds[0] && p.x < c.bounds[2]).ok_or_else(|| bad(CMD, "Scegli una colonna all'interno della tavola."))
}
fn overlaps(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] < b[2] && a[2] > b[0] && a[1] < b[3] && a[3] > b[1]
}
fn segment_rect(a: Vec2, b: Vec2, r: [f64; 4]) -> bool {
    overlaps([a.x.min(b.x) - 0.1, a.y.min(b.y) - 0.1, a.x.max(b.x) + 0.1, a.y.max(b.y) + 0.1], r)
}
fn candidate(s: &Session, d: &Draft, p: Vec2) -> Result<Candidate> {
    if !p.is_finite() {
        return Err(bad(CMD, "Punto non valido."));
    }
    if d.symbol.kind == "line_marker" {
        return marker(s, d, p);
    }
    let col = column(d, p)?;
    let mut axis = col.axis_x + d.symbol.axis_offset_mm;
    for e in s.doc()?.model.iter() {
        if let Some(m) = data(e, APP)?
            && m["kind"] == "device"
            && m["column"].as_u64() == Some(col.index as u64)
            && let Some(x) = m["at"][0].as_f64().filter(|x| x.is_finite())
        {
            axis = x;
            break;
        }
    }
    let at = Vec2::new(axis, p.y);
    let mut geometry = d.geometry.clone();
    for k in &mut geometry {
        k.transform(&Mat3::translate(at));
    }
    let b = cadcraft_geom::Bounds2::from_points(
        geometry.iter().flat_map(|k| if let EntityKind::Line(l) = k { vec![l.a.xy(), l.b.xy()] } else { vec![] }),
    );
    if b.max.y > col.symbol_y_range[1]
        || at.y > col.symbol_y_range[1]
        || b.min.y < col.symbol_y_range[0]
        || b.min.x < col.bounds[0] + 1.0
        || b.max.x > col.bounds[2] - 1.0
    {
        return Err(bad(CMD, "Il simbolo esce dalla colonna o invade la fascia PE. Scegli un'altra altezza."));
    }
    let obstacles = super::annotations::obstacles(s.doc()?, d.target, d.layout.drawing_area[1])?;
    if obstacles.iter().any(|r| overlaps([b.min.x - 0.7, b.min.y - 0.7, b.max.x + 0.7, b.max.y + 0.7], *r)) {
        return Err(bad(CMD, "Il simbolo si sovrappone a un altro dispositivo o ai suoi dati. Scegli un'altra altezza."));
    }
    if d.symbol.data_position != "table" {
        let cells = super::annotations::local(&d.symbol, &d.fields, col, at, b, &obstacles, &d.layout)?;
        return Ok(Candidate { at, column: Some(col.index), geometry, label: None, cells, line: None, attachment: None });
    }
    let sigla = d.fields.get("sigla").map(|v| v.trim()).unwrap_or("");
    let mut label = None;
    if !sigla.is_empty() {
        let width = cadcraft_fonts::line_width(sigla, 2.3, 1.0);
        let mut found = None;
        for y in [at.y - 3.0, at.y - 8.0, at.y - 13.0] {
            for x in [at.x + 3.0, at.x - 3.0 - width] {
                let rect = [x - 0.5, y - 0.5, x + width + 0.5, y + 2.8];
                if rect[0] < col.bounds[0] + 1.0
                    || rect[2] > col.bounds[2] - 1.0
                    || rect[1] < col.symbol_y_range[0]
                    || rect[3] > col.symbol_y_range[1]
                {
                    continue;
                }
                if geometry.iter().any(|k| matches!(k,EntityKind::Line(l) if segment_rect(l.a.xy(),l.b.xy(),rect))) {
                    continue;
                }
                if obstacles.iter().any(|r| overlaps(rect, *r)) {
                    continue;
                }
                // Direct draft lines/text must not be hidden by the label. Native devices occupy other columns.
                if s.doc()?
                    .model
                    .iter()
                    .filter(|e| {
                        e.common.visible
                            && e.common.layer != "UNI_GUIDE"
                            && s.doc().is_ok_and(|d| d.layer(&e.common.layer).is_none_or(|l| l.on && !l.frozen))
                    })
                    .any(|e| match &e.kind {
                        EntityKind::Line(l) => segment_rect(l.a.xy(), l.b.xy(), rect),
                        EntityKind::Text(t) => overlaps(
                            [
                                t.insert.x,
                                t.insert.y - 0.5,
                                t.insert.x + cadcraft_fonts::line_width(&t.value, t.height, t.width_factor),
                                t.insert.y + t.height,
                            ],
                            rect,
                        ),
                        _ => false,
                    })
                {
                    continue;
                }
                found = Some(text(Vec2::new(x, y), sigla.into(), 2.3));
                break;
            }
            if found.is_some() {
                break;
            }
        }
        label = Some(found.ok_or_else(|| bad(CMD, "Non c'è spazio libero per la sigla. Abbreviala o cambia altezza."))?);
    }
    let cells = table_cells(s, d, col)?;
    Ok(Candidate { at, column: Some(col.index), geometry, label, cells, line: None, attachment: None })
}
fn table_cells(s: &Session, d: &Draft, col: &Column) -> Result<Vec<EntityKind>> {
    for e in s.doc()?.model.iter().filter(|e| Some(e.handle) != d.target) {
        if let Some(m) = data(e, APP)?
            && m["column"].as_u64() == Some(col.index as u64)
            && m["data_position"].as_str().unwrap_or("table") == "table"
        {
            for (key, value) in &d.fields {
                if !value.trim().is_empty() && m["fields"][key].as_str().is_some_and(|v| !v.trim().is_empty()) {
                    return Err(bad(
                        CMD,
                        format!(
                            "La casella {key} della colonna {} è già compilata da un altro simbolo. Modifica quel dato prima di assegnarlo qui.",
                            col.index
                        ),
                    ));
                }
            }
        }
    }
    let mut cells = Vec::new();
    for (key, value) in &d.fields {
        let cell = col.cells.get(key).ok_or_else(|| bad(CMD, "Cella non disponibile."))?;
        let rows = cell_lines(value, cell)?;
        let y = cell.text_center[1] + (rows.len().saturating_sub(1)) as f64 * 1.5 - 0.85;
        for (i, value) in rows.into_iter().enumerate() {
            let width = cadcraft_fonts::line_width(&value, 2.1, 1.0);
            cells.push(text(Vec2::new(cell.text_center[0] - width / 2.0, y - i as f64 * 3.0), value, 2.1));
        }
    }
    Ok(cells)
}
fn marker(s: &Session, d: &Draft, p: Vec2) -> Result<Candidate> {
    let mut best = None;
    let mut distance = 2.0;
    let segments = super::markers::segments(s.doc()?)?;
    for (index, segment) in segments.iter().enumerate() {
        let a = segment.a;
        let b = segment.b;
        let v = b - a;
        let len = v.len();
        if !len.is_finite() || len < 1e-6 {
            continue;
        }
        let dir = v / len;
        let t = ((p - a).dot(dir)).clamp(0.0, len);
        let at = a + dir * t;
        let dist = p.dist(at);
        if dist < distance
            && at.x > d.layout.drawing_area[0]
            && at.x < d.layout.drawing_area[2]
            && at.y > d.layout.columns.first().map(|c| c.symbol_y_range[0]).unwrap_or(d.layout.drawing_area[1])
            && at.y < d.layout.drawing_area[3]
        {
            distance = dist;
            best = Some((segment.owner, at, dir, a, b, index));
        }
    }
    let (handle, at, mut dir, a, b, index) = best.ok_or_else(|| bad(CMD, "Indica una linea di collegamento nell'area simboli (entro 2 mm)."))?;
    if dir.y < 0.0 || (dir.y.abs() < 1e-8 && dir.x < 0.0) {
        dir = -dir;
    }
    let angle = dir.y.atan2(dir.x) - std::f64::consts::FRAC_PI_2;
    let mut geometry = d.geometry.clone();
    for k in &mut geometry {
        k.transform(&Mat3::rotate(angle));
        k.transform(&Mat3::translate(at));
    }
    let min = a.dot(dir).min(b.dot(dir));
    let max = a.dot(dir).max(b.dot(dir));
    for k in &geometry {
        if let EntityKind::Line(l) = k {
            for q in [l.a.xy(), l.b.xy()] {
                let projection = q.dot(dir);
                if projection < min - 0.2 || projection > max + 0.2 || q.y < d.layout.drawing_area[1] + 18.0 {
                    return Err(bad(CMD, "Il marcatore non entra nel tratto di linea disponibile."));
                }
            }
        }
    }
    let col = if d.fields.values().any(|v| !v.trim().is_empty()) { Some(column(d, at)?) } else { None };
    let cells = col.map(|c| table_cells(s, d, c)).transpose()?.unwrap_or_default();
    Ok(Candidate {
        at,
        column: col.map(|c| c.index),
        geometry,
        label: None,
        cells,
        line: Some(handle),
        attachment: super::markers::attachment(&segments, index, at, angle),
    })
}
fn commit(s: &mut Session, p: Vec2) -> Result<Value> {
    let d = current(s)?.clone();
    let c = candidate(s, &d, p)?;
    let mut drawing = s.doc()?.clone();
    for layer in ["UNI_APPARECCHI", "UNI_TESTI"] {
        if drawing.layer(layer).is_some_and(|l| l.locked || l.frozen || !l.on) {
            return Err(bad(CMD, format!("Attiva e sblocca {layer}.")));
        }
        drawing.ensure_layer(layer);
    }
    if let Some(h) = d.target {
        let e = drawing.entity(h).ok_or_else(|| bad(CMD, "Oggetto assente."))?;
        if drawing.layer(&e.common.layer).is_some_and(|l| l.locked || !l.on || l.frozen) {
            return Err(bad(CMD, "Il layer dell'oggetto è bloccato o nascosto."));
        }
    }
    let name = (1..=100000).map(|i| format!("CC_UNIFILARE_{i}")).find(|n| drawing.block(n).is_none()).ok_or_else(|| bad(CMD, "Troppi blocchi."))?;
    let mut block = Block::new(&name);
    for (mut k, layer) in c.geometry.into_iter().map(|k| (k, "0")).chain(c.label.into_iter().chain(c.cells).map(|k| (k, "UNI_TESTI"))) {
        k.transform(&Mat3::translate(-c.at));
        let mut e = Entity::new(drawing.new_handle(), k);
        e.common.layer = layer.into();
        block.entities.push(e);
    }
    drawing.blocks.insert(name.clone(), Arc::new(block));
    let mut common = d
        .target
        .and_then(|h| drawing.entity(h))
        .map(|e| e.common.clone())
        .unwrap_or_else(|| Common { layer: "UNI_APPARECCHI".into(), ..Default::default() });
    tags(
        &mut common,
        APP,
        &json!({"version":1,"model":d.layout.id,"symbol":d.symbol.id,"name":d.symbol.name,"kind":d.symbol.kind,"column":c.column,"fields":d.fields,"legacy_fields":d.legacy_fields,"data_position":d.symbol.data_position,"at":[c.at.x,c.at.y],"ports":d.symbol.ports,"line":c.line.map(|h|h.hex()),"attachment":c.attachment}),
    );
    let kind = EntityKind::Insert(Insert {
        block: name,
        insert: Vec3::new(c.at.x, c.at.y, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: vec![],
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    });
    let h = if let Some(h) = d.target {
        drawing.modify_entity(h, |e| {
            e.kind = kind;
            e.common = common;
        })?;
        h
    } else {
        drawing.add(&Space::Model, common, kind)?
    };
    super::connections::refresh(&mut drawing, &d.layout)?;
    let backup = backup(s.doc()?, d.backup_dir.as_deref())?;
    *s.doc_mut()? = drawing;
    s.unifilar_draft = None;
    Ok(json!({"handle":h.hex(),"column":c.column,"backup":backup}))
}
pub(super) fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    prepare(s, p)?;
    let at: [f64; 2] = serde_json::from_value(p["at"].clone()).map_err(|e| bad(CMD, e.to_string()))?;
    commit(s, Vec2::new(at[0], at[1]))
}
pub(super) fn update(s: &mut Session, p: &Value) -> Result<Value> {
    let h = handle(s, p)?;
    let old = metadata(s, h)?;
    let mut params = p.clone();
    params["handle"] = json!(h.hex());
    if params.get("id").is_none() {
        params["id"] = old["symbol"].clone();
    }
    if params.get("fields").is_none() {
        params["fields"] = old["fields"].clone();
    }
    let at = params.get("at").cloned().unwrap_or_else(|| old["at"].clone());
    params["at"] = at;
    insert(s, &params)
}
pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("electrical.unifilar.catalog", "Read unifilar catalog", schema::catalog).noundo(),
        CommandSpec::new("electrical.unifilar.read", "Read unifilar device", read_object).noundo(),
        CommandSpec::new("electrical.unifilar.prepare", "Prepare unifilar device", prepare).noundo(),
        CommandSpec::new("electrical.unifilar.insert", "Insert unifilar device", insert),
        CommandSpec::new("electrical.unifilar.update", "Update unifilar device", update),
        CommandSpec::new("electrical.unifilar.place", "Place unifilar device", |_, _| Err(bad(CMD, "Prepara un simbolo dal catalogo.")))
            .enabled(|s| current(s).map(|_| ()).map_err(|e| e.to_string()))
            .interactive(|s| {
                current(s)?;
                Ok(Box::new(Place))
            }),
    ]
}
struct Place;
impl Interactive for Place {
    fn name(&self) -> &'static str {
        "electrical.unifilar.place"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let error = s.unifilar_draft.as_ref().and_then(|d| d.error.as_deref());
        Prompt::new(error.unwrap_or("Scegli la colonna e l'altezza del simbolo; Esc annulla"), Accept::POINT)
    }
    fn input(&mut self, s: &mut Session, input: Input) -> Result<Step> {
        match input {
            Input::Point(p) => match commit(s, p) {
                Ok(v) => {
                    s.echo(format!("Inserito. Backup: {}", v["backup"].as_str().unwrap_or("")));
                    Ok(Step::Done)
                }
                Err(e) => {
                    if let Some(d) = &mut s.unifilar_draft {
                        d.error = Some(match &e {
                            EngineError::BadParams { msg, .. } => msg.clone(),
                            _ => e.to_string(),
                        });
                    }
                    Err(e)
                }
            },
            Input::Cancel => {
                s.unifilar_draft = None;
                Ok(Step::Cancel)
            }
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, cursor: Vec2) -> Vec<EntityKind> {
        let Ok(d) = current(s) else { return vec![] };
        let Ok(c) = candidate(s, d, cursor) else { return vec![] };
        let mut all = c.geometry;
        all.extend(c.label);
        all.extend(c.cells);
        if let Some(i) = c.column.and_then(|i| d.layout.columns.iter().find(|c| c.index == i)) {
            all.extend([
                line(Vec2::new(i.bounds[0], i.symbol_y_range[0]), Vec2::new(i.bounds[0], i.symbol_y_range[1])),
                line(Vec2::new(i.bounds[2], i.symbol_y_range[0]), Vec2::new(i.bounds[2], i.symbol_y_range[1])),
                line(Vec2::new(i.axis_x, i.symbol_y_range[0]), Vec2::new(i.axis_x, i.symbol_y_range[1])),
            ]);
        }
        all
    }
}
