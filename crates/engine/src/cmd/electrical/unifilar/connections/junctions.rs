//! Stable junction anchors and an inspectable, versioned logical network.
use super::*;
impl Env {
    fn tap_path(&self, t: &Tap) -> Result<Vec<Vec2>> {
        let h = handle(&t.wire)?;
        let _wire = self.links.get(&h).ok_or_else(|| err("Il filo della giunzione non esiste più."))?;
        if t.part == "bar" {
            let (bus, span) = self.bus(&t.wire)?;
            Ok(vec![Vec2::new(span[0], bus.y), Vec2::new(span[1], bus.y)])
        } else {
            self.paths.get(&h).filter(|v| v.len() >= 2).cloned().ok_or_else(|| err("Il filo non ha un percorso valido."))
        }
    }
    pub(super) fn tap_point(&self, t: &Tap) -> Result<(Vec2, Vec2)> {
        let path = self.tap_path(t)?;
        let total: f64 = path.windows(2).map(|p| p[0].dist(p[1])).sum();
        if total < 1e-7 {
            return Err(err("Tratto di giunzione troppo corto."));
        }
        let mut left = t.fraction * total;
        for p in path.windows(2) {
            let length = p[0].dist(p[1]);
            if length > 1e-7 && left <= length + 1e-7 {
                return Ok((p[0] + (p[1] - p[0]) * (left / length).clamp(0., 1.), (p[1] - p[0]) / length));
            }
            left -= length;
        }
        Err(err("Giunzione fuori dal percorso."))
    }
    pub(super) fn tap_escape(&self, l: &Link, a: Vec2, b: Vec2) -> Result<Vec2> {
        let tap = l.tap.as_ref().ok_or_else(|| err("Giunzione mancante."))?;
        let (_, direction) = self.tap_point(tap)?;
        let horizontal = direction.x.abs() < 1e-7;
        let preferred = if horizontal { Vec2::new(if b.x >= a.x { 2. } else { -2. }, 0.) } else { Vec2::new(0., if b.y >= a.y { 2. } else { -2. }) };
        let parent = handle(&tap.wire)?;
        let boxes: Vec<_> = self.boxes.iter().map(|o| o.rect).collect();
        for offset in [preferred, -preferred] {
            let p = a + offset;
            if p.x >= self.layout.drawing_area[0]
                && p.x <= self.layout.drawing_area[2]
                && p.y >= self.layout.columns.first().map(|c| c.symbol_y_range[0]).unwrap_or(171.)
                && p.y <= self.layout.drawing_area[3] - 1.
                && !routing::blocked(a, p, &boxes)
                && !self.wires.iter().any(|(h, c, d)| *h == parent && routing::overlaps_wire(a, p, *c, *d))
            {
                return Ok(p);
            }
        }
        Err(err("La giunzione non ha spazio libero per uscire dal filo. Scegli un punto vicino."))
    }
    pub(super) fn branch_start(&self, l: &Link, destination: Vec2) -> Result<Vec2> {
        if let Some(t) = &l.tap {
            return self.tap_point(t).map(|p| p.0);
        }
        let (bus, span) = self.bus(l.bus.as_deref().ok_or_else(|| err("Scegli il filo o la barra di origine."))?)?;
        Ok(Vec2::new(destination.x.clamp(span[0], span[1]), bus.y))
    }
    pub(super) fn pick_tap(&self, d: &Drawing, p: Vec2, tolerance: f64) -> Option<(Tap, Vec2)> {
        let mut best = None;
        let mut distance = tolerance;
        for (h, l) in &self.links {
            if d.entity(*h).is_none_or(|e| !e.common.visible || d.layer(&e.common.layer).is_some_and(|l| !l.on || l.frozen)) {
                continue;
            }
            for part in if l.kind == "bus" { &["route", "bar"][..] } else { &["route"][..] } {
                let template = Tap { wire: h.hex(), part: (*part).into(), fraction: 0. };
                let Ok(path) = self.tap_path(&template) else {
                    continue;
                };
                let total: f64 = path.windows(2).map(|p| p[0].dist(p[1])).sum();
                let mut before = 0.;
                for segment in path.windows(2) {
                    let delta = segment[1] - segment[0];
                    let length = delta.len();
                    if length < 1e-7 {
                        continue;
                    }
                    let fraction = ((p - segment[0]).dot(delta) / (length * length)).clamp(0., 1.);
                    let at = segment[0] + delta * fraction;
                    if p.dist(at) < distance {
                        distance = p.dist(at);
                        best = Some((Tap { fraction: (before + fraction * length) / total, ..template.clone() }, at));
                    }
                    before += length;
                }
            }
        }
        best
    }
}
impl Draft {
    /// A free same-axis connection needs no routing-level or horizontal-offset question.
    pub fn simple_vertical(&self) -> bool {
        if !self.link.automatic || !self.link.waypoints.is_empty() {
            return false;
        }
        let Ok(g) = drawing(self, &self.link) else {
            return false;
        };
        let mut x = None;
        for kind in g {
            if let EntityKind::Line(l) = kind {
                let axis = *x.get_or_insert(l.a.x);
                if (l.a.x - axis).abs() > 1e-7 || (l.b.x - axis).abs() > 1e-7 {
                    return false;
                }
            }
        }
        x.is_some()
    }
}
pub(super) fn ready_target(d: &mut Draft) -> Result<()> {
    let to = d.link.to.as_ref().ok_or_else(|| err("Scegli l'arrivo."))?;
    let b = d.env.port(to, "in")?;
    let a = if d.link.tap.is_some() || d.link.kind == "branch" {
        d.env.branch_start(&d.link, b)?
    } else {
        d.env.port(d.link.from.as_ref().ok_or_else(|| err("Scegli l'origine."))?, "out")?
    };
    d.link.waypoints.clear();
    if (a.x - b.x).abs() < 1e-7 || d.link.tap.is_some() {
        d.link.automatic = true;
        d.link.vertical_x = None;
        d.link.y = a.y;
        // If the automatic detour is impossible, keep the endpoints and offer manual routing.
        if drawing(d, &d.link).is_ok() {
            d.phase = 3;
            return Ok(());
        }
    }
    d.link.automatic = false;
    d.phase = 2;
    Ok(())
}
pub(super) fn graph_read(s: &mut Session, p: &Value) -> Result<Value> {
    let (layout, _, _) = schema::load(p["model"].as_str().ok_or_else(|| err("Indica il modello."))?)?;
    let env = Env::new(s.doc()?, &placement::sheet(s, &layout)?)?;
    validate_graph(&env.links)?;
    let mut junctions = BTreeMap::new();
    let mut wires = Vec::new();
    for (h, l) in &env.links {
        if let Some(p) = &l.from {
            env.port(p, if l.kind == "pe" { "pe" } else { "out" })?;
        }
        if let Some(p) = &l.to {
            env.port(p, "in")?;
        }
        for t in l.end_tap.iter() {
            let (at, _) = env.tap_point(t)?;
            let id = format!("{}:{}:{:.9}", t.wire, t.part, t.fraction);
            junctions.insert(id.clone(), json!({"id":id,"wire":t.wire,"part":t.part,"fraction":t.fraction,"at":[at.x,at.y]}));
        }
        let junction = if let Some(t) = &l.tap {
            let (at, _) = env.tap_point(t)?;
            let id = format!("{}:{}:{:.9}", t.wire, t.part, t.fraction);
            junctions.insert(id.clone(), json!({"id":id,"wire":t.wire,"part":t.part,"fraction":t.fraction,"at":[at.x,at.y]}));
            Some(id)
        } else {
            None
        };
        wires.push(json!({"handle":h.hex(),"source":l.from,"junction":junction,"bus":l.bus,"destination":l.to,
            "end_junction":l.end_tap,"pe_start":l.pe_start,"pe_x":l.pe_x,"net":net(l,&env.links)?,"waypoints":l.waypoints,"automatic":l.automatic}));
    }
    let ports: Vec<_> = env
        .nodes
        .iter()
        .flat_map(|(h, n)| n.ports.iter().map(move |(id, (at, role))| json!({"device":h.hex(),"port":id,"role":role,"at":[at.x,at.y]})))
        .collect();
    Ok(json!({"version":3,"ports":ports,"wires":wires,"junctions":junctions.into_values().collect::<Vec<_>>()}))
}

