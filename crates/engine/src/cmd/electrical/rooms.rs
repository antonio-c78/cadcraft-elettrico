//! Named room boundaries, kept with their labels as one native block reference.
use super::*;
use cadcraft_doc::{Drawing, Handle};
use cadcraft_geom::PolyVertex;

fn perimeter(points: &[Vec2], closed: bool) -> EntityKind {
    crate::cmd::helpers::lwpoly(points.iter().copied().map(PolyVertex::new).collect(), closed)
}

const LAYER: &str = "EL_AMBIENTI";
const APP: &str = "CC_AMBIENTE";
const REF: &str = "CC_AMBIENTE_REF";

#[derive(Clone)]
pub struct Draft {
    pub points: Vec<Vec2>,
    pub name: String,
    pub floor: String,
    pub label_height: f64,
    pub naming: bool,
    uid: u64,
    revision: u64,
    target: Option<Handle>,
    backup_dir: Option<String>,
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("electrical.room.prepare", "Prepare electrical room", prepare).noundo().params("{handle?,label_height?,backup_dir?}"),
        CommandSpec::new("electrical.room.options", "Electrical room options", options)
            .noundo()
            .params("{name?,floor?,label_height?,action?:undo|close|redraw}"),
        CommandSpec::new("electrical.room.place", "Define electrical room", |_, _| Err(bad("electrical.room.place", "Prepara l'ambiente.")))
            .enabled(|s| {
                if s.electrical_room.as_ref().is_some_and(|draft| current(s, draft).is_ok()) { Ok(()) } else { Err("Prepara l'ambiente.".into()) }
            })
            .interactive(|s| {
                let draft = s.electrical_room.as_ref().ok_or_else(|| bad("electrical.room.place", "Prepara l'ambiente."))?;
                current(s, draft)?;
                Ok(Box::new(Place))
            }),
        CommandSpec::new("electrical.room.create", "Create electrical room", create)
            .params("{points:[[x,y],...],name,floor?,label_height?,backup_dir?}"),
        CommandSpec::new("electrical.room.list", "List electrical rooms", list).noundo(),
        CommandSpec::new("electrical.room.scan", "Read electrical room assignments", scan).noundo(),
        CommandSpec::new("electrical.room.assign", "Assign electrical components to room", assign).params("{room:handle|null,handles?,backup_dir?}"),
        CommandSpec::new("electrical.room.delete", "Delete electrical room", delete).params("{handle,backup_dir?}"),
    ]
}

fn current(s: &Session, d: &Draft) -> Result<()> {
    if s.space() != Space::Model || s.state()?.uid != d.uid || s.state()?.revision != d.revision {
        return Err(bad("electrical.room.place", "Il disegno è cambiato. Riapri l'ambiente."));
    }
    Ok(())
}

fn handle(v: &Value) -> Option<Handle> {
    v.as_str().and_then(Handle::parse_hex).or_else(|| v.as_u64().map(Handle))
}

fn data(e: &Entity, app: &str) -> Result<Option<Value>> {
    let mut active = false;
    let mut text = String::new();
    for tag in &e.common.xdata {
        if tag.code == 1001 {
            active = tag.value == app;
        } else if active && tag.code == 1000 {
            if text.len().saturating_add(tag.value.len()) > 4096 {
                return Err(bad("electrical.room.list", "Dati ambiente troppo grandi."));
            }
            text.push_str(&tag.value);
        }
    }
    if text.is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&text).map(Some).map_err(|_| bad("electrical.room.list", "Dati ambiente non validi."))
}

fn tags(common: &mut Common, app: &str, value: Value) {
    let mut active = false;
    common.xdata.retain(|t| {
        if t.code == 1001 {
            active = t.value == app;
        }
        !active
    });
    common.xdata.push(RawTag { code: 1001, value: app.into() });
    let mut chunk = String::new();
    for ch in value.to_string().chars() {
        if chunk.len() + ch.len_utf8() > 200 {
            common.xdata.push(RawTag { code: 1000, value: std::mem::take(&mut chunk) });
        }
        chunk.push(ch);
    }
    common.xdata.push(RawTag { code: 1000, value: chunk });
}

