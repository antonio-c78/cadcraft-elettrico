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
    let command = sym.category == "Comando" && sym.data_position == "right";
    let height = if command { 1.5 } else { 2.1 };
    for key in &sym.fields {
        if command && key == "bobina_alimentazione" {
            continue;
        }
        let Some(value) = fields.get(key).filter(|v| !v.trim().is_empty()) else {
            continue;
        };
        // The catalog's redundant type default need not consume the annotation column.
        // Keep the value in metadata; explicit model/type variants are still displayed.
        if !command && key == "tipo" && value.trim().eq_ignore_ascii_case(sym.name.trim()) {
            continue;
        }
        if command {
            let value = match key.as_str() {
                "bobina" => format!("{} V {}", value.trim(), fields.get("bobina_alimentazione").map(String::as_str).unwrap_or("")),
                "poli" => format!("{}P", value.trim()),
                "in_apparecchio" => format!("In [A] {}", value.trim()),
                _ => value.trim().to_string(),
            };
            let coil = key == "bobina";
            let mut cell = cell.clone();
            if coil {
                cell.bounds[2] -= 2.4;
            }
            cell.bounds[2] *= 2.1 / height;
            for (i, line) in placement::cell_lines(&value, &cell)?.into_iter().enumerate() {
                rows.push((line, coil && i == 0, if coil { 2.4 } else { 0.0 }));
            }
            continue;
        }
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
        rows.extend(placement::cell_lines(&value, &cell)?.into_iter().map(|v| (v, false, 0.0)));
    }
    let top = if below { b.min.y - 4.0 } else { at.y - 2.5 };
    let floor = layout.pe["y"].as_f64().unwrap_or(col.symbol_y_range[0] - 10.) + 3.0;
    let mut result = Vec::new();
    for (i, (value, coil, indent)) in rows.into_iter().enumerate() {
        let w = cadcraft_fonts::line_width(&value, height, 1.0);
        let row_x = if below { (at.x - w * 0.5).clamp(x, (col.bounds[2] - 1.5 - w).max(x)) } else { x };
        let pos = Vec2::new(row_x + indent, top - i as f64 * (height + 0.9));
        if coil {
            // Original character-height coil pictogram: rectangle with two terminals.
            for (a, b) in [
                ([0., 1.05], [0.4, 1.05]),
                ([0.4, 0.], [0.4, 2.1]),
                ([0.4, 2.1], [2.2, 2.1]),
                ([2.2, 2.1], [2.2, 0.]),
                ([2.2, 0.], [0.4, 0.]),
                ([2.2, 1.05], [2.6, 1.05]),
            ] {
                let kind = EntityKind::Line(Line {
                    a: Vec3::new(row_x + a[0] * height / 2.1, pos.y + a[1] * height / 2.1, 0.),
                    b: Vec3::new(row_x + b[0] * height / 2.1, pos.y + b[1] * height / 2.1, 0.),
                });
                if let Some(r) = rect(&kind, Vec2::ZERO, floor - 1000.)
                    && (r[1] < floor || r[3] > col.symbol_y_range[1] || obstacles.iter().any(|o| overlap(r, *o)))
                {
                    return Err(bad(CMD, "La bobina incontra un collegamento o un simbolo. Cambia altezza."));
                }
                result.push(kind);
            }
        }
        let kind = placement::text(pos, value, height);
        if let Some(r) = rect(&kind, Vec2::ZERO, floor - 1000.)
            && (r[1] < floor || r[3] > col.symbol_y_range[1] || obstacles.iter().any(|o| overlap(r, *o)))
        {
            return Err(bad(CMD, "I dati incontrano un simbolo, un collegamento o la fascia PE. Abbrevia il testo o cambia altezza."));
        }
        result.push(kind);
    }
    Ok(result)
}
