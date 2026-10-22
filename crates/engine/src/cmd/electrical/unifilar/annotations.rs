//! Local device data and conservative collision checks, without a per-column device limit.
use super::super::*;
use super::{
    placement,
    schema::{Cell, Column, Layout, Symbol},
};
use cadcraft_doc::{Drawing, Handle};
use std::collections::BTreeMap;
const CMD: &str = "electrical.unifilar.prepare";
fn overlap(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] < b[2] && a[2] > b[0] && a[1] < b[3] && a[3] > b[1]
}
fn rect(k: &EntityKind, at: Vec2, floor: f64) -> Option<[f64; 4]> {
    match k {
        EntityKind::Line(l) if l.a.y + at.y >= floor || l.b.y + at.y >= floor => {
            Some([l.a.x.min(l.b.x) + at.x - 0.2, l.a.y.min(l.b.y) + at.y - 0.2, l.a.x.max(l.b.x) + at.x + 0.2, l.a.y.max(l.b.y) + at.y + 0.2])
        }
        EntityKind::Text(t) if t.insert.y + at.y >= floor => Some([
            t.insert.x + at.x - 0.4,
            t.insert.y + at.y - 0.4,
            t.insert.x + at.x + cadcraft_fonts::line_width(&t.value, t.height, t.width_factor) + 0.4,
            t.insert.y + at.y + t.height + 0.4,
        ]),
        _ => None,
    }
}
pub(super) fn obstacles(d: &Drawing, target: Option<Handle>, floor: f64) -> Result<Vec<[f64; 4]>> {
    let mut out = Vec::new();
    let mut scanned = 0usize;
    for e in d.model.iter().filter(|e| {
        Some(e.handle) != target && e.common.visible && e.common.layer != "UNI_GUIDE" && d.layer(&e.common.layer).is_none_or(|l| l.on && !l.frozen)
    }) {
        if let EntityKind::Insert(i) = &e.kind {
            if placement::data(e, "CC_UNIFILARE_DEVICE")?.is_none() {
                continue;
            }
            let b = d.block(&i.block).ok_or_else(|| bad(CMD, "Geometria del simbolo assente."))?;
            scanned = scanned.saturating_add(b.entities.len());
            if scanned > 200000 {
                return Err(bad(CMD, "Troppi elementi nella tavola."));
            }
            let mut body = cadcraft_geom::Bounds2::EMPTY;
            for p in b.entities.iter() {
                if p.common.layer != "UNI_TESTI"
                    && let EntityKind::Line(l) = &p.kind
                {
                    body = body.union(&cadcraft_geom::Bounds2::from_points([l.a.xy() + i.insert.xy(), l.b.xy() + i.insert.xy()]));
                }
                if let Some(r) = rect(&p.kind, i.insert.xy(), floor) {
                    out.push(r);
                }
            }
            if !body.is_empty() {
                out.push([body.min.x - 0.4, body.min.y - 0.4, body.max.x + 0.4, body.max.y + 0.4]);
            }
        } else if let Some(r) = rect(&e.kind, Vec2::ZERO, floor) {
            out.push(r);
        }
        if out.len() > 20000 {
            return Err(bad(CMD, "Troppi elementi nella tavola."));
        }
    }
    Ok(out)
}
pub(super) fn local(
    sym: &Symbol,
    fields: &BTreeMap<String, String>,
    col: &Column,
    at: Vec2,
    b: cadcraft_geom::Bounds2,
    obstacles: &[[f64; 4]],
    layout: &Layout,
) -> Result<Vec<EntityKind>> {
    let below = sym.data_position == "below";
    let x = if below { col.bounds[0] + 1.5 } else { b.max.x + 2.0 };
    let width = col.bounds[2] - 1.5 - x;
    if width < 5.0 {
        return Err(bad(CMD, "Non c'è spazio per i dati accanto al simbolo."));
    }
    let cell = Cell { bounds: [0., 0., width, 220.], text_center: [0., 0.], padding_mm: 0., max_lines: 64 };
    let mut rows = Vec::new();
    for key in &sym.fields {
        let Some(value) = fields.get(key).filter(|v| !v.trim().is_empty()) else {
            continue;
        };
        let prefix = match key.as_str() {
            "sigla" | "descrizione" | "modello" => "",
            "in_apparecchio" => "In",
            "moduli" => "DIN",
            "poli" => "Poli",
            "un" => "Un",
            "marca" => "Marca",
            "tipo" => "Tipo",
            "note" => "Note",
            _ => key.as_str(),
        };
        let value = if prefix.is_empty() { value.clone() } else { format!("{prefix}: {value}") };
        rows.extend(placement::cell_lines(&value, &cell)?);
    }
    let top = if below { b.min.y - 4.0 } else { at.y - 2.5 };
    let floor = layout.pe["y"].as_f64().unwrap_or(col.symbol_y_range[0] - 10.) + 3.0;
    let mut result = Vec::new();
    for (i, value) in rows.into_iter().enumerate() {
        let w = cadcraft_fonts::line_width(&value, 2.1, 1.0);
        let row_x = if below { (at.x - w * 0.5).clamp(x, (col.bounds[2] - 1.5 - w).max(x)) } else { x };
        let pos = Vec2::new(row_x, top - i as f64 * 3.0);
        let kind = placement::text(pos, value, 2.1);
        if let Some(r) = rect(&kind, Vec2::ZERO, floor - 1000.)
            && (r[1] < floor || r[3] > col.symbol_y_range[1] || obstacles.iter().any(|o| overlap(r, *o)))
        {
            return Err(bad(CMD, "I dati incontrano un simbolo, un collegamento o la fascia PE. Abbrevia il testo o cambia altezza."));
        }
        result.push(kind);
    }
    Ok(result)
}