fn edges(points: &[Vec2]) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
    points.iter().copied().zip(points.iter().copied().cycle().skip(1)).take(points.len())
}
fn tolerance(points: &[Vec2]) -> f64 {
    let b = Bounds2::from_points(points.iter().copied());
    b.width().max(b.height()).max(1.0) * 1e-9
}
fn cross(a: Vec2, b: Vec2, c: Vec2) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}
fn on_edge(p: Vec2, a: Vec2, b: Vec2, eps: f64) -> bool {
    cross(a, b, p).abs() <= eps * a.dist(b).max(eps)
        && p.x >= a.x.min(b.x) - eps
        && p.x <= a.x.max(b.x) + eps
        && p.y >= a.y.min(b.y) - eps
        && p.y <= a.y.max(b.y) + eps
}
fn intersects(a: Vec2, b: Vec2, c: Vec2, d: Vec2, eps: f64) -> bool {
    if on_edge(a, c, d, eps) || on_edge(b, c, d, eps) || on_edge(c, a, b, eps) || on_edge(d, a, b, eps) {
        return true;
    }
    (cross(a, b, c) > 0.0) != (cross(a, b, d) > 0.0) && (cross(c, d, a) > 0.0) != (cross(c, d, b) > 0.0)
}
fn area(points: &[Vec2]) -> f64 {
    let origin = points.first().copied().unwrap_or(Vec2::ZERO);
    edges(points).map(|(a, b)| cross(origin, a, b)).sum::<f64>().abs() / 2.0
}
fn validate(points: &[Vec2]) -> Result<()> {
    if !(3..=512).contains(&points.len()) || points.iter().any(|p| !p.is_finite() || p.x.abs() > 1e12 || p.y.abs() > 1e12) {
        return Err(bad("electrical.room.place", "Servono da 3 a 512 vertici con coordinate valide."));
    }
    let eps = tolerance(points);
    if edges(points).any(|(a, b)| a.dist(b) <= eps) || area(points) <= eps * eps {
        return Err(bad("electrical.room.place", "Il perimetro contiene lati nulli o non racchiude un'area."));
    }
    for (i, (a, b)) in edges(points).enumerate() {
        let c = points.get((i + 2) % points.len()).copied().ok_or_else(|| bad("electrical.room.place", "Vertice assente."))?;
        if on_edge(c, a, b, eps) || on_edge(a, b, c, eps) {
            return Err(bad("electrical.room.place", "Il perimetro ripercorre lo stesso lato."));
        }
        for (j, (c, d)) in edges(points).enumerate().skip(i + 1) {
            if j == i + 1 || (i == 0 && j == points.len() - 1) {
                continue;
            }
            if intersects(a, b, c, d, eps) {
                return Err(bad("electrical.room.place", "Il perimetro si incrocia o si tocca. Correggi i vertici."));
            }
        }
    }
    Ok(())
}
// 0 outside, 1 inside, 2 on boundary. The distinction avoids silently guessing a room.
fn contains(points: &[Vec2], p: Vec2) -> u8 {
    let eps = tolerance(points);
    let mut inside = false;
    for (a, b) in edges(points) {
        if on_edge(p, a, b, eps) {
            return 2;
        }
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    u8::from(inside)
}

fn label_point(points: &[Vec2]) -> Vec2 {
    let center = Bounds2::from_points(points.iter().copied()).center();
    if contains(points, center) == 1 {
        return center;
    }
    let mut ys: Vec<_> = points.iter().map(|p| p.y).collect();
    ys.sort_by(f64::total_cmp);
    ys.dedup();
    let mut best = (0.0, center);
    for pair in ys.windows(2) {
        let y = (pair[0] + pair[1]) / 2.0;
        let mut xs: Vec<_> =
            edges(points).filter(|(a, b)| (a.y > y) != (b.y > y)).map(|(a, b)| a.x + (b.x - a.x) * (y - a.y) / (b.y - a.y)).collect();
        xs.sort_by(f64::total_cmp);
        for pair in xs.as_chunks::<2>().0 {
            let width = pair[1] - pair[0];
            if width > best.0 {
                best = (width, Vec2::new((pair[0] + pair[1]) / 2.0, y));
            }
        }
    }
    best.1
}

fn read(s: &Session, h: Handle) -> Result<Draft> {
    let e = s.doc()?.entity(h).ok_or_else(|| bad("electrical.room.list", "Ambiente assente."))?;
    if s.doc()?.space_of(h) != Some(Space::Model) {
        return Err(bad("electrical.room.list", "L'ambiente deve essere nello spazio modello."));
    }
    let m = data(e, APP)?.ok_or_else(|| bad("electrical.room.list", "Seleziona un ambiente elettrico."))?;
    let EntityKind::Insert(ins) = &e.kind else {
        return Err(bad("electrical.room.list", "Ambiente non valido."));
    };
    if m["version"] != 1
        || ins.insert.z != 0.0
        || !ins.attribs.is_empty()
        || ins.rows != 1
        || ins.cols != 1
        || !ins.insert.is_finite()
        || !ins.rotation.is_finite()
        || !ins.scale.is_finite()
        || ins.scale.x <= 0.0
        || ins.scale.x != ins.scale.y
        || ins.rotation.abs() > 1e6
    {
        return Err(bad("electrical.room.list", "Trasformazione ambiente non supportata."));
    }
    let block = s.doc()?.block(&ins.block).ok_or_else(|| bad("electrical.room.list", "Blocco ambiente assente."))?;
    if block.base != Vec3::ZERO || block.entities.len() != 2 {
        return Err(bad("electrical.room.list", "Blocco ambiente modificato manualmente."));
    }
    let poly = block
        .entities
        .iter()
        .find_map(|e| if let EntityKind::LwPolyline(p) = &e.kind { Some(p) } else { None })
        .ok_or_else(|| bad("electrical.room.list", "Perimetro assente."))?;
    if !poly.closed || poly.elevation != 0.0 || poly.vertices.len() > 512 || poly.vertices.iter().any(|v| v.bulge != 0.0) {
        return Err(bad("electrical.room.list", "Usa un perimetro chiuso a segmenti rettilinei."));
    }
    let (sin, cos) = ins.rotation.sin_cos();
    let points = poly
        .vertices
        .iter()
        .map(|v| {
            let q = v.p * ins.scale.x;
            Vec2::new(ins.insert.x + q.x * cos - q.y * sin, ins.insert.y + q.x * sin + q.y * cos)
        })
        .collect::<Vec<_>>();
    validate(&points)?;
    let draft = Draft {
        points,
        name: m["name"].as_str().unwrap_or("").into(),
        floor: m["floor"].as_str().unwrap_or("").into(),
        label_height: m["label_height"].as_f64().unwrap_or(100.0) * ins.scale.x,
        naming: true,
        uid: s.state()?.uid,
        revision: s.state()?.revision,
        target: Some(h),
        backup_dir: None,
    };
    fields(&draft)?;
    Ok(draft)
}

fn fields(d: &Draft) -> Result<()> {
    if d.name.trim().is_empty()
        || d.name.len() > 160
        || d.floor.len() > 80
        || d.name.chars().chain(d.floor.chars()).any(char::is_control)
        || !d.label_height.is_finite()
        || !(0.00001..=1e6).contains(&d.label_height)
    {
        return Err(bad("electrical.room.place", "Indica il nome del locale e un'altezza testo valida."));
    }
    Ok(())
}
fn prepare(s: &mut Session, p: &Value) -> Result<Value> {
    if s.running.is_some() || s.space() != Space::Model {
        return Err(bad("electrical.room.prepare", "Termina il comando e passa allo spazio modello."));
    }
    if s.doc()?.layer(LAYER).is_some_and(|l| l.locked || l.frozen || !l.on) {
        return Err(bad("electrical.room.prepare", "Attiva e sblocca EL_AMBIENTI prima di disegnare il perimetro."));
    }
    let mut draft = if let Some(v) = p.get("handle") {
        read(s, handle(v).ok_or_else(|| bad("electrical.room.prepare", "Identificativo ambiente non valido."))?)?
    } else {
        Draft {
            points: Vec::new(),
            name: String::new(),
            floor: String::new(),
            label_height: 100.0,
            naming: false,
            uid: s.state()?.uid,
            revision: s.state()?.revision,
            target: None,
            backup_dir: None,
        }
    };
    if let Some(v) = p.get("label_height") {
        draft.label_height = v
            .as_f64()
            .filter(|n| n.is_finite() && (0.00001..=1e6).contains(n))
            .ok_or_else(|| bad("electrical.room.prepare", "Altezza testo non valida."))?;
    }
    draft.backup_dir = p.get("backup_dir").and_then(Value::as_str).map(str::to_string);
    if draft.backup_dir.as_ref().is_some_and(|v| v.len() > 32768) {
        return Err(bad("electrical.room.prepare", "Percorso backup troppo lungo."));
    }
    s.electrical_room = Some(draft);
    Ok(json!({"prepared":true}))
}
fn options(s: &mut Session, p: &Value) -> Result<Value> {
    let mut draft = s.electrical_room.clone().ok_or_else(|| bad("electrical.room.options", "Prepara un ambiente."))?;
    current(s, &draft)?;
    if let Some(v) = p.get("name") {
        draft.name = v.as_str().ok_or_else(|| bad("electrical.room.options", "Nome non valido."))?.into();
    }
    if let Some(v) = p.get("floor") {
        draft.floor = v.as_str().ok_or_else(|| bad("electrical.room.options", "Piano non valido."))?.into();
    }
    if let Some(v) = p.get("label_height") {
        draft.label_height = v.as_f64().ok_or_else(|| bad("electrical.room.options", "Altezza testo non valida."))?;
    }
    if draft.name.len() > 160 || draft.floor.len() > 80 || !draft.label_height.is_finite() || !(0.00001..=1e6).contains(&draft.label_height) {
        return Err(bad("electrical.room.options", "Dati ambiente non validi."));
    }
    match p.get("action").and_then(Value::as_str) {
        Some("undo") => {
            draft.points.pop();
            draft.naming = false;
        }
        Some("close") => {
            validate(&draft.points)?;
            draft.naming = true;
        }
        Some("redraw") => {
            draft.points.clear();
            draft.naming = false;
        }
        None => {}
        _ => return Err(bad("electrical.room.options", "Azione non valida.")),
    }
    s.electrical_room = Some(draft);
    Ok(json!({"updated":true}))
}

fn editable(d: &Drawing, h: Handle) -> Result<()> {
    let e = d.entity(h).ok_or_else(|| bad("electrical.room.place", "Oggetto assente."))?;
    if d.layer(&e.common.layer).is_some_and(|l| l.locked || l.frozen || !l.on) {
        return Err(bad("electrical.room.place", "Attiva e sblocca il layer dell'oggetto."));
    }
    Ok(())
}
fn commit(s: &mut Session) -> Result<Value> {
    let draft = s.electrical_room.as_ref().ok_or_else(|| bad("electrical.room.place", "Prepara un ambiente."))?;
    current(s, draft)?;
    validate(&draft.points)?;
    fields(draft)?;
    let mut d = s.doc()?.clone();
    if draft.target.is_none() && d.model.iter().filter(|e| e.common.xdata.iter().any(|t| t.code == 1001 && t.value == APP)).count() >= 512 {
        return Err(bad("electrical.room.place", "Limite di 512 ambienti per disegno."));
    }
    if d.layer(LAYER).is_some_and(|l| l.locked || l.frozen || !l.on) {
        return Err(bad("electrical.room.place", "Attiva e sblocca EL_AMBIENTI."));
    }
    if let Some(h) = draft.target {
        editable(&d, h)?;
    }
    if d.ensure_layer(LAYER)
        && let Some(layer) = d.layer_mut(LAYER)
    {
        layer.plot = false;
        layer.color = cadcraft_color::Color::Index(3);
    }
    let name = (1..=100000)
        .map(|n| format!("CC_AMBIENTE_{n}"))
        .find(|name| d.block(name).is_none())
        .ok_or_else(|| bad("electrical.room.place", "Troppi blocchi ambiente."))?;
    let mut block = Block::new(&name);
    block.entities.push(Entity::new(d.new_handle(), perimeter(&draft.points, true)));
    let p = label_point(&draft.points);
    let p = Vec3::new(p.x, p.y, 0.0);
    block.entities.push(Entity::new(
        d.new_handle(),
        EntityKind::Text(Text {
            insert: p,
            align_pt: Some(p),
            height: draft.label_height,
            value: if draft.floor.trim().is_empty() { draft.name.trim().into() } else { format!("{} / {}", draft.name.trim(), draft.floor.trim()) },
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: "Standard".into(),
            halign: HAlign::Center,
            valign: VAlign::Middle,
        }),
    ));
    d.blocks.insert(name.clone(), Arc::new(block));
    let mut common = Common { layer: LAYER.into(), ..Default::default() };
    tags(&mut common, APP, json!({"version":1,"name":draft.name.trim(),"floor":draft.floor.trim(),"label_height":draft.label_height}));
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
    let h = if let Some(h) = draft.target {
        d.modify_entity(h, |e| {
            e.kind = kind.clone();
            e.common.layer = LAYER.into();
            tags(&mut e.common, APP, json!({"version":1,"name":draft.name.trim(),"floor":draft.floor.trim(),"label_height":draft.label_height}));
        })?;
        h
    } else {
        d.add(&Space::Model, common, kind)?
    };
    let path = backup(s.doc()?, draft.backup_dir.as_deref())?;
    *s.doc_mut()? = d;
    Ok(json!({"handle":h.hex(),"backup":path}))
}
fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let raw = p
        .get("points")
        .and_then(Value::as_array)
        .filter(|a| a.len() <= 512)
        .ok_or_else(|| bad("electrical.room.create", "Indica i vertici del perimetro."))?;
    let points = raw
        .iter()
        .map(|v| {
            v.as_array()
                .filter(|a| a.len() == 2)
                .and_then(|a| Some(Vec2::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?)))
                .ok_or_else(|| bad("electrical.room.create", "Vertice non valido."))
        })
        .collect::<Result<Vec<_>>>()?;
    validate(&points)?;
    prepare(s, p)?;
    let result = (|| {
        options(s, p)?;
        if let Some(d) = &mut s.electrical_room {
            d.points = points;
        }
        commit(s)
    })();
    s.electrical_room = None;
    result
}

