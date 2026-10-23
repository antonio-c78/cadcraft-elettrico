//! Interactive tracing policy is separate from stored topology and path geometry.
//! Endpoint references survive moves; only explicit taps create electrical junctions.
use super::*;
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RouteMode {
    #[default]
    Orthogonal,
    Diagonal,
    Free,
}
impl RouteMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Orthogonal => "Ortogonale",
            Self::Diagonal => "45°",
            Self::Free => "Libera",
        }
    }
}
pub(super) fn valid_tap(t: &Tap) -> bool {
    handle(&t.wire).is_ok() && ["route", "bar"].contains(&t.part.as_str()) && t.fraction.is_finite() && (0.0..=1.0).contains(&t.fraction)
}
/// Exact endpoint-preserving orthogonal/octilinear leg. Flip changes the first bend.
fn leg(a: Vec2, b: Vec2, mode: RouteMode, flip: bool) -> Vec<Vec2> {
    let d = b - a;
    if a.near(b, 1e-7) {
        return vec![a];
    }
    if mode == RouteMode::Free || d.x.abs() < 1e-7 || d.y.abs() < 1e-7 {
        return vec![a, b];
    }
    let c = if mode == RouteMode::Diagonal {
        let k = d.x.abs().min(d.y.abs());
        let v = Vec2::new(d.x.signum() * k, d.y.signum() * k);
        if flip { b - v } else { a + v }
    } else if flip {
        Vec2::new(a.x, b.y)
    } else {
        Vec2::new(b.x, a.y)
    };
    routing::simplify(vec![a, c, b])
}
#[derive(Clone)]
enum End {
    Port(Port),
    Tap(Tap),
    Pe(f64),
}
impl Env {
    fn endpoint(&self, d: &Drawing, p: Vec2, tolerance: f64) -> Option<(End, Vec2)> {
        // Symbol anchors win over wire projections, including at a wire endpoint.
        if let Some((port, at)) = self
            .nodes
            .iter()
            .filter(|(_, n)| n.visible)
            .flat_map(|(h, n)| n.ports.iter().map(move |(id, (at, _))| (Port { handle: h.hex(), port: id.clone() }, *at)))
            .filter(|(_, at)| at.dist(p) <= tolerance)
            .min_by(|(_, a), (_, b)| a.dist(p).total_cmp(&b.dist(p)))
        {
            return Some((End::Port(port), at));
        }
        if let Some((tap, at)) = self.pick_tap(d, p, tolerance) {
            return Some((End::Tap(tap), at));
        }
        if let Ok((y, span)) = self.pe()
            && p.x >= span[0]
            && p.x <= span[1]
            && (p.y - y).abs() <= tolerance
        {
            return Some((End::Pe(p.x), Vec2::new(p.x, y)));
        }
        None
    }
    fn start(&self, l: &Link) -> Result<Vec2> {
        if let Some(p) = &l.from {
            self.port(p, "any")
        } else if let Some(t) = &l.tap {
            self.tap_point(t).map(|v| v.0)
        } else if let Some(x) = l.pe_start {
            self.pe().map(|(y, _)| Vec2::new(x, y))
        } else {
            Err(err("Scegli la partenza."))
        }
    }
    fn end(&self, l: &Link) -> Result<Vec2> {
        if let Some(p) = &l.to {
            self.port(p, "any")
        } else if let Some(t) = &l.end_tap {
            self.tap_point(t).map(|v| v.0)
        } else if let Some(x) = l.pe_x {
            self.pe().map(|(y, _)| Vec2::new(x, y))
        } else {
            Err(err("Scegli l’arrivo."))
        }
    }
    fn traced_path(&self, l: &Link, b: Vec2) -> Result<Vec<Vec2>> {
        let a = self.start(l)?;
        let mut path = vec![a];
        for (index, next) in l.waypoints.iter().map(|p| Vec2::new(p[0], p[1])).chain(std::iter::once(b)).enumerate() {
            let prev = path.last().copied().ok_or_else(|| err("Percorso vuoto."))?;
            path.extend(leg(prev, next, l.segment_modes.get(index).copied().unwrap_or(l.route_mode), false).into_iter().skip(1));
        }
        let path = routing::simplify(path);
        if path.len() < 2 || path.len() > 128 {
            return Err(err("Il filo richiede due punti distinti e al massimo 128 vertici."));
        }
        let (pe, span) = self.pe()?;
        for p in &path {
            if !p.is_finite()
                || p.x < self.layout.drawing_area[0]
                || p.x > self.layout.drawing_area[2]
                || p.y < pe
                || p.y > self.layout.drawing_area[3] - 1.
            {
                return Err(err("Il percorso deve restare nell’area di disegno, sopra la barra PE."));
            }
        }
        for x in [l.pe_start, l.pe_x].into_iter().flatten() {
            if x < span[0] || x > span[1] {
                return Err(err("Punto fuori dalla barra PE."));
            }
        }
        for (i, segment) in path.windows(2).enumerate() {
            let boxes: Vec<_> = self
                .boxes
                .iter()
                .filter(|o| {
                    // Only the terminal leg can escape its owning symbol's envelope.
                    !(o.body
                        && ((i == 0 && l.from.as_ref().is_some_and(|p| handle(&p.handle).ok() == Some(o.owner)))
                            || (i + 2 == path.len() && l.to.as_ref().is_some_and(|p| handle(&p.handle).ok() == Some(o.owner)))))
                })
                .map(|o| o.rect)
                .collect();
            if routing::blocked(segment[0], segment[1], &boxes) {
                return Err(err("Il tratto attraversa un simbolo o un testo: aggiungi un punto di passaggio."));
            }
        }
        Ok(path)
    }
    fn needs_dot(&self, at: Vec2) -> bool {
        if self.nodes.values().any(|n| n.ports.values().any(|(p, _)| p.near(at, 1e-6))) {
            return false;
        }
        let mut rays = Vec::<Vec2>::new();
        for (a, b) in self.wires.iter().map(|(_, a, b)| (*a, *b)) {
            let d = b - a;
            let len = d.len();
            if len < 1e-7 {
                continue;
            }
            let t = (at - a).dot(d) / (len * len);
            if !(-1e-6..=1. + 1e-6).contains(&t) || (a + d * t).dist(at) > 1e-6 {
                continue;
            }
            for end in [a, b] {
                let v = end - at;
                if v.len() > 1e-6 {
                    let v = v / v.len();
                    if !rays.iter().any(|r| r.near(v, 1e-6)) {
                        rays.push(v);
                    }
                }
            }
        }
        // Candidate wire adds another leg. For stored geometry all rays are already present.
        rays.len() >= 2
    }
    pub(super) fn traced_geometry(&self, l: &Link) -> Result<(Vec<EntityKind>, Vec<Vec2>)> {
        let b = self.end(l)?;
        let path = self.traced_path(l, b)?;
        let mut g: Vec<_> = path.windows(2).map(|p| line(p[0], p[1])).collect();
        let a = self.start(l)?;
        for (at, tap, pe, end) in [(a, l.tap.as_ref(), l.pe_start.is_some(), false), (b, l.end_tap.as_ref(), l.pe_x.is_some(), true)] {
            let at = if let Some(t) = tap { contacts::junction(self, t, &path, end)? } else { at };
            if pe || (tap.is_some() && self.needs_dot(at)) {
                g.extend(junction(at));
            }
        }
        Ok((g, path))
    }
}
impl Draft {
    pub fn tracking_base(&self) -> Option<Vec2> {
        (self.tracing && self.phase == 9).then(|| self.trace_last().ok()).flatten()
    }
    pub fn trace_mode_label(&self) -> &'static str {
        if self.temporary_45 { "45° · Maiusc" } else { self.trace_mode.label() }
    }
    fn mode(&self) -> RouteMode {
        if self.temporary_45 { RouteMode::Diagonal } else { self.trace_mode }
    }
    fn trace_last(&self) -> Result<Vec2> {
        self.link.waypoints.last().map(|p| Vec2::new(p[0], p[1])).map(Ok).unwrap_or_else(|| self.env.start(&self.link))
    }
    pub(super) fn trace_snap(&self, p: Vec2, tol: f64) -> Option<Vec2> {
        if ![0, 9].contains(&self.phase) {
            return None;
        }
        self.env.endpoint(&self.baseline, p, tol).map(|v| v.1)
    }
    pub(super) fn trace_point(&mut self, p: Vec2) -> Result<()> {
        let picked = self.env.endpoint(&self.baseline, p, 1.5);
        if self.phase == 0 {
            let (end, _) = picked.ok_or_else(|| err("Scegli un punto di un simbolo, un filo o il PE."))?;
            self.link.from = None;
            self.link.tap = None;
            self.link.pe_start = None;
            match end {
                End::Port(p) => self.link.from = Some(p),
                End::Tap(t) => self.link.tap = Some(t),
                End::Pe(x) => self.link.pe_start = Some(x),
            }
            self.phase = 9;
            self.error = None;
            return Ok(());
        }
        let last = self.trace_last()?;
        let at = picked.as_ref().map(|v| v.1).unwrap_or(p);
        let new = leg(last, at, self.mode(), self.bend != self.temporary_flip);
        if last.near(at, 1e-7) {
            return Err(err("Scegli un punto diverso dall’ultimo."));
        }
        let mut link = self.link.clone();
        let end = picked.map(|v| v.0);
        let count = if end.is_some() { new.len().saturating_sub(2) } else { new.len().saturating_sub(1) };
        link.segment_modes.extend(std::iter::repeat_n(self.mode(), new.len().saturating_sub(1)));
        link.waypoints.extend(new.iter().skip(1).take(count).map(|p| [p.x, p.y]));
        if link.waypoints.len() > 32 {
            return Err(err("Al massimo 32 vertici fissati."));
        }
        // Preserve explicit diagonal/free vertices when modes change between clicks.
        link.route_mode = if self.mode() == RouteMode::Free { RouteMode::Free } else { RouteMode::Diagonal };
        if let Some(end) = end {
            link.to = None;
            link.end_tap = None;
            link.pe_x = None;
            match end {
                End::Port(p) => link.to = Some(p),
                End::Tap(t) => link.end_tap = Some(t),
                End::Pe(x) => link.pe_x = Some(x),
            }
            let _ = drawing(self, &link)?;
            self.link = link;
            self.phase = 3;
        } else {
            let mut preview = link.clone();
            preview.waypoints.pop();
            self.env.traced_path(&preview, at)?;
            // Persist vertices but keep the selected UI mode for the next leg.
            self.trace_steps.push(self.link.waypoints.len());
            self.link.segment_modes = link.segment_modes;
            self.link.waypoints = link.waypoints;
            self.link.route_mode = link.route_mode;
        }
        self.error = None;
        Ok(())
    }
    pub(super) fn trace_keyword(&mut self, key: &str) -> Result<bool> {
        match key {
            "ortogonale" => self.trace_mode = RouteMode::Orthogonal,
            "45" | "diagonale" => self.trace_mode = RouteMode::Diagonal,
            "libera" => self.trace_mode = RouteMode::Free,
            "piega" => self.bend = !self.bend,
            "indietro" | "undo" if self.phase == 9 => {
                if let Some(count) = self.trace_steps.pop() {
                    self.link.waypoints.truncate(count);
                    self.link.segment_modes.truncate(count);
                } else {
                    self.phase = 0;
                }
            }
            "punti" | "percorso" | "origine" | "destinazione" => {
                self.link.waypoints.clear();
                self.link.segment_modes.clear();
                self.trace_steps.clear();
                self.link.to = None;
                self.link.end_tap = None;
                self.link.pe_x = None;
                self.phase = if key == "origine" { 0 } else { 9 };
            }
            _ => return Ok(false),
        }
        self.error = None;
        Ok(true)
    }
    pub(super) fn trace_preview(&self, cursor: Vec2) -> Vec<EntityKind> {
        let mut g = Vec::new();
        for n in self.env.nodes.values().filter(|n| n.visible) {
            for (p, _) in n.ports.values() {
                g.push(EntityKind::Circle(cadcraft_doc::Circle { center: p.to3(0.), radius: 0.8 }));
            }
        }
        let at = self.trace_snap(cursor, 1.5).unwrap_or(cursor);
        g.push(EntityKind::Circle(cadcraft_doc::Circle { center: at.to3(0.), radius: 1. }));
        if self.phase == 9
            && let Ok(last) = self.trace_last()
        {
            let mut path = vec![self.env.start(&self.link).unwrap_or(last)];
            path.extend(self.link.waypoints.iter().map(|p| Vec2::new(p[0], p[1])));
            path.extend(leg(last, at, self.mode(), self.bend != self.temporary_flip).into_iter().skip(1));
            g.extend(path.windows(2).map(|p| line(p[0], p[1])));
        }
        g
    }
}