/// Move one orthogonal segment; intermediate coordinates become explicit fixed anchors.
pub(super) fn shift_segment(d: &Draft, p: Vec2) -> Result<Link> {
    let index = d.moving_segment.ok_or_else(|| err("Seleziona un tratto."))?;
    let mut path = d.editing_path.clone();
    let a = *path.get(index).ok_or_else(|| err("Tratto non disponibile."))?;
    let b = *path.get(index + 1).ok_or_else(|| err("Tratto non disponibile."))?;
    if d.link.kind == "wire" {
        let v = b - a;
        let length = v.len();
        if length < 1e-7 {
            return Err(err("Tratto troppo corto."));
        }
        let normal = Vec2::new(-v.y, v.x) / length;
        let offset = normal * (p - a).dot(normal);
        if let Some(q) = path.get_mut(index) {
            *q += offset;
        }
        if let Some(q) = path.get_mut(index + 1) {
            *q += offset;
        }
        let mut link = d.link.clone();
        link.waypoints.clear();
        link.segment_modes.clear();
        if index == 0 {
            link.waypoints.push([path[0].x, path[0].y]);
        }
        link.waypoints.extend(path.iter().skip(1).take(path.len().saturating_sub(2)).map(|p| [p.x, p.y]));
        if index + 2 == path.len()
            && let Some(q) = path.last()
        {
            link.waypoints.push([q.x, q.y]);
        }
        if link.waypoints.len() > 32 {
            return Err(err("Troppi vertici."));
        }
        return Ok(link);
    }
    let vertical = (a.x - b.x).abs() < 1e-7;
    if let Some(q) = path.get_mut(index) {
        if vertical {
            q.x = p.x;
        } else {
            q.y = p.y;
        }
    }
    if let Some(q) = path.get_mut(index + 1) {
        if vertical {
            q.x = p.x;
        } else {
            q.y = p.y;
        }
    }
    let last = path.len().saturating_sub(1);
    if index == 0
        && vertical
        && let Some(q) = path.first_mut()
    {
        q.y -= 2.;
    }
    if index + 1 == last
        && vertical
        && let Some(q) = path.last_mut()
    {
        q.y += 2.;
    }
    let mut link = d.link.clone();
    link.automatic = false;
    link.vertical_x = None;
    link.waypoints = path
        .into_iter()
        .enumerate()
        .filter(|(i, _)| (*i > 0 && *i < last) || (*i == 0 && index == 0) || (*i == last && index + 1 == last))
        .map(|(_, p)| [p.x, p.y])
        .collect();
    if link.waypoints.len() > 32 {
        return Err(err("Troppi tratti: usa i punti di passaggio per semplificare il percorso."));
    }
    Ok(link)
}
