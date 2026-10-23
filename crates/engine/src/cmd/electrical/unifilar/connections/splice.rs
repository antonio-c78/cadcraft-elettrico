//! Inserting a two-terminal device into an existing wire preserves both outer anchors.
use super::*;
fn distance(path: &[Vec2], p: Vec2) -> Option<f64> {
    let mut before = 0.;
    for pair in path.windows(2) {
        let delta = pair[1] - pair[0];
        let len = delta.len();
        if len < 1e-7 {
            continue;
        }
        let t = (p - pair[0]).dot(delta) / (len * len);
        if (-1e-7..=1. + 1e-7).contains(&t) && (pair[0] + delta * t).near(p, 1e-6) {
            return Some(before + t.clamp(0., 1.) * len);
        }
        before += len;
    }
    None
}
fn trim(path: &[Vec2], lo: f64, hi: f64, a: Vec2, b: Vec2) -> Vec<Vec2> {
    let mut out = vec![a];
    let mut at = 0.;
    for pair in path.windows(2) {
        at += pair[0].dist(pair[1]);
        if at > lo + 1e-7 && at < hi - 1e-7 {
            out.push(pair[1]);
        }
    }
    out.push(b);
    routing::simplify(out)
}
fn fixed(l: &mut Link, path: &[Vec2]) -> Result<()> {
    l.version = 3;
    l.kind = "wire".into();
    l.bus = None;
    l.span = None;
    l.vertical_x = None;
    l.automatic = false;
    l.waypoints = path.iter().skip(1).take(path.len().saturating_sub(2)).map(|p| [p.x, p.y]).collect();
    l.segment_modes.clear();
    if l.route_mode != tracing::RouteMode::Free {
        l.route_mode = tracing::RouteMode::Diagonal;
    }
    if l.waypoints.len() > 32 {
        return Err(err("Il filo ha troppi vertici per inserire un dispositivo. Semplifica il percorso."));
    }
    Ok(())
}
/// Only a new device whose IN and OUT lie on one existing route is inserted automatically.
/// Ambiguous crossings and branches inside the device are rejected atomically.
pub fn splice(d: &mut Drawing, layout: &schema::Layout, device: Handle) -> Result<bool> {
    let mut env = Env::new(d, layout)?;
    let Some(node) = env.nodes.get(&device) else {
        return Ok(false);
    };
    let (Some((input, _)), Some((output, _))) = (node.ports.get("in"), node.ports.get("out")) else {
        return Ok(false);
    };
    if input.near(*output, 1e-7) {
        return Ok(false);
    }
    let mut found = None;
    for (h, l) in &env.links {
        if l.kind == "bus" {
            continue;
        }
        let Some(path) = env.paths.get(h) else {
            continue;
        };
        let (Some(a), Some(b)) = (distance(path, *input), distance(path, *output)) else {
            continue;
        };
        let total: f64 = path.windows(2).map(|p| p[0].dist(p[1])).sum();
        let lo = a.min(b);
        let hi = a.max(b);
        if lo < 1e-6 || hi > total - 1e-6 {
            continue;
        }
        if found.is_some() {
            return Err(err("Più fili attraversano il simbolo: spostalo o separa i collegamenti prima di inserirlo."));
        }
        let first = if a < b { ("in", *input) } else { ("out", *output) };
        let last = if a < b { ("out", *output) } else { ("in", *input) };
        found = Some((*h, l.clone(), path.clone(), lo, hi, total, first, last));
    }
    let Some((h, original, path, lo, hi, total, first, last)) = found else {
        return Ok(false);
    };
    let start = *path.first().ok_or_else(|| err("Percorso vuoto."))?;
    let end = *path.last().ok_or_else(|| err("Percorso vuoto."))?;
    let left_path = trim(&path, 0., lo, start, first.1);
    let right_path = trim(&path, hi, total, last.1, end);
    let mut left = original.clone();
    let mut right = original;
    left.to = Some(Port { handle: device.hex(), port: first.0.into() });
    left.end_tap = None;
    left.pe_x = None;
    right.from = Some(Port { handle: device.hex(), port: last.0.into() });
    right.tap = None;
    right.pe_start = None;
    fixed(&mut left, &left_path)?;
    fixed(&mut right, &right_path)?;
    let g = left_path.windows(2).map(|p| line(p[0], p[1])).collect();
    store(d, Some(h), &left, g, left_path.clone())?;
    let g = right_path.windows(2).map(|p| line(p[0], p[1])).collect();
    let new = store(d, None, &right, g, right_path.clone())?;
    env.links.insert(h, left);
    env.links.insert(new, right);
    env.paths.insert(h, left_path);
    env.paths.insert(new, right_path);
    // Existing taps retain their physical position on the retained half of the route.
    let mut updates = Vec::new();
    for (owner, link) in &env.links {
        if *owner == h || *owner == new {
            continue;
        }
        let mut link = link.clone();
        let mut changed = false;
        for tap in [&mut link.tap, &mut link.end_tap].into_iter().flatten() {
            if handle(&tap.wire)? != h || tap.part != "route" {
                continue;
            }
            let at = tap.fraction * total;
            if at < lo - 1e-6 {
                tap.fraction = at / lo;
            } else if at > hi + 1e-6 {
                tap.wire = new.hex();
                tap.fraction = (at - hi) / (total - hi);
            } else {
                return Err(err("Nel tratto occupato dal simbolo c’è una giunzione: spostala prima di inserire il dispositivo."));
            }
            changed = true;
        }
        if changed {
            updates.push((*owner, link));
        }
    }
    for (owner, link) in updates {
        let path = env.paths.get(&owner).cloned().ok_or_else(|| err("Percorso assente."))?;
        let g = path.windows(2).map(|p| line(p[0], p[1])).collect();
        store(d, Some(owner), &link, g, path)?;
    }
    let mut markers = Vec::new();
    for e in d.model.iter() {
        if let Some(mut m) = placement::data(e, "CC_UNIFILARE_DEVICE")?
            && m["kind"] == "line_marker"
            && m["line"] == h.hex()
        {
            let fraction = m["attachment"]["fraction"]
                .as_f64()
                .filter(|v| v.is_finite() && (0.0..=1.).contains(v))
                .ok_or_else(|| err("Posizione del marcatore non valida."))?;
            let at = fraction * total;
            if at < lo - 1e-6 {
                m["attachment"]["fraction"] = json!(at / lo);
            } else if at > hi + 1e-6 {
                m["line"] = json!(new.hex());
                m["attachment"]["fraction"] = json!((at - hi) / (total - hi));
            } else {
                return Err(err("Nel tratto occupato dal simbolo c’è un marcatore: spostalo prima di inserire il dispositivo."));
            }
            markers.push((e.handle, m));
        }
    }
    for (h, m) in markers {
        d.modify_entity(h, |e| placement::tags(&mut e.common, "CC_UNIFILARE_DEVICE", &m))?;
    }
    Ok(true)
}
