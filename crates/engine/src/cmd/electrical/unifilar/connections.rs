//! Attached, orthogonal unifilar connections. All changes commit together after a backup.
use super::super::*;
use super::{placement, routing, schema};
use cadcraft_doc::{Drawing, Handle, Solid};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
const CMD: &str = "electrical.unifilar.link";
const APP: &str = "CC_UNIFILARE_LINK";
type PreviewCache = Arc<Mutex<Option<(String, std::result::Result<Vec<EntityKind>, String>)>>>;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Port {
    pub handle: String,
    pub port: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Link {
    pub version: u8,
    pub kind: String,
    pub from: Option<Port>,
    pub to: Option<Port>,
    pub bus: Option<String>,
    pub span: Option<[f64; 2]>,
    pub y: f64,
    #[serde(default)]
    pub vertical_x: Option<f64>,
}
#[derive(Clone)]
pub struct Draft {
    pub link: Link,
    pub phase: u8,
    pub error: Option<String>,
    pub target: Option<Handle>,
    uid: u64,
    baseline: Arc<Drawing>,
    layout: schema::Layout,
    backup_dir: Option<String>,
    env: Arc<Env>,
    cache: PreviewCache,
    vertical_anchor: Option<f64>,
}
impl Draft {
    pub fn proposed_vertical_x(&self) -> Option<f64> {
        self.link.vertical_x.or_else(|| vertical_default(self).ok().flatten())
    }
    pub fn description(&self) -> String {
        describe(&self.link, &self.baseline)
    }
    pub fn instruction(&self) -> &'static str {
        if self.phase == 3 {
            return "Controlla l'anteprima, poi conferma. Puoi tornare indietro.";
        }
        match (self.link.kind.as_str(), self.phase) {
            (_, 5) => "Scegli il passaggio verticale: al centro fra le colonne, oppure spostalo con il mouse. Invio mantiene il valore proposto.",
            ("branch", 0) => "Scegli la barra comune a cui collegare la derivazione.",
            ("pe", 0) => "Scegli il punto PE del simbolo (cerchio evidenziato).",
            (_, 0) => "Scegli l'uscita del simbolo di partenza (cerchio evidenziato).",
            (_, 1) => "Scegli l'ingresso del simbolo di arrivo (cerchio evidenziato).",
            ("bus", 4) => "Scegli l'altra estremità della barra. L'altezza resta quella del primo punto.",
            ("bus", _) => "Scegli la prima estremità e l'altezza della barra comune.",
            _ => "Scegli l'altezza dei tratti orizzontali. Il percorso evita simboli e testi.",
        }
    }
}
#[derive(Clone)]
struct Node {
    ports: BTreeMap<String, (Vec2, String)>,
    visible: bool,
}
struct Obstacle {
    owner: Handle,
    body: bool,
    rect: routing::Rect,
}
struct Env {
    nodes: BTreeMap<Handle, Node>,
    boxes: Vec<Obstacle>,
    links: BTreeMap<Handle, Link>,
    layout: schema::Layout,
    wires: Vec<(Handle, Vec2, Vec2)>,
}
fn err(msg: impl Into<String>) -> crate::EngineError {
    bad(CMD, msg)
}
fn handle(text: &str) -> Result<Handle> {
    Handle::parse_hex(text).ok_or_else(|| err("Riferimento non valido."))
}
fn identity(e: &Entity) -> Result<&Insert> {
    let EntityKind::Insert(i) = &e.kind else {
        return Err(err("Oggetto unifilare non valido."));
    };
    if i.rotation != 0.0 || i.scale != Vec3::new(1.0, 1.0, 1.0) {
        return Err(err("Annulla la trasformazione generica e usa la modifica elettrica."));
    }
    Ok(i)
}
pub(super) fn read_link(d: &Drawing, h: Handle) -> Result<Link> {
    if d.space_of(h) != Some(Space::Model) {
        return Err(err("Il collegamento deve essere nello spazio modello."));
    }
    let e = d.entity(h).ok_or_else(|| err("Collegamento non disponibile."))?;
    let i = identity(e)?;
    if i.insert != Vec3::ZERO {
        return Err(err("Annulla lo spostamento generico del collegamento e usa Modifica collegamento."));
    }
    let m = placement::data(e, APP)?.ok_or_else(|| err("Seleziona un collegamento o una barra unifilare."))?;
    if m["owner"] != h.hex() {
        return Err(err("Una copia generica del collegamento non è valida: eliminala e crea un nuovo collegamento."));
    }
    let link: Link = serde_json::from_value(m["link"].clone()).map_err(|_| err("Dati del collegamento non validi."))?;
    if link.version != 1
        || !["series", "bus", "branch", "pe"].contains(&link.kind.as_str())
        || !link.y.is_finite()
        || link.y.abs() > 1000.0
        || link.vertical_x.is_some_and(|x| !x.is_finite() || x.abs() > 1000.0)
        || link.span.is_some_and(|s| s.iter().any(|v| !v.is_finite() || v.abs() > 1000.0))
        || [link.from.as_ref(), link.to.as_ref()].into_iter().flatten().any(|p| p.handle.len() > 32 || p.port.len() > 64)
        || link.bus.as_ref().is_some_and(|s| s.len() > 32)
    {
        return Err(err("Dati del collegamento non validi."));
    }
    Ok(link)
}
impl Env {
    fn new(d: &Drawing, layout: &schema::Layout) -> Result<Self> {
        let mut env = Self { nodes: BTreeMap::new(), boxes: Vec::new(), links: BTreeMap::new(), layout: layout.clone(), wires: Vec::new() };
        let mut scanned = 0usize;
        if d.entity_count() > 10000 {
            return Err(err("Tavola troppo complessa."));
        }
        for e in d.model.iter() {
            if e.common.xdata.iter().any(|t| t.code == 1001 && t.value == APP) {
                env.links.insert(e.handle, read_link(d, e.handle)?);
                if let EntityKind::Insert(i) = &e.kind
                    && let Some(block) = d.block(&i.block)
                {
                    if block.entities.len() > 8192 {
                        return Err(err("Geometria del collegamento troppo complessa."));
                    }
                    for part in block.entities.iter() {
                        if let EntityKind::Line(l) = &part.kind {
                            env.wires.push((e.handle, l.a.xy(), l.b.xy()));
                        }
                    }
                    if env.wires.len() > 8192 {
                        return Err(err("Troppi tratti di collegamento."));
                    }
                }
            }
            if let Some(m) = placement::data(e, "CC_UNIFILARE_DEVICE")? {
                if m["kind"] != "device" {
                    continue;
                }
                let i = identity(e)?;
                let at: [f64; 2] = serde_json::from_value(m["at"].clone()).map_err(|_| err("Posizione del simbolo non valida."))?;
                if i.insert.xy() != Vec2::new(at[0], at[1]) {
                    return Err(err("Un simbolo è stato spostato con comandi generici. Annulla e usa Modifica apparecchio."));
                }
                let mut ports = BTreeMap::new();
                for p in m["ports"].as_array().filter(|v| v.len() <= 16).ok_or_else(|| err("Punti di connessione non validi."))? {
                    let id = p["id"].as_str().filter(|s| s.len() <= 64).ok_or_else(|| err("Punto non valido."))?;
                    let pos: [f64; 3] = serde_json::from_value(p["position"].clone()).map_err(|_| err("Punto non valido."))?;
                    if pos.iter().any(|v| !v.is_finite() || v.abs() > 1000.0) {
                        return Err(err("Punto non valido."));
                    }
                    ports.insert(id.into(), (i.insert.xy() + Vec2::new(pos[0], pos[1]), p["role"].as_str().unwrap_or("").into()));
                }
                let block = d.block(&i.block).ok_or_else(|| err("Geometria del simbolo assente."))?;
                scanned = scanned.saturating_add(block.entities.len());
                if scanned > 200000 {
                    return Err(err("Geometria dei simboli troppo complessa."));
                }
                let mut bounds = Bounds2::EMPTY;
                for part in block.entities.iter() {
                    match &part.kind {
                        EntityKind::Text(t) => {
                            let p = t.insert.xy() + i.insert.xy();
                            let w = cadcraft_fonts::line_width(&t.value, t.height, t.width_factor);
                            if p.y >= layout.drawing_area[1] {
                                env.boxes.push(Obstacle {
                                    owner: e.handle,
                                    body: false,
                                    rect: [p.x - 0.8, p.y - 0.8, p.x + w + 0.8, p.y + t.height + 0.8],
                                });
                            }
                        }
                        EntityKind::Line(l) if part.common.layer == "UNI_TESTI" => {
                            let a = l.a.xy() + i.insert.xy();
                            let b = l.b.xy() + i.insert.xy();
                            env.boxes.push(Obstacle {
                                owner: e.handle,
                                body: false,
                                rect: [a.x.min(b.x) - 0.8, a.y.min(b.y) - 0.8, a.x.max(b.x) + 0.8, a.y.max(b.y) + 0.8],
                            });
                        }
                        EntityKind::Line(l) if part.common.layer != "UNI_TESTI" => {
                            bounds = bounds.union(&Bounds2::from_points([l.a.xy() + i.insert.xy(), l.b.xy() + i.insert.xy()]));
                        }
                        _ => {}
                    }
                }
                if !bounds.is_empty() {
                    env.boxes.push(Obstacle {
                        owner: e.handle,
                        body: true,
                        rect: [bounds.min.x - 0.6, bounds.min.y - 0.6, bounds.max.x + 0.6, bounds.max.y + 0.6],
                    });
                }
                let visible = e.common.visible && d.layer(&e.common.layer).is_none_or(|l| l.on && !l.frozen);
                env.nodes.insert(e.handle, Node { ports, visible });
            } else if let EntityKind::Text(t) = &e.kind
                && t.insert.y >= layout.drawing_area[1]
            {
                let p = t.insert.xy();
                let w = cadcraft_fonts::line_width(&t.value, t.height, t.width_factor);
                env.boxes.push(Obstacle { owner: e.handle, body: false, rect: [p.x - 0.8, p.y - 0.8, p.x + w + 0.8, p.y + t.height + 0.8] });
            }
        }
        if env.nodes.len() > 64 || env.links.len() > 256 || env.boxes.len() > 256 {
            return Err(err("Troppi oggetti nella tavola."));
        }
        Ok(env)
    }
    fn port(&self, p: &Port, id: &str) -> Result<Vec2> {
        if p.port != id {
            return Err(err(format!("Scegli il punto {id} previsto per questo collegamento.")));
        }
        let (at, role) = self
            .nodes
            .get(&handle(&p.handle)?)
            .and_then(|n| n.ports.get(&p.port))
            .ok_or_else(|| err("Punto assente: verifica i simboli collegati."))?;
        if role != if id == "pe" { "pe" } else { "power" } {
            return Err(err("Tipo di punto non compatibile."));
        }
        Ok(*at)
    }
    fn choose(&self, p: Vec2, id: &str) -> Result<Port> {
        self.nodes
            .iter()
            .filter(|(_, node)| node.visible)
            .flat_map(|(h, n)| {
                n.ports
                    .iter()
                    .filter(move |(key, (_, role))| key.as_str() == id && role == if id == "pe" { "pe" } else { "power" })
                    .map(move |(key, (at, _))| (p.dist(*at), Port { handle: h.hex(), port: key.clone() }))
            })
            .filter(|(dist, _)| *dist <= 4.0)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|v| v.1)
            .ok_or_else(|| err("Clicca sul cerchio del punto di connessione evidenziato."))
    }
    fn bus(&self, h: &str) -> Result<(&Link, [f64; 2])> {
        let l = self.links.get(&handle(h)?).filter(|l| l.kind == "bus").ok_or_else(|| err("La barra comune non è più disponibile."))?;
        let span = l.span.filter(|v| v[1] - v[0] >= 2.0).ok_or_else(|| err("Estremità della barra non valide."))?;
        Ok((l, span))
    }
    fn pe(&self) -> Result<(f64, [f64; 2])> {
        let y = self.layout.pe["y"].as_f64().filter(|v| v.is_finite()).ok_or_else(|| err("Barra PE non configurata."))?;
        let span: [f64; 2] = serde_json::from_value(self.layout.pe["x_range"].clone()).map_err(|_| err("Barra PE non configurata."))?;
        if span.iter().any(|v| !v.is_finite()) || span[0] >= span[1] {
            return Err(err("Barra PE non valida."));
        }
        Ok((y, span))
    }
    fn stub(&self, p: &Port, id: &str) -> Result<(Vec2, Vec2)> {
        let at = self.port(p, id)?;
        let escape = at + Vec2::new(0.0, if id == "in" { 2.0 } else { -2.0 });
        let own = handle(&p.handle)?;
        let boxes: Vec<_> = self.boxes.iter().filter(|o| !(o.owner == own && o.body)).map(|o| o.rect).collect();
        if routing::blocked(at, escape, &boxes) {
            return Err(err("Il punto di connessione è coperto da un testo o da un altro simbolo."));
        }
        Ok((at, escape))
    }
    fn path(&self, a: Vec2, b: Vec2, y: f64, pe: bool, link: &Link) -> Result<Vec<Vec2>> {
        let mut area = self.layout.drawing_area;
        area[1] = if pe { self.pe()?.0 } else { self.layout.columns.first().map(|c| c.symbol_y_range[0]).unwrap_or(area[1]) };
        area[3] -= 1.0;
        let boxes: Vec<_> = self.boxes.iter().map(|o| o.rect).collect();
        let lanes: Vec<_> = self.layout.columns.iter().flat_map(|c| [c.bounds[0] + 2.0, c.bounds[2] - 2.0]).collect();
        let root = net(link, &self.links)?;
        let wires: Vec<_> = self
            .wires
            .iter()
            .filter(|(h, _, _)| self.links.get(h).and_then(|l| net(l, &self.links).ok()).as_ref() != Some(&root))
            .map(|(_, a, b)| (*a, *b))
            .collect();
        if matches!(link.kind.as_str(), "series" | "branch") && (a.x - b.x).abs() > 1e-7 {
            let x = link.vertical_x.unwrap_or((a.x + b.x) * 0.5);
            let corner = Vec2::new(x, y);
            let top = Vec2::new(x, b.y);
            if !x.is_finite()
                || x < area[0]
                || x > area[2]
                || routing::blocked(corner, top, &boxes)
                || routing::blocked(top, b, &boxes)
                || wires.iter().any(|(c, d)| routing::overlaps_wire(corner, top, *c, *d) || routing::overlaps_wire(top, b, *c, *d))
            {
                return Err(err("Passaggio verticale non disponibile: spostalo oppure cambia altezza."));
            }
            let mut path = routing::route(a, corner, y, area, &boxes, &lanes, &wires)?;
            path.extend([top, b]);
            Ok(routing::simplify(path))
        } else {
            routing::route(a, b, y, area, &boxes, &lanes, &wires)
        }
    }
    fn geometry(&self, l: &Link) -> Result<(Vec<EntityKind>, Vec<Vec2>)> {
        let from = || l.from.as_ref().ok_or_else(|| err("Scegli il simbolo di partenza."));
        let to = || l.to.as_ref().ok_or_else(|| err("Scegli il simbolo di arrivo."));
        let mut dot = None;
        let (points, bar) = match l.kind.as_str() {
            "series" => {
                let (a, ea) = self.stub(from()?, "out")?;
                let (b, eb) = self.stub(to()?, "in")?;
                let mut p = vec![a];
                p.extend(self.path(ea, eb, l.y, false, l)?);
                p.push(b);
                (routing::simplify(p), None)
            }
            "bus" => {
                let (a, ea) = self.stub(from()?, "out")?;
                let span = l.span.filter(|s| s[1] - s[0] >= 2.0).ok_or_else(|| err("La barra deve essere lunga almeno 2 mm."))?;
                let b = Vec2::new(a.x.clamp(span[0], span[1]), l.y);
                let start = Vec2::new(span[0], l.y);
                let end = Vec2::new(span[1], l.y);
                let boxes: Vec<_> = self.boxes.iter().map(|o| o.rect).collect();
                if routing::blocked(start, end, &boxes) || span[0] < self.layout.drawing_area[0] || span[1] > self.layout.drawing_area[2] {
                    return Err(err("La barra attraversa un simbolo o un testo. Scegli un'altra altezza."));
                }
                let mut p = vec![a];
                if self.wires.iter().any(|(h, a, b)| {
                    self.links.get(h).and_then(|v| net(v, &self.links).ok()) != net(l, &self.links).ok() && routing::overlaps_wire(start, end, *a, *b)
                }) {
                    return Err(err("La barra si sovrappone a un altro circuito. Scegli un'altra altezza."));
                }
                p.extend(self.path(ea, b, l.y, false, l)?);
                if b.x > span[0] + 0.1 && b.x < span[1] - 0.1 {
                    dot = Some(b);
                }
                (routing::simplify(p), Some((start, end)))
            }
            "branch" => {
                let (bus, span) = self.bus(l.bus.as_deref().ok_or_else(|| err("Scegli la barra comune."))?)?;
                let (b, eb) = self.stub(to()?, "in")?;
                let a = Vec2::new(b.x.clamp(span[0], span[1]), bus.y);
                let ea = a + Vec2::new(0.0, if eb.y >= a.y { 2.0 } else { -2.0 });
                let boxes: Vec<_> = self.boxes.iter().map(|o| o.rect).collect();
                if routing::blocked(a, ea, &boxes) {
                    return Err(err("La derivazione parte dentro un simbolo. Sposta la barra."));
                }
                let mut p = vec![a];
                p.extend(self.path(ea, eb, l.y, false, l)?);
                p.push(b);
                dot = Some(a);
                (routing::simplify(p), None)
            }
            "pe" => {
                let (a, ea) = self.stub(from()?, "pe")?;
                let (y, span) = self.pe()?;
                let b = Vec2::new(a.x.clamp(span[0], span[1]), y);
                let mut p = vec![a];
                p.extend(self.path(ea, b, y, true, l)?);
                dot = Some(b);
                (routing::simplify(p), None)
            }
            _ => return Err(err("Tipo di collegamento non valido.")),
        };
        let mut geometry: Vec<_> = points.windows(2).map(|p| line(p[0], p[1])).collect();
        if let Some((a, b)) = bar {
            geometry.push(line(a, b));
        }
        if let Some(at) = dot {
            geometry.extend(junction(at));
        }
        Ok((geometry, points))
    }
}
fn junction(at: Vec2) -> Vec<EntityKind> {
    (0..8)
        .map(|i| {
            let a = at + Vec2::from_angle(f64::from(i) * std::f64::consts::TAU / 8.0) * 0.55;
            let b = at + Vec2::from_angle(f64::from(i + 1) * std::f64::consts::TAU / 8.0) * 0.55;
            EntityKind::Solid(Solid { corners: [at.to3(0.0), a.to3(0.0), b.to3(0.0), b.to3(0.0)] })
        })
        .collect()
}

