//! Junction marks are derived graphics, never the electrical connection itself.
use super::*;
/// Recompute after insert, move, reroute or erase; crossings alone add no candidates.
pub(super) fn rebuild(d: &mut Drawing) -> Result<()> {
    let mut links = BTreeMap::new();
    let mut lines = Vec::new();
    let mut candidates = Vec::new();
    let mut ports = Vec::new();
    for e in d.model.iter() {
        if let Some(m) = placement::data(e, "CC_UNIFILARE_DEVICE")?
            && m["kind"] == "device"
        {
            let i = identity(e)?;
            for p in m["ports"].as_array().into_iter().flatten().take(16) {
                if let Ok(v) = serde_json::from_value::<[f64; 3]>(p["position"].clone()) {
                    ports.push(i.insert.xy() + Vec2::new(v[0], v[1]));
                }
            }
        }
        if !e.common.xdata.iter().any(|t| t.code == 1001 && t.value == APP) {
            continue;
        }
        let l = read_link(d, e.handle)?;
        let m = placement::data(e, APP)?.ok_or_else(|| err("Dati del filo assenti."))?;
        let path: Vec<[f64; 2]> = serde_json::from_value(m["path"].clone()).map_err(|_| err("Percorso non valido."))?;
        if path.len() > 128 {
            return Err(err("Troppi vertici."));
        }
        if let Some(p) = path.first()
            && (l.tap.is_some() || l.kind == "branch")
        {
            candidates.push((e.handle, Vec2::new(p[0], p[1]), false));
        }
        if let Some(p) = path.last()
            && (l.end_tap.is_some() || l.kind == "bus" || l.kind == "pe" || l.pe_x.is_some())
        {
            candidates.push((e.handle, Vec2::new(p[0], p[1]), l.kind == "pe" || l.pe_x.is_some()));
        }
        if let Some(p) = path.first()
            && l.pe_start.is_some()
        {
            candidates.push((e.handle, Vec2::new(p[0], p[1]), true));
        }
        let i = identity(e)?;
        let b = d.block(&i.block).ok_or_else(|| err("Geometria del filo assente."))?;
        for part in b.entities.iter() {
            if let EntityKind::Line(l) = &part.kind {
                lines.push((e.handle, l.a.xy(), l.b.xy()));
            }
        }
        links.insert(e.handle, l);
    }
    if links.len() > 256 || lines.len() > 8192 {
        return Err(err("Rete troppo complessa."));
    }
    let nets: BTreeMap<_, _> = links.iter().map(|(h, l)| net(l, &links).map(|n| (*h, n))).collect::<Result<_>>()?;
    let mut marks: BTreeMap<Handle, Vec<Vec2>> = BTreeMap::new();
    let mut emitted = Vec::<Vec2>::new();
    for (owner, at, pe) in candidates {
        if ports.iter().any(|p| p.near(at, 1e-6)) || emitted.iter().any(|p| p.near(at, 1e-6)) {
            continue;
        }
        let mut rays = Vec::<Vec2>::new();
        for (h, a, b) in &lines {
            if nets.get(h) != nets.get(&owner) {
                continue;
            }
            let v = *b - *a;
            let len = v.len();
            if len < 1e-7 {
                continue;
            }
            let t = (at - *a).dot(v) / (len * len);
            if !(-1e-6..=1. + 1e-6).contains(&t) || (*a + v * t).dist(at) > 1e-6 {
                continue;
            }
            for end in [*a, *b] {
                let delta = end - at;
                let n = delta.len();
                if n > 1e-6 {
                    let ray = delta / n;
                    if !rays.iter().any(|v| v.near(ray, 1e-6)) {
                        rays.push(ray);
                    }
                }
            }
        }
        if pe || rays.len() >= 3 {
            marks.entry(owner).or_default().push(at);
            emitted.push(at);
        }
    }
    for h in links.keys() {
        let i = identity(d.entity(*h).ok_or_else(|| err("Filo assente."))?)?;
        let name = i.block.clone();
        let mut b = d.block(&name).ok_or_else(|| err("Blocco assente."))?.as_ref().clone();
        // All SOLIDs in generated wire blocks are our junction wedges.
        let remove: Vec<_> = b.entities.iter().filter(|e| matches!(e.kind, EntityKind::Solid(_))).map(|e| e.handle).collect();
        for h in remove {
            b.entities.remove(h);
        }
        for at in marks.get(h).into_iter().flatten() {
            for kind in junction(*at) {
                b.entities.push(Entity::new(d.new_handle(), kind));
            }
        }
        d.blocks.insert(name, Arc::new(b));
    }
    Ok(())
}