fn rooms(s: &Session) -> Result<Vec<(Handle, Draft)>> {
    let mut out = Vec::new();
    for e in s.doc()?.model.iter() {
        if e.common.xdata.iter().any(|t| t.code == 1001 && t.value == APP) {
            if out.len() >= 512 {
                return Err(bad("electrical.room.list", "Limite di 512 ambienti per disegno."));
            }
            out.push((e.handle, read(s, e.handle)?));
        }
    }
    Ok(out)
}
fn list(s: &mut Session, _: &Value) -> Result<Value> {
    Ok(
        json!({"rooms":rooms(s)?.iter().map(|(h,r)|json!({"handle":h.hex(),"name":r.name,"floor":r.floor,"vertices":r.points.len(),"area_model_units":area(&r.points)})).collect::<Vec<_>>()}),
    )
}
fn electrical(e: &Entity) -> bool {
    e.common.xdata.iter().any(|t| t.code == 1001 && matches!(t.value.as_str(), "CC_COMPOSITORE" | "CC_SIMBOLO"))
}
fn assign(s: &mut Session, p: &Value) -> Result<Value> {
    if s.running.is_some() || s.space() != Space::Model {
        return Err(bad("electrical.room.assign", "Termina il comando e passa allo spazio modello."));
    }
    let room = p.get("room").ok_or_else(|| bad("electrical.room.assign", "Indica l'ambiente o null per tornare all'assegnazione automatica."))?;
    if !room.is_null() {
        read(s, handle(room).ok_or_else(|| bad("electrical.room.assign", "Ambiente non valido."))?)?;
    }
    let hs = crate::cmd::targets(s, p)?;
    if hs.is_empty() || hs.len() > 20000 {
        return Err(bad("electrical.room.assign", "Seleziona i componenti elettrici da assegnare."));
    }
    let mut d = s.doc()?.clone();
    for h in &hs {
        editable(&d, *h)?;
        let e = d.entity(*h).ok_or_else(|| bad("electrical.room.assign", "Componente assente."))?;
        if !electrical(e) || d.space_of(*h) != Some(Space::Model) {
            return Err(bad("electrical.room.assign", "La selezione deve contenere solo scatole e simboli elettrici nativi."));
        }
    }
    for h in &hs {
        d.modify_entity(*h, |e| tags(&mut e.common, REF, json!({"room":handle(room).map(|h|h.hex())})))?;
    }
    let path = backup(s.doc()?, p.get("backup_dir").and_then(Value::as_str))?;
    *s.doc_mut()? = d;
    Ok(json!({"assigned":hs.len(),"backup":path}))
}
fn scan(s: &mut Session, _: &Value) -> Result<Value> {
    let rooms = rooms(s)?;
    let mut out = Vec::new();
    let mut work = 0usize;
    for e in s.doc()?.model.iter().filter(|e| electrical(e)) {
        if out.len() >= 20000 {
            return Err(bad("electrical.room.scan", "Troppi componenti da analizzare."));
        }
        let EntityKind::Insert(ins) = &e.kind else { continue };
        let manual = data(e, REF)?.and_then(|v| handle(&v["room"]));
        let (room, status) = if let Some(h) = manual {
            (Some(h), if rooms.iter().any(|(r, _)| *r == h) { "manual" } else { "missing_room" })
        } else {
            let mut hits = Vec::new();
            let mut boundary = false;
            for (h, r) in &rooms {
                work = work.saturating_add(r.points.len());
                if work > 20000000 {
                    return Err(bad("electrical.room.scan", "Analisi troppo complessa: riduci i vertici dei perimetri."));
                }
                match contains(&r.points, Vec2::new(ins.insert.x, ins.insert.y)) {
                    1 => hits.push(*h),
                    2 => boundary = true,
                    _ => {}
                }
            }
            if boundary {
                (None, "boundary")
            } else if hits.len() == 1 {
                (hits.first().copied(), "automatic")
            } else {
                (None, if hits.is_empty() { "outside" } else { "overlap" })
            }
        };
        out.push(json!({"handle":e.handle.hex(),"room":room.map(|h|h.hex()),"status":status}));
    }
    Ok(json!({"components":out}))
}
fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    if s.running.is_some() || s.space() != Space::Model {
        return Err(bad("electrical.room.delete", "Termina il comando e passa allo spazio modello."));
    }
    let h = p.get("handle").and_then(handle).ok_or_else(|| bad("electrical.room.delete", "Seleziona un ambiente."))?;
    read(s, h)?;
    editable(s.doc()?, h)?;
    let mut d = s.doc()?.clone();
    d.remove_entity(h);
    let path = backup(s.doc()?, p.get("backup_dir").and_then(Value::as_str))?;
    *s.doc_mut()? = d;
    Ok(json!({"deleted":h.hex(),"backup":path}))
}