fn power_pair(l: &Link, links: &BTreeMap<Handle, Link>) -> Result<Option<(Handle, Handle)>> {
    let from = match l.kind.as_str() {
        "series" => l.from.as_ref(),
        "branch" => links
            .get(&handle(l.bus.as_deref().ok_or_else(|| err("Barra comune mancante."))?)?)
            .filter(|v| v.kind == "bus")
            .and_then(|v| v.from.as_ref()),
        _ => return Ok(None),
    }
    .ok_or_else(|| err("Origine del collegamento mancante."))?;
    let to = l.to.as_ref().ok_or_else(|| err("Arrivo del collegamento mancante."))?;
    Ok(Some((handle(&from.handle)?, handle(&to.handle)?)))
}
fn net(l: &Link, links: &BTreeMap<Handle, Link>) -> Result<(String, String)> {
    if l.kind == "pe" {
        return Ok(("PE".into(), "pe".into()));
    }
    let from = if l.kind == "branch" {
        links.get(&handle(l.bus.as_deref().ok_or_else(|| err("Barra assente."))?)?).and_then(|b| b.from.as_ref())
    } else {
        l.from.as_ref()
    }
    .ok_or_else(|| err("Origine assente."))?;
    Ok((from.handle.clone(), from.port.clone()))
}
fn validate_graph(links: &BTreeMap<Handle, Link>) -> Result<()> {
    if links.len() > 256 {
        return Err(err("Troppi collegamenti."));
    }
    let mut inputs = BTreeSet::new();
    let mut edges: BTreeMap<Handle, Vec<Handle>> = BTreeMap::new();
    let mut pe = BTreeSet::new();
    let mut outputs = BTreeSet::new();
    for l in links.values() {
        if matches!(l.kind.as_str(), "series" | "bus")
            && let Some(from) = &l.from
            && !outputs.insert((&from.handle, &from.port))
        {
            return Err(err("L'uscita è già collegata. Per il parallelo usa la barra comune e le derivazioni."));
        }
        if let Some((a, b)) = power_pair(l, links)? {
            if a == b {
                return Err(err("Un apparecchio non può alimentare se stesso."));
            }
            if !inputs.insert(b) {
                return Err(err("L'ingresso è già collegato. Modifica o elimina il collegamento esistente."));
            }
            edges.entry(a).or_default().push(b);
        }
        if l.kind == "pe" && !pe.insert(l.from.as_ref().map(|p| (&p.handle, &p.port))) {
            return Err(err("Il punto PE è già collegato."));
        }
    }
    for start in edges.keys() {
        let mut seen = BTreeSet::new();
        let mut todo = edges.get(start).cloned().unwrap_or_default();
        while let Some(h) = todo.pop() {
            if h == *start {
                return Err(err("Il collegamento creerebbe un circuito chiuso di alimentazione."));
            }
            if seen.insert(h)
                && let Some(next) = edges.get(&h)
            {
                todo.extend_from_slice(next);
            }
        }
    }
    Ok(())
}
fn editable(d: &Drawing, h: Handle) -> Result<()> {
    let e = d.entity(h).ok_or_else(|| err("Oggetto assente."))?;
    if d.layer(&e.common.layer).is_some_and(|l| l.locked || l.frozen || !l.on) {
        return Err(err("Attiva e sblocca il layer del collegamento."));
    }
    Ok(())
}
fn store(d: &mut Drawing, target: Option<Handle>, l: &Link, geometry: Vec<EntityKind>, path: Vec<Vec2>) -> Result<Handle> {
    let layer = if l.kind == "pe" { "UNI_PE" } else { "UNI_COLLEGAMENTI" };
    if d.layer(layer).is_some_and(|l| l.locked || l.frozen || !l.on) {
        return Err(err(format!("Attiva e sblocca {layer}.")));
    }
    d.ensure_layer(layer);
    let name = if let Some(h) = target {
        editable(d, h)?;
        identity(d.entity(h).ok_or_else(|| err("Collegamento assente."))?)?.block.clone()
    } else {
        (1..=100000).map(|n| format!("CC_UNIFILARE_LINK_{n}")).find(|n| d.block(n).is_none()).ok_or_else(|| err("Troppi blocchi."))?
    };
    if d.model.iter().filter(|e| matches!(&e.kind,EntityKind::Insert(i) if i.block==name)).count() > 1 {
        return Err(err("Il blocco del collegamento è stato copiato: elimina la copia generica."));
    }
    let mut block = Block::new(&name);
    for k in geometry {
        let mut e = Entity::new(d.new_handle(), k);
        e.common.linetype = "Continuous".into();
        block.entities.push(e);
    }
    d.blocks.insert(name.clone(), Arc::new(block));
    let kind = EntityKind::Insert(Insert {
        block: name,
        insert: Vec3::ZERO,
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: vec![],
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    });
    let h = if let Some(h) = target {
        d.modify_entity(h, |e| e.kind = kind)?;
        h
    } else {
        d.add(&Space::Model, Common { layer: layer.into(), linetype: "Continuous".into(), ..Default::default() }, kind)?
    };
    let points: Vec<_> = path.iter().map(|p| [p.x, p.y]).collect();
    d.modify_entity(h, |e| {
        e.common.layer = layer.into();
        placement::tags(&mut e.common, APP, &json!({"owner":h.hex(),"link":l,"path":points}));
    })?;
    Ok(h)
}
pub(super) fn refresh(d: &mut Drawing, layout: &schema::Layout) -> Result<()> {
    let mut env = Env::new(d, layout)?;
    if env.links.is_empty() {
        return Ok(());
    }
    validate_graph(&env.links)?;
    let mut ready = Vec::new();
    let links = env.links.clone();
    for (h, l) in &links {
        let (g, path) = env.geometry(l)?;
        env.wires.retain(|(owner, _, _)| owner != h);
        env.wires.extend(g.iter().filter_map(|k| if let EntityKind::Line(line) = k { Some((*h, line.a.xy(), line.b.xy())) } else { None }));
        ready.push((*h, l.clone(), g, path));
    }
    let roots: BTreeMap<_, _> = links.iter().map(|(h, l)| net(l, &links).map(|root| (*h, root))).collect::<Result<_>>()?;
    for (index, (h, a, b)) in env.wires.iter().enumerate() {
        for (other, c, e) in env.wires.iter().skip(index + 1) {
            if h != other && routing::overlaps_wire(*a, *b, *c, *e) && roots.get(h) != roots.get(other) {
                return Err(err("Due circuiti avrebbero tratti sovrapposti. Modifica l'altezza del percorso."));
            }
        }
    }
    for (h, l, g, path) in ready {
        store(d, Some(h), &l, g, path)?;
    }
    super::markers::refresh(d, layout)?;
    Ok(())
}
fn selected(s: &Session, p: &Value) -> Result<Handle> {
    if let Some(h) = p.get("handle") {
        return handle(h.as_str().ok_or_else(|| err("Riferimento non valido."))?);
    }
    s.selection().first().copied().filter(|_| s.selection().len() == 1).ok_or_else(|| err("Seleziona un collegamento o una barra."))
}
fn describe(l: &Link, d: &Drawing) -> String {
    let name = |p: &Port| {
        handle(&p.handle)
            .ok()
            .and_then(|h| d.entity(h))
            .and_then(|e| placement::data(e, "CC_UNIFILARE_DEVICE").ok().flatten())
            .map(|m| m["fields"]["sigla"].as_str().filter(|s| !s.is_empty()).or(m["name"].as_str()).unwrap_or("Apparecchio").to_string())
            .unwrap_or_else(|| "Apparecchio da verificare".into())
    };
    let from = l.from.as_ref().map(name).unwrap_or_else(|| "Origine da scegliere".into());
    let to = l.to.as_ref().map(name).unwrap_or_else(|| "Arrivo da scegliere".into());
    match l.kind.as_str() {
        "bus" => format!("Barra comune da {from}"),
        "pe" => format!("{from} → PE"),
        "branch" => format!("Barra comune → {to}"),
        _ => format!("{from} → {to}"),
    }
}
pub fn has_port(d: &Drawing, h: Handle, id: &str) -> bool {
    d.entity(h)
        .and_then(|e| placement::data(e, "CC_UNIFILARE_DEVICE").ok().flatten())
        .and_then(|m| m["ports"].as_array().cloned())
        .is_some_and(|p| p.iter().any(|v| v["id"] == id))
}
pub fn is_bus(d: &Drawing, h: Handle) -> bool {
    read_link(d, h).is_ok_and(|l| l.kind == "bus")
}
fn read(s: &mut Session, p: &Value) -> Result<Value> {
    let h = selected(s, p)?;
    let l = read_link(s.doc()?, h)?;
    Ok(json!({"handle":h.hex(),"description":describe(&l,s.doc()?),"link":l,"uid":s.state()?.uid,"revision":s.state()?.revision}))
}
fn prepare(s: &mut Session, p: &Value) -> Result<Value> {
    if s.running.is_some() {
        return Err(err("Termina il comando corrente."));
    }
    let (layout, _, _) = schema::load(p["model"].as_str().ok_or_else(|| err("Indica il modello unifilare."))?)?;
    let layout = placement::sheet(s, &layout)?;
    let env = Arc::new(Env::new(s.doc()?, &layout)?);
    let target = p.get("handle").map(|v| handle(v.as_str().unwrap_or(""))).transpose()?;
    let mut l = if let Some(h) = target {
        read_link(s.doc()?, h)?
    } else {
        Link {
            version: 1,
            kind: p["kind"].as_str().unwrap_or("series").into(),
            from: None,
            to: None,
            bus: None,
            span: None,
            y: 262.5,
            vertical_x: None,
        }
    };
    if let Some(raw) = p.get("link") {
        if raw.to_string().len() > 4096 {
            return Err(err("Dati troppo grandi."));
        }
        l = serde_json::from_value(raw.clone()).map_err(|_| err("Dati del collegamento non validi."))?;
    }
    if !["series", "bus", "branch", "pe"].contains(&l.kind.as_str())
        || l.version != 1
        || !l.y.is_finite()
        || l.vertical_x.is_some_and(|x| !x.is_finite() || x.abs() > 1000.0)
    {
        return Err(err("Tipo di collegamento non valido."));
    }
    if let Some(h) = target {
        if read_link(s.doc()?, h)?.kind != l.kind {
            return Err(err("Mantieni lo stesso tipo di collegamento durante la modifica."));
        }
        if let Some(uid) = p.get("uid")
            && uid.as_u64() != Some(s.state()?.uid)
        {
            return Err(err("Il disegno è cambiato. Riapri la modifica."));
        }
        let _ = read_link(s.doc()?, h)?;
        editable(s.doc()?, h)?;
    } else if p.get("link").is_none()
        && s.selection().len() == 1
        && let Some(h) = s.selection().first()
    {
        let port = if l.kind == "pe" { "pe" } else { "out" };
        if l.kind == "branch" && env.links.get(h).is_some_and(|v| v.kind == "bus") {
            l.bus = Some(h.hex());
        } else if l.kind == "branch" && env.nodes.get(h).is_some_and(|n| n.ports.contains_key("in")) {
            l.to = Some(Port { handle: h.hex(), port: "in".into() });
        } else if let Some(node) = env.nodes.get(h)
            && node.ports.contains_key(port)
        {
            l.from = Some(Port { handle: h.hex(), port: port.into() });
        }
    }
    let phase = if p.get("link").is_some() || l.kind == "pe" && l.from.is_some() {
        3
    } else if target.is_some() {
        2
    } else if l.kind == "branch" {
        if l.bus.is_some() { 1 } else { 0 }
    } else if l.from.is_none() {
        0
    } else if l.kind == "series" {
        1
    } else {
        2
    };
    let backup_dir = p.get("backup_dir").and_then(Value::as_str).filter(|s| s.len() <= 32768).map(str::to_string);
    s.unifilar_link = Some(Draft {
        link: l,
        phase,
        error: None,
        target,
        uid: s.state()?.uid,
        baseline: s.state()?.doc.clone(),
        layout,
        backup_dir,
        env,
        cache: Arc::new(Mutex::new(None)),
        vertical_anchor: None,
    });
    Ok(json!({"prepared":true}))
}
fn current(s: &Session) -> Result<&Draft> {
    let d = s.unifilar_link.as_ref().ok_or_else(|| err("Avvia un collegamento dal menu Unifilare."))?;
    if s.space() != Space::Model || s.state()?.block_edit.is_some() || s.state()?.uid != d.uid || !Arc::ptr_eq(&s.state()?.doc, &d.baseline) {
        return Err(err("Il disegno è cambiato. Riapri la procedura."));
    }
    Ok(d)
}
fn drawing(d: &Draft, l: &Link) -> Result<Vec<EntityKind>> {
    let key = serde_json::to_string(l).map_err(|e| err(e.to_string()))?;
    if let Ok(cache) = d.cache.lock()
        && let Some((cached, value)) = &*cache
        && cached == &key
    {
        return value.clone().map_err(err);
    }
    let result = (|| {
        let mut links = d.env.links.clone();
        links.insert(d.target.unwrap_or(Handle(0)), l.clone());
        validate_graph(&links)?;
        d.env.geometry(l).map(|v| v.0)
    })();
    if let Ok(mut cache) = d.cache.lock() {
        *cache = Some((
            key,
            result.as_ref().map(Clone::clone).map_err(|e| match e {
                crate::EngineError::BadParams { msg, .. } => msg.clone(),
                _ => e.to_string(),
            }),
        ));
    }
    result
}
fn commit(s: &mut Session) -> Result<Value> {
    let d = current(s)?.clone();
    if d.phase != 3 {
        return Err(err("Completa la selezione e controlla l'anteprima."));
    }
    let _ = drawing(&d, &d.link)?;
    let mut doc = s.doc()?.clone();
    let (g, path) = d.env.geometry(&d.link)?;
    let h = store(&mut doc, d.target, &d.link, g, path)?;
    refresh(&mut doc, &d.layout)?;
    let backup = backup(s.doc()?, d.backup_dir.as_deref())?;
    *s.doc_mut()? = doc;
    s.unifilar_link = None;
    Ok(json!({"handle":h.hex(),"backup":backup}))
}
fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    prepare(s, p)?;
    commit(s)
}
fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    if s.space() != Space::Model || s.state()?.block_edit.is_some() {
        return Err(err("Passa allo spazio modello."));
    }
    if s.running.is_some() {
        return Err(err("Termina il comando corrente."));
    }
    if let Some(uid) = p.get("uid")
        && uid.as_u64() != Some(s.state()?.uid)
    {
        return Err(err("Il disegno è cambiato. Riapri la conferma."));
    }
    let h = selected(s, p)?;
    let l = read_link(s.doc()?, h)?;
    let mut doc = s.doc()?.clone();
    if let Some(original) = p.get("link")
        && *original != serde_json::to_value(&l).map_err(|e| err(e.to_string()))?
    {
        return Err(err("Il collegamento è cambiato. Riapri la conferma."));
    }
    let mut remove = vec![h];
    if l.kind == "bus" {
        for e in doc.model.iter() {
            if let Some(m) = placement::data(e, APP)?
                && m["link"]["bus"] == h.hex()
            {
                remove.push(e.handle);
            }
        }
    }
    if remove.len() > 1 && p["cascade"] != true {
        return Err(err("La barra ha derivazioni: conferma l'eliminazione della barra e delle sue derivazioni."));
    }
    let roots: BTreeSet<_> = remove.iter().map(|h| h.hex()).collect();
    for e in doc.model.iter() {
        if let Some(m) = placement::data(e, "CC_UNIFILARE_DEVICE")?
            && m["kind"] == "line_marker"
            && m["line"].as_str().is_some_and(|v| roots.contains(v))
        {
            remove.push(e.handle);
        }
    }
    for h in &remove {
        editable(&doc, *h)?;
        doc.remove_entity(*h);
    }
    let backup = backup(s.doc()?, p["backup_dir"].as_str())?;
    *s.doc_mut()? = doc;
    Ok(json!({"deleted":remove.len(),"backup":backup}))
}
pub fn snap_port(s: &Session, p: Vec2, tol: f64) -> Option<Vec2> {
    if !s.running.as_ref().is_some_and(|r| r.id == "electrical.unifilar.link.place") {
        return None;
    }
    let d = current(s).ok()?;
    let id = match (d.link.kind.as_str(), d.phase) {
        ("pe", 0) => "pe",
        ("branch", 0) => return None,
        (_, 0) => "out",
        (_, 1) => "in",
        _ => return None,
    };
    d.env
        .nodes
        .values()
        .filter(|n| n.visible)
        .flat_map(|n| n.ports.iter().filter(move |(key, _)| key.as_str() == id).map(|(_, v)| v.0))
        .filter(|q| p.dist(*q) <= tol.max(1.0))
        .min_by(|a, b| p.dist(*a).total_cmp(&p.dist(*b)))
}
fn point(d: &mut Draft, p: Vec2) -> Result<()> {
    if !p.is_finite() {
        return Err(err("Punto non valido."));
    }
    if d.phase == 0 {
        if d.link.kind == "branch" {
            let mut nearest = None;
            let mut distance = 4.0;
            for (h, l) in &d.env.links {
                if d.baseline.entity(*h).is_none_or(|e| !e.common.visible || d.baseline.layer(&e.common.layer).is_some_and(|l| !l.on || l.frozen)) {
                    continue;
                }
                if l.kind == "bus"
                    && let Some(span) = l.span
                {
                    let at = Vec2::new(p.x.clamp(span[0], span[1]), l.y);
                    let dist = p.dist(at);
                    if dist < distance {
                        nearest = Some(h.hex());
                        distance = dist;
                    }
                }
            }
            d.link.bus = Some(nearest.ok_or_else(|| err("Clicca sulla linea orizzontale di una barra comune."))?);
            d.phase = if d.link.to.is_some() { 2 } else { 1 };
        } else {
            d.link.from = Some(d.env.choose(p, if d.link.kind == "pe" { "pe" } else { "out" })?);
            d.phase = if d.link.kind == "pe" {
                3
            } else if d.link.kind == "series" {
                1
            } else {
                2
            };
        }
    } else if d.phase == 1 {
        d.link.to = Some(d.env.choose(p, "in")?);
        d.phase = 2;
    } else if d.phase == 2 {
        if d.link.kind == "bus" {
            if p.x < d.layout.drawing_area[0]
                || p.x > d.layout.drawing_area[2]
                || p.y < d.layout.columns.first().map(|c| c.symbol_y_range[0]).unwrap_or(171.0)
                || p.y > d.layout.drawing_area[3] - 1.0
            {
                return Err(err("Scegli un punto nell'area dei simboli, sopra la fascia PE."));
            }
            d.link.span = Some([p.x, p.x]);
            d.link.y = p.y;
            d.phase = 4;
            d.error = None;
            return Ok(());
        }
        let mut l = d.link.clone();
        l.y = p.y;
        let offset = vertical_default(d)?;
        if offset.is_some() {
            d.link = l;
            d.vertical_anchor = Some(p.x);
            d.phase = 5;
            d.error = None;
            return Ok(());
        }
        let _ = drawing(d, &l)?;
        d.link = l;
        d.phase = 3;
    } else if d.phase == 5 {
        let mut l = d.link.clone();
        l.vertical_x = Some(p.x);
        let _ = drawing(d, &l)?;
        d.link = l;
        d.phase = 3;
    } else if d.phase == 4 {
        let x = d.link.span.ok_or_else(|| err("Scegli la prima estremità della barra."))?[0];
        let mut l = d.link.clone();
        l.span = Some([x.min(p.x), x.max(p.x)]);
        let _ = drawing(d, &l)?;
        d.link = l;
        d.phase = 3;
    }
    if d.phase == 3 {
        let _ = drawing(d, &d.link)?;
    }
    d.error = None;
    Ok(())
}
fn vertical_default(d: &Draft) -> Result<Option<f64>> {
    let b = if let Some(to) = &d.link.to {
        d.env.port(to, "in")?
    } else {
        return Ok(None);
    };
    let a = if d.link.kind == "series" {
        d.env.port(d.link.from.as_ref().ok_or_else(|| err("Scegli l'origine."))?, "out")?
    } else if d.link.kind == "branch" {
        let (bus, span) = d.env.bus(d.link.bus.as_deref().ok_or_else(|| err("Scegli la barra."))?)?;
        Vec2::new(b.x.clamp(span[0], span[1]), bus.y)
    } else {
        return Ok(None);
    };
    Ok(((a.x - b.x).abs() > 1e-7).then_some((a.x + b.x) * 0.5))
}
pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("electrical.unifilar.link.prepare", "Prepare unifilar connection", prepare).noundo(),
        CommandSpec::new("electrical.unifilar.link.insert", "Insert unifilar connection", insert),
        CommandSpec::new("electrical.unifilar.link.read", "Read unifilar connection", read).noundo(),
        CommandSpec::new("electrical.unifilar.link.delete", "Delete unifilar connection", delete),
        CommandSpec::new("electrical.unifilar.link.place", "Place unifilar connection", |_, _| Err(err("Avvia la procedura dal menu Unifilare.")))
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
        "electrical.unifilar.link.place"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let text = s.unifilar_link.as_ref().map(|d| d.error.as_deref().unwrap_or(d.instruction())).unwrap_or("Avvia un collegamento.");
        Prompt::new(text, Accept::POINT).kw(&["Indietro", "Origine", "Destinazione", "Percorso", "Verticale"])
    }
    fn input(&mut self, s: &mut Session, input: Input) -> Result<Step> {
        match input {
            Input::Cancel => {
                s.unifilar_link = None;
                Ok(Step::Cancel)
            }
            Input::Enter if current(s)?.phase == 5 => {
                let mut d = current(s)?.clone();
                let _ = drawing(&d, &d.link)?;
                d.phase = 3;
                d.error = None;
                s.unifilar_link = Some(d);
                Ok(Step::Continue)
            }
            Input::Enter => match commit(s) {
                Ok(r) => {
                    s.echo(format!("Collegamento salvato. Backup: {}", r["backup"].as_str().unwrap_or("")));
                    Ok(Step::Done)
                }
                Err(e) => {
                    if let Some(d) = &mut s.unifilar_link {
                        d.error = Some(match &e {
                            crate::EngineError::BadParams { msg, .. } => msg.clone(),
                            _ => e.to_string(),
                        });
                    }
                    Err(e)
                }
            },
            Input::Keyword(k) => {
                let mut d = current(s)?.clone();
                let key = k.to_lowercase();
                d.phase = match key.as_str() {
                    "origine" => 0,
                    "destinazione" if d.link.kind == "series" || d.link.kind == "branch" => 1,
                    "percorso" => 2,
                    "verticale" if vertical_default(&d)?.is_some() => 5,
                    _ if d.phase == 5 => 2,
                    _ if d.phase == 3 && vertical_default(&d)?.is_some() => 5,
                    _ if d.phase == 4 => 2,
                    _ if d.phase == 3 && d.link.kind == "bus" => 4,
                    _ if d.link.kind == "pe" => 0,
                    _ if d.phase == 2 && d.link.kind == "bus" => 0,
                    _ => d.phase.saturating_sub(1),
                };
                d.error = None;
                d.vertical_anchor = None;
                s.unifilar_link = Some(d);
                Ok(Step::Continue)
            }
            Input::Point(p) => {
                let mut d = current(s)?.clone();
                match point(&mut d, p) {
                    Ok(()) => {
                        s.unifilar_link = Some(d);
                        Ok(Step::Continue)
                    }
                    Err(e) => {
                        d.error = Some(match &e {
                            crate::EngineError::BadParams { msg, .. } => msg.clone(),
                            _ => e.to_string(),
                        });
                        s.unifilar_link = Some(d);
                        Err(e)
                    }
                }
            }
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, cursor: Vec2) -> Vec<EntityKind> {
        let Ok(d) = current(s) else {
            return vec![];
        };
        if d.phase <= 1 {
            let id = if d.phase == 1 {
                "in"
            } else if d.link.kind == "pe" {
                "pe"
            } else {
                "out"
            };
            if d.link.kind == "branch" && d.phase == 0 {
                return d
                    .env
                    .links
                    .values()
                    .filter(|l| l.kind == "bus")
                    .filter_map(|l| l.span.map(|x| line(Vec2::new(x[0], l.y), Vec2::new(x[1], l.y))))
                    .collect();
            }
            return d
                .env
                .nodes
                .values()
                .filter(|n| n.visible)
                .flat_map(|n| {
                    n.ports.iter().filter(move |(key, _)| key.as_str() == id).map(|(_, v)| {
                        EntityKind::Circle(cadcraft_doc::Circle { center: v.0.to3(0.0), radius: if v.0.dist(cursor) < 4.0 { 1.3 } else { 0.8 } })
                    })
                })
                .collect();
        }
        let mut l = d.link.clone();
        if d.phase == 2 {
            if l.kind == "bus" {
                return vec![EntityKind::Circle(cadcraft_doc::Circle { center: cursor.to3(0.0), radius: 0.8 })];
            }
            l.y = cursor.y;
        } else if d.phase == 5 {
            if d.vertical_anchor.is_none_or(|x| (x - cursor.x).abs() > 1e-7) {
                l.vertical_x = Some(cursor.x);
            }
        } else if d.phase == 4
            && let Some(span) = l.span
        {
            l.span = Some([span[0].min(cursor.x), span[0].max(cursor.x)]);
        }
        drawing(d, &l).unwrap_or_default()
    }
}
