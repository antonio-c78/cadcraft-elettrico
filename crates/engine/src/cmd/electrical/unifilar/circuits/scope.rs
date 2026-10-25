//! Selection scope and rigid column mapping, independent of routing and commit.
use super::*;
use std::collections::BTreeMap;
pub(super) fn selected_columns(s: &Session, p: &Value) -> Result<BTreeSet<usize>> {
    let selected = if let Some(values) = p.get("handles") {
        let values = values.as_array().filter(|v| v.len() <= 512).ok_or_else(|| error("Selezione non valida o troppo grande."))?;
        values
            .iter()
            .map(|v| v.as_str().and_then(Handle::parse_hex).ok_or_else(|| error("Identificativo non valido.")))
            .collect::<Result<Vec<_>>>()?
    } else if let Some(value) = p.get("handle") {
        vec![value.as_str().and_then(Handle::parse_hex).ok_or_else(|| error("Identificativo non valido."))?]
    } else {
        s.selection()
    };
    if selected.len() > 512 {
        return Err(error("Selezione troppo grande."));
    }
    let mut columns = BTreeSet::new();
    for h in selected {
        let Some(e) = s.doc()?.entity(h) else {
            return Err(error("Oggetto selezionato assente."));
        };
        if let Some(m) = placement::data(e, APP)? {
            // Geometry-only markers follow their owner wire; they own no column.
            if m["kind"] == "line_marker" && m.get("column").is_none_or(Value::is_null) {
                continue;
            }
            let c = m["column"].as_u64().and_then(|v| usize::try_from(v).ok()).ok_or_else(|| error("Colonna non valida."))?;
            columns.insert(c);
        }
    }
    if columns.is_empty() {
        return Err(error("Seleziona almeno un apparecchio delle colonne da spostare."));
    }
    Ok(columns)
}
pub(in crate::cmd::electrical::unifilar) fn destinations(
    layout: &schema::Layout,
    columns: &BTreeSet<usize>,
    source: usize,
    target: usize,
) -> Result<BTreeMap<usize, (usize, f64)>> {
    let index = |c| layout.columns.iter().position(|v| v.index == c).ok_or_else(|| error("Colonna assente."));
    let start = index(source)?;
    let end = index(target)?;
    let base = layout.columns.get(start).ok_or_else(|| error("Colonna assente."))?;
    let dest = layout.columns.get(end).ok_or_else(|| error("Colonna assente."))?;
    let dx = dest.axis_x - base.axis_x;
    let mut out = BTreeMap::new();
    for c in columns {
        let i = index(*c)?;
        let next = if end >= start { i.checked_add(end - start) } else { i.checked_sub(start - end) };
        let from = layout.columns.get(i).ok_or_else(|| error("Colonna assente."))?;
        let to = next.and_then(|j| layout.columns.get(j)).ok_or_else(|| error("Il gruppo esce dalla tavola."))?;
        if (to.axis_x - from.axis_x - dx).abs() > 1e-6
            || (to.bounds[2] - to.bounds[0] - (from.bounds[2] - from.bounds[0])).abs() > 1e-6
            || from.cells.iter().any(|(k, v)| {
                to.cells
                    .get(k)
                    .is_none_or(|w| (w.text_center[0] - v.text_center[0] - dx).abs() > 1e-6 || (w.text_center[1] - v.text_center[1]).abs() > 1e-6)
            })
        {
            return Err(error("Le colonne hanno formati incompatibili."));
        }
        out.insert(*c, (to.index, dx));
    }
    Ok(out)
}