pub(super) fn upgrade(env: &Env, l: &mut Link, target: Option<Handle>) -> Result<()> {
    if l.kind == "wire" {
        return Ok(());
    }
    if l.kind == "bus" {
        return Err(err("La barra mantiene il comando dedicato."));
    }
    let path = target.and_then(|h| env.paths.get(&h)).ok_or_else(|| err("Percorso non disponibile."))?;
    if l.kind == "branch" && l.tap.is_none() {
        let destination = env.port(l.to.as_ref().ok_or_else(|| err("Arrivo assente."))?, "any")?;
        let at = env.branch_start(l, destination)?;
        let bus = l.bus.clone().ok_or_else(|| err("Barra assente."))?;
        let (_, span) = env.bus(&bus)?;
        l.tap = Some(Tap { wire: bus, part: "bar".into(), fraction: (at.x - span[0]) / (span[1] - span[0]) });
    }
    if l.kind == "pe" {
        l.pe_x = path.last().map(|p| p.x);
    }
    l.bus = None;
    l.span = None;
    l.vertical_x = None;
    l.automatic = false;
    l.waypoints = path.iter().skip(1).take(path.len().saturating_sub(2)).map(|p| [p.x, p.y]).collect();
    if l.waypoints.len() > 32 {
        return Err(err("Troppi vertici per la modifica. Semplifica prima il collegamento."));
    }
    l.segment_modes.clear();
    l.route_mode = RouteMode::Orthogonal;
    l.kind = "wire".into();
    l.version = 3;
    Ok(())
}
