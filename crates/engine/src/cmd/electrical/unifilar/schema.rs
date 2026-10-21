//! External sheet contracts, shared by placement and the native browser.
use super::super::*;
use std::collections::{BTreeMap, BTreeSet};
const CMD: &str = "electrical.unifilar.catalog";
#[derive(Clone, Deserialize, Serialize)]
pub struct Cell {
    pub bounds: [f64; 4],
    pub text_center: [f64; 2],
    pub padding_mm: f64,
    pub max_lines: usize,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Column {
    pub index: usize,
    pub bounds: [f64; 4],
    pub axis_x: f64,
    pub symbol_y_range: [f64; 2],
    pub cells: BTreeMap<String, Cell>,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Row {
    pub id: String,
    pub label: String,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Table {
    pub rows: Vec<Row>,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Layout {
    pub id: String,
    pub columns: Vec<Column>,
    pub table: Table,
    pub drawing_area: [f64; 4],
    pub pe: Value,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Symbol {
    pub id: String,
    pub name: String,
    pub block: String,
    pub category: String,
    pub kind: String,
    #[serde(default)]
    pub fields: Vec<String>,
    #[serde(default)]
    pub ports: Vec<Value>,
    #[serde(default)]
    pub defaults: BTreeMap<String, String>,
}
#[derive(Deserialize)]
struct Catalog {
    format: String,
    version: u32,
    library: String,
    symbols: Vec<Symbol>,
}
fn local(parent: &std::path::Path, name: &str) -> Result<std::path::PathBuf> {
    let p = std::path::Path::new(name);
    if p.components().count() != 1 || !matches!(p.components().next(), Some(std::path::Component::Normal(_))) {
        return Err(bad(CMD, "Il file deve essere nella cartella del modello."));
    }
    Ok(parent.join(p))
}
pub(super) fn load(path: &str) -> Result<(Layout, Vec<Symbol>, String)> {
    if path.len() > 32768 {
        return Err(bad(CMD, "Percorso troppo lungo."));
    }
    let path = std::path::Path::new(path);
    let raw: Value = serde_json::from_slice(&super::read(path, 524288)?).map_err(|e| bad(CMD, e.to_string()))?;
    if raw["format"] != "cadcraft-unifilar-template" || raw["version"] != 1 || raw["units"] != "mm" {
        return Err(bad(CMD, "Modello non supportato."));
    }
    let layout: Layout = serde_json::from_value(raw.clone()).map_err(|e| bad(CMD, e.to_string()))?;
    validate(&layout)?;
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let catalog_path = local(parent, raw["catalog"].as_str().ok_or_else(|| bad(CMD, "Catalogo non configurato."))?)?;
    let catalog: Catalog = serde_json::from_slice(&super::read(&catalog_path, 2 * 1024 * 1024)?).map_err(|e| bad(CMD, e.to_string()))?;
    if catalog.format != "cadcraft-unifilar-symbols" || catalog.version != 1 || catalog.symbols.is_empty() || catalog.symbols.len() > 512 {
        return Err(bad(CMD, "Catalogo non supportato."));
    }
    let library = local(parent, &catalog.library)?.to_string_lossy().into_owned();
    let mut ids = BTreeSet::new();
    for sym in &catalog.symbols {
        if [&sym.id, &sym.name, &sym.block, &sym.category].iter().any(|s| s.is_empty() || s.len() > 255)
            || !ids.insert(&sym.id)
            || !["device", "line_marker"].contains(&sym.kind.as_str())
            || sym.defaults.len() > 32
            || sym.defaults.iter().any(|(k, v)| !sym.fields.contains(k) || v.len() > 256 || v.chars().any(char::is_control))
            || sym.fields.len() > 32
            || sym.ports.len() > 16
            || serde_json::to_string(&sym.ports).map_or(true, |s| s.len() > 4096)
            || sym.fields.iter().any(|f| !layout.table.rows.iter().any(|r| &r.id == f))
        {
            return Err(bad(CMD, "Attributi del catalogo non validi."));
        }
    }
    Ok((layout, catalog.symbols, library))
}
pub(super) fn validate(l: &Layout) -> Result<()> {
    let finite = |a: &[f64]| a.iter().all(|v| v.is_finite() && v.abs() <= 1000.0);
    if l.id.is_empty()
        || l.id.len() > 128
        || l.columns.is_empty()
        || l.columns.len() > 32
        || l.table.rows.is_empty()
        || l.table.rows.len() > 32
        || !finite(&l.drawing_area)
        || l.drawing_area[0] >= l.drawing_area[2]
        || l.drawing_area[1] >= l.drawing_area[3]
    {
        return Err(bad(CMD, "Geometria del modello non valida."));
    }
    let mut rowids = BTreeSet::new();
    for r in &l.table.rows {
        if r.id.is_empty() || r.id.len() > 64 || r.label.len() > 100 || !rowids.insert(&r.id) {
            return Err(bad(CMD, "Righe non valide."));
        }
    }
    let mut indices = BTreeSet::new();
    let mut right = f64::NEG_INFINITY;
    for c in &l.columns {
        if !indices.insert(c.index)
            || !finite(&c.bounds)
            || c.bounds[0] < right
            || c.bounds[2] - c.bounds[0] < 15.0
            || c.bounds[2] > l.drawing_area[2]
            || c.bounds[0] < l.drawing_area[0]
            || (c.axis_x - (c.bounds[0] + c.bounds[2]) / 2.0).abs() > 1e-6
            || !c.axis_x.is_finite()
            || c.axis_x <= c.bounds[0]
            || c.axis_x >= c.bounds[2]
            || !finite(&c.symbol_y_range)
            || c.symbol_y_range[0] >= c.symbol_y_range[1]
            || c.symbol_y_range[0] < l.drawing_area[1]
            || c.symbol_y_range[1] > l.drawing_area[3]
            || c.cells.len() != l.table.rows.len()
        {
            return Err(bad(CMD, "Colonne non valide."));
        }
        right = c.bounds[2];
        for r in &l.table.rows {
            let cell = c.cells.get(&r.id).ok_or_else(|| bad(CMD, "Cella mancante."))?;
            if !finite(&cell.bounds)
                || !finite(&cell.text_center)
                || cell.bounds[0] < c.bounds[0]
                || cell.bounds[2] > c.bounds[2]
                || cell.bounds[2] <= cell.bounds[0]
                || cell.bounds[3] - cell.bounds[1] < 3.0
                || cell.text_center[0] < cell.bounds[0]
                || cell.text_center[0] > cell.bounds[2]
                || cell.text_center[1] < cell.bounds[1]
                || cell.text_center[1] > cell.bounds[3]
                || !cell.padding_mm.is_finite()
                || !(0.5..=3.0).contains(&cell.padding_mm)
                || !(1..=3).contains(&cell.max_lines)
            {
                return Err(bad(CMD, "Celle non valide."));
            }
        }
    }
    Ok(())
}
pub(super) fn catalog(s: &mut Session, p: &Value) -> Result<Value> {
    let path = p["model"].as_str().ok_or_else(|| bad(CMD, "Indica il modello."))?;
    let (layout, symbols, library) = load(path)?;
    let sheet = super::placement::sheet(s, &layout)?;
    Ok(json!({"layout":sheet,"symbols":symbols,"library":library}))
}

/// Compact snapshot fits within DXF XDATA limits and freezes the row geometry for this sheet.
pub(super) fn snapshot(l: &Layout) -> Result<Value> {
    let first = l.columns.first().ok_or_else(|| bad(CMD, "Colonne mancanti."))?;
    let rows:Vec<_>=l.table.rows.iter().map(|r|{
  let c=first.cells.get(&r.id).ok_or_else(||bad(CMD,"Cella mancante."))?;
  if l.columns.iter().any(|col|col.cells.get(&r.id).is_none_or(|v|v.bounds[1]!=c.bounds[1]||v.bounds[3]!=c.bounds[3]||v.text_center[1]!=c.text_center[1]||v.padding_mm!=c.padding_mm||v.max_lines!=c.max_lines)){return Err(bad(CMD,"Le righe devono essere allineate in tutte le colonne."));}
  Ok(json!({"id":r.id,"label":r.label,"y0":c.bounds[1],"y1":c.bounds[3],"center_y":c.text_center[1],"padding_mm":c.padding_mm,"max_lines":c.max_lines}))
 }).collect::<Result<_>>()?;
    let columns: Vec<_> =
        l.columns.iter().map(|c| json!({"index":c.index,"bounds":c.bounds,"axis_x":c.axis_x,"symbol_y_range":c.symbol_y_range})).collect();
    let raw = json!({"snapshot_version":1,"id":l.id,"columns":columns,"rows":rows,"drawing_area":l.drawing_area,"pe":l.pe});
    if raw.to_string().len() > 12000 {
        return Err(bad(CMD, "Definizione della tavola troppo grande."));
    }
    Ok(raw)
}
pub(super) fn restore(mut raw: Value) -> Result<Layout> {
    if raw["snapshot_version"] != 1 {
        return Err(bad(CMD, "Versione geometria tavola non supportata."));
    }
    let rows = raw["rows"].as_array().filter(|r| r.len() <= 32).cloned().ok_or_else(|| bad(CMD, "Righe non valide."))?;
    let columns = raw["columns"].as_array_mut().filter(|c| c.len() <= 32).ok_or_else(|| bad(CMD, "Colonne non valide."))?;
    for col in columns {
        let x0 = col["bounds"].get(0).and_then(Value::as_f64).ok_or_else(|| bad(CMD, "Colonna non valida."))?;
        let x1 = col["bounds"].get(2).and_then(Value::as_f64).ok_or_else(|| bad(CMD, "Colonna non valida."))?;
        let mut cells = serde_json::Map::new();
        for row in &rows {
            let id = row["id"].as_str().ok_or_else(|| bad(CMD, "Riga non valida."))?;
            cells.insert(id.into(),json!({"bounds":[x0,row["y0"],x1,row["y1"]],"text_center":[(x0+x1)/2.0,row["center_y"]],"padding_mm":row["padding_mm"],"max_lines":row["max_lines"]}));
        }
        col["cells"] = Value::Object(cells);
    }
    raw["table"] = json!({"rows":rows.iter().map(|r|json!({"id":r["id"],"label":r["label"]})).collect::<Vec<_>>()});
    let layout: Layout = serde_json::from_value(raw).map_err(|e| bad(CMD, e.to_string()))?;
    validate(&layout)?;
    Ok(layout)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_sheet_snapshot_roundtrips_and_rejects_misaligned_rows() {
        let raw = json!({"id":"synthetic","columns":[{"index":1,"bounds":[10,20,45,220],"axis_x":27.5,"symbol_y_range":[100,200],"cells":{"sigla":{"bounds":[10,20,45,25],"text_center":[27.5,22.5],"padding_mm":1.5,"max_lines":1}}}],"table":{"rows":[{"id":"sigla","label":"Sigla"}]},"drawing_area":[0,80,100,220],"pe":{"y":90}});
        let layout: Layout = serde_json::from_value(raw).unwrap();
        validate(&layout).unwrap();
        let encoded = snapshot(&layout).unwrap();
        assert!(encoded.to_string().len() < 16000);
        let copy = restore(encoded).unwrap();
        assert_eq!(serde_json::to_value(&layout).unwrap(), serde_json::to_value(copy).unwrap());
        let mut invalid = layout.clone();
        invalid.columns[0].axis_x = 44.0;
        assert!(validate(&invalid).is_err());
    }
}