struct Place;
impl Interactive for Place {
    fn name(&self) -> &'static str {
        "electrical.room.place"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let naming = s.electrical_room.as_ref().is_some_and(|d| d.naming);
        Prompt::new(
            if naming { "Completa nome e piano nella finestra" } else { "Scegli i vertici; Invio chiude il perimetro" },
            if naming { Accept { enter: true, ..Default::default() } } else { Accept { point: true, enter: true, ..Default::default() } },
        )
    }
    fn input(&mut self, s: &mut Session, input: Input) -> Result<Step> {
        let d = s.electrical_room.as_ref().ok_or_else(|| bad("electrical.room.place", "Ambiente assente."))?;
        current(s, d)?;
        match input {
            Input::Point(p) if !d.naming => {
                if !p.is_finite() || p.x.abs() > 1e12 || p.y.abs() > 1e12 || d.points.len() >= 512 {
                    return Err(bad("electrical.room.place", "Vertice non valido o limite di 512 vertici raggiunto."));
                }
                if d.points.first().is_some_and(|q| q.dist(p) <= tolerance(&d.points)) && d.points.len() >= 2 {
                    options(s, &json!({"action":"close"}))?;
                } else if let Some(d) = &mut s.electrical_room
                    && d.points.last().is_none_or(|q| q.dist(p) > 1e-9)
                {
                    d.points.push(p);
                }
            }
            Input::Enter if !d.naming => {
                options(s, &json!({"action":"close"}))?;
            }
            Input::Enter => {
                let result = commit(s)?;
                s.echo(format!("Ambiente salvato. Copia di sicurezza: {}", result["backup"].as_str().unwrap_or("")));
                s.electrical_room = None;
                return Ok(Step::Done);
            }
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, s: &Session, cursor: Vec2) -> Vec<EntityKind> {
        let Some(d) = &s.electrical_room else { return vec![] };
        if current(s, d).is_err() {
            return vec![];
        }
        let mut points = d.points.clone();
        if !d.naming && cursor.is_finite() {
            points.push(cursor);
        }
        vec![perimeter(&points, d.naming)]
    }
}
