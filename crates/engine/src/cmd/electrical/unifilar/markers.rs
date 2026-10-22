//! Markers retain an arc-length attachment to a route or to the horizontal bus span.
use super::super::*;
use super::{connections, placement, schema};
use cadcraft_doc::{Drawing, Handle};
pub(super) struct Segment {
    pub owner: Handle,
    pub a: Vec2,
    pub b: Vec2,
    pub part: &'static str,
}
pub(super) fn segments(d: &Drawing) -> Result<Vec<Segment>> {
    let mut out = Vec::new();
    for e in d.model.iter() {
        if let EntityKind::Line(l) = &e.kind {
            out.push(Segment { owner: e.handle, a: l.a.xy(), b: l.b.xy(), part: "plain" });
        } else if e.common.xdata.iter().any(|t| t.code == 1001 && t.value == "CC_UNIFILARE_LINK") {
            let l = connections::read_link(d, e.handle)?;
            let EntityKind::Insert(i) = &e.kind else {
                continue;
            };
            let block = d.block(&i.block).ok_or_else(|| bad("electrical.unifilar.link", "Geometria del collegamento assente."))?;
            if block.entities.len() > 8192 {
                return Err(bad("electrical.unifilar.link", "Troppi tratti di collegamento."));
            }
            for child in block.entities.iter() {
                if let EntityKind::Line(line) = &child.kind {
                    let is_bar = l.kind == "bus"
                        && l.span.is_some_and(|span| {
                            (line.a.y - l.y).abs() < 1e-7
                                && (line.b.y - l.y).abs() < 1e-7
                                && (line.a.x.min(line.b.x) - span[0]).abs() < 1e-7
                                && (line.a.x.max(line.b.x) - span[1]).abs() < 1e-7
                        });
                    out.push(Segment { owner: e.handle, a: line.a.xy(), b: line.b.xy(), part: if is_bar { "bar" } else { "route" } });
                }
            }
        }
        if out.len() > 16000 {
            return Err(bad("electrical.unifilar.link", "Troppi tratti di collegamento."));
        }
    }
    Ok(out)
}
pub(super) fn attachment(lines: &[Segment], index: usize, at: Vec2, angle: f64) -> Option<Value> {
    let chosen = lines.get(index)?;
    if chosen.part == "plain" {
        return None;
    }
    let mut before = 0.0;
    let mut total = 0.0;
    let mut found = false;
    for (i, line) in lines.iter().enumerate().filter(|(_, v)| v.owner == chosen.owner && v.part == chosen.part) {
        let len = line.a.dist(line.b);
        if i == index {
            before = total + line.a.dist(at);
            found = true;
        }
        total += len;
    }
    (found && total > 1e-8).then(|| json!({"part":chosen.part,"fraction":(before/total).clamp(0.0,1.0),"angle":angle}))
}
pub(super) fn refresh(d: &mut Drawing, layout: &schema::Layout) -> Result<()> {
    let lines = segments(d)?;
    let marked: Vec<_> = d
        .model
        .iter()
        .filter_map(|e| placement::data(e, "CC_UNIFILARE_DEVICE").transpose().map(|r| r.map(|m| (e.handle, m))))
        .collect::<Result<Vec<_>>>()?;
    for (h, mut meta) in marked {
        if meta["kind"] != "line_marker" || meta["attachment"].is_null() {
            continue;
        }
        let Some(owner) = meta["line"].as_str().and_then(Handle::parse_hex) else {
            continue;
        };
        let Some(part) = meta["attachment"]["part"].as_str() else {
            continue;
        };
        let path: Vec<_> = lines.iter().filter(|v| v.owner == owner && v.part == part).collect();
        if path.is_empty() {
            continue;
        }
        let fraction = meta["attachment"]["fraction"]
            .as_f64()
            .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
            .ok_or_else(|| bad("electrical.unifilar.link", "Posizione del marcatore non valida."))?;
        let old_angle = meta["attachment"]["angle"]
            .as_f64()
            .filter(|v| v.is_finite())
            .ok_or_else(|| bad("electrical.unifilar.link", "Rotazione del marcatore non valida."))?;
        let mut left = fraction * path.iter().map(|v| v.a.dist(v.b)).sum::<f64>();
        let mut chosen = None;
        for seg in &path {
            let len = seg.a.dist(seg.b);
            if left <= len + 1e-8 && len > 1e-8 {
                chosen = Some((*seg, (left / len).clamp(0.0, 1.0)));
                break;
            }
            left -= len;
        }
        let Some((seg, t)) = chosen else {
            continue;
        };
        let at = seg.a + (seg.b - seg.a) * t;
        let mut dir = (seg.b - seg.a) / (seg.b - seg.a).len();
        if dir.y < 0.0 || (dir.y.abs() < 1e-8 && dir.x < 0.0) {
            dir = -dir;
        }
        let angle = dir.y.atan2(dir.x) - std::f64::consts::FRAC_PI_2;
        let entity = d.entity(h).ok_or_else(|| bad("electrical.unifilar.link", "Marcatore assente."))?;
        if d.layer(&entity.common.layer).is_some_and(|l| l.locked || l.frozen || !l.on) {
            return Err(bad("electrical.unifilar.link", "Attiva e sblocca il layer del marcatore."));
        }
        let EntityKind::Insert(insert) = &entity.kind else {
            continue;
        };
        let old_at: [f64; 2] = serde_json::from_value(meta["at"].clone()).map_err(|_| bad("electrical.unifilar.link", "Marcatore non valido."))?;
        if insert.insert.xy() != Vec2::new(old_at[0], old_at[1]) || insert.rotation != 0.0 || insert.scale != Vec3::new(1., 1., 1.) {
            return Err(bad("electrical.unifilar.link", "Annulla la trasformazione generica del marcatore."));
        }
        let source = d.block(&insert.block).ok_or_else(|| bad("electrical.unifilar.link", "Geometria del marcatore assente."))?.clone();
        if source.entities.len() > 20000 {
            return Err(bad("electrical.unifilar.link", "Marcatore troppo complesso."));
        }
        let name = format!("CC_UNIFILARE_MARKER_{}", h.hex());
        let mut block = Block::new(&name);
        for part in source.entities.iter() {
            let mut kind = part.kind.clone();
            if part.common.layer == "UNI_TESTI" {
                kind.transform(&Mat3::translate(Vec2::new(old_at[0], old_at[1]) - at));
            } else {
                kind.transform(&Mat3::rotate(angle - old_angle));
            }
            if let EntityKind::Line(l) = &kind {
                for p in [l.a.xy() + at, l.b.xy() + at] {
                    let projection = p.dot(dir);
                    if projection < seg.a.dot(dir).min(seg.b.dot(dir)) - 0.2
                        || projection > seg.a.dot(dir).max(seg.b.dot(dir)) + 0.2
                        || p.y < layout.drawing_area[1] + 18.0
                    {
                        return Err(bad(
                            "electrical.unifilar.link",
                            "Un marcatore non entra più nel tratto. Spostalo prima di cambiare il percorso.",
                        ));
                    }
                }
            }
            let mut e = Entity::new(d.new_handle(), kind);
            e.common = part.common.clone();
            block.entities.push(e);
        }
        d.blocks.insert(name.clone(), Arc::new(block));
        meta["at"] = json!([at.x, at.y]);
        meta["attachment"]["angle"] = json!(angle);
        d.modify_entity(h, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                i.block = name;
                i.insert = at.to3(0.0);
            }
            placement::tags(&mut e.common, "CC_UNIFILARE_DEVICE", &meta);
        })?;
    }
    Ok(())
}
