//! Page continuations have an explicit input/output role; geometry follows the attached wire.
use super::*;
use crate::{Accept, Input, Interactive, Prompt, Step};
use cadcraft_doc::{Block, Common, Entity, EntityKind, Handle, Insert, Line, RawTag, Space, Text};
use cadcraft_geom::Vec3;
const APP: &str = "CC_PAGE_REFERENCE";

/// Local geometry leaves the attachment at the origin. Input heads point into the wire;
/// output heads point away. The block insert stays unrotated for the wire-port adapter.
fn arrow(away: Vec3, kind: &str) -> [Line; 3] {
    let far = away * 10.0;
    let (tip, back) = if kind == "input" { (Vec3::ZERO, away) } else { (far, away * -1.0) };
    let normal = Vec3::new(-away.y, away.x, 0.0);
    [Line { a: Vec3::ZERO, b: far }, Line { a: tip + back * 2.0 + normal * 2.0, b: tip }, Line { a: tip + back * 2.0 - normal * 2.0, b: tip }]
}

fn wire_tangent(wire: &Value, at: Vec3) -> Option<Vec3> {
    let path: Vec<[f64; 2]> = serde_json::from_value(wire["path"].clone()).ok()?;
    let near = |p: &[f64; 2]| (p[0] - at.x).hypot(p[1] - at.y) < 1e-6;
    let neighbor = if path.first().is_some_and(near) {
        path.iter().find(|p| !near(p))
    } else if path.last().is_some_and(near) {
        path.iter().rev().find(|p| !near(p))
    } else {
        None
    }?;
    let dx = at.x - neighbor[0];
    let dy = at.y - neighbor[1];
    let length = dx.hypot(dy);
    (length.is_finite() && length > 1e-6).then(|| Vec3::new(dx / length, dy / length, 0.))
}

/// Recompute only derived graphics, using the actual final wire segment, never its draw order.
/// Shared by the project refresh and wire engine, so edits, deletion and undo stay coherent.
type OrientationChanges = Vec<(String, Vec<Entity>)>;
fn orientation_changes(d: &Drawing) -> OrientationChanges {
    if !d.model.iter().any(|e| is_reference(e)) {
        return Vec::new();
    }
    let wires: Vec<_> = d.model.iter().filter_map(|e| data(e, "CC_UNIFILARE_LINK")).collect();
    let mut changes = Vec::new();
    for e in d.model.iter().filter(|e| is_reference(e)) {
        let Some(meta) = data(e, APP) else { continue };
        let EntityKind::Insert(i) = &e.kind else { continue };
        let kind = meta["kind"].as_str().unwrap_or("output");
        let handle = e.handle.hex();
        let tangent_at = |wire: &Value| wire_tangent(wire, i.insert);
        let tangent = wires
            .iter()
            .filter(|wire| wire["link"]["from"]["handle"] == handle || wire["link"]["to"]["handle"] == handle)
            .find_map(tangent_at)
            // A reference placed at a free wire endpoint also follows its local tangent.
            // Never acquire a mere crossing or an internal point of an unrelated wire.
            .or_else(|| wires.iter().find_map(tangent_at));
        let away = tangent.unwrap_or_else(|| Vec3::new(if kind == "input" { -1. } else { 1. }, 0., 0.));
        let lines = arrow(away, kind);
        let far = away * 10.;
        let label_at = if away.y.abs() < 1e-6 { Vec3::new(far.x.min(0.), 3., 0.) } else { Vec3::new(far.x.max(0.) + 3., far.y * 0.5, 0.) };
        let Some(block) = d.block(&i.block) else { continue };
        let mut updates = Vec::new();
        let mut index = 0;
        for entity in block.entities.iter() {
            let mut next = entity.as_ref().clone();
            match &mut next.kind {
                EntityKind::Line(l) => {
                    if let Some(new) = lines.get(index) {
                        *l = new.clone();
                    }
                    index += 1;
                }
                EntityKind::Text(t) => {
                    t.insert = label_at;
                    t.rotation = 0.;
                }
                _ => {}
            }
            if next.kind != entity.kind {
                updates.push(next);
            }
        }
        if !updates.is_empty() {
            changes.push((i.block.clone(), updates));
        }
    }
    changes
}
fn apply_orientation(d: &mut Drawing, changes: OrientationChanges) {
    for (name, updates) in changes {
        if let Some(block) = d.blocks.get_mut(&name) {
            let block = Arc::make_mut(block);
            for e in updates {
                block.entities.replace(e);
            }
        }
    }
}

pub fn orient(d: &mut Drawing) -> Result<bool> {
    let changes = orientation_changes(d);
    let changed = !changes.is_empty();
    apply_orientation(d, changes);
    Ok(changed)
}

pub fn is_reference(e: &Entity) -> bool {
    e.common.xdata.iter().any(|t| t.code == 1001 && t.value == APP)
}
pub fn data(e: &Entity, app: &str) -> Option<Value> {
    let mut active = false;
    let mut out = String::new();
    for t in &e.common.xdata {
        if t.code == 1001 {
            active = t.value == app;
        } else if active && t.code == 1000 {
            if out.len().saturating_add(t.value.len()) > 8192 {
                return None;
            }
            out.push_str(&t.value);
        }
    }
    serde_json::from_str(&out).ok()
}
fn tags(common: &mut Common, app: &str, value: &Value) {
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
    if !chunk.is_empty() {
        common.xdata.push(RawTag { code: 1000, value: chunk });
    }
}
pub fn new_object_id(e: &mut Entity) {
    tags(&mut e.common, "CC_OBJECT_ID", &json!({"id":id()}));
}
pub fn duplicate_entity(e: &mut Entity) {
    new_object_id(e);
    if is_reference(e) {
        let mut m = data(e, APP).unwrap_or_else(|| json!({"version":2}));
        m["pair"] = json!(id());
        tags(&mut e.common, APP, &m);
    }
}

#[derive(Clone)]
struct End {
    sheet: String,
    number: String,
    handle: String,
    pair: String,
    uid: u64,
    zone: String,
    kind: String,
}
fn ends(s: &Session) -> Result<Vec<End>> {
    let Some(p) = &s.project else { return Ok(vec![]) };
    let mut out = vec![];
    for sheet in &p.meta.sheets {
        let Some(st) = s.docs.iter().chain(&p.closed).find(|d| d.uid == sheet.uid) else { return Err(error("Foglio assente.")) };
        for e in st.doc.model.iter() {
            if !is_reference(e) {
                continue;
            }
            let m = data(e, APP).ok_or_else(|| error("Metadati del rimando non validi."))?;
            if !matches!(m["version"].as_u64(), Some(1 | 2))
                || (m["version"] == 2 && !m["kind"].as_str().is_some_and(|v| ["input", "output"].contains(&v)))
            {
                return Err(error("Tipo o versione del rimando non supportati."));
            }
            let pair = m["pair"].as_str().filter(|v| valid_id(v)).ok_or_else(|| error("Identità del rimando non valida."))?;
            let EntityKind::Insert(insert) = &e.kind else { return Err(error("Grafica del rimando non valida.")) };
            let zone = if let Some(g) = st.unifilar_grid {
                format!("col. {:.0}", ((insert.insert.x - g.origin.x) / (g.spacing.x * 2.0)).floor() + 1.0)
            } else {
                String::new()
            };
            out.push(End {
                sheet: sheet.id.clone(),
                number: sheet.number.clone(),
                handle: e.handle.hex(),
                pair: pair.into(),
                uid: st.uid,
                zone,
                kind: m["kind"].as_str().filter(|v| ["input", "output"].contains(v)).unwrap_or("").into(),
            });
            if out.len() > 1024 {
                return Err(error("Troppi rimandi nel progetto (massimo 1024)."));
            }
        }
    }
    // Migrate legacy pairs deterministically; existing explicit roles always win.
    for index in 0..out.len() {
        if out.get(index).is_some_and(|e| e.kind.is_empty()) {
            let pair = out.get(index).map(|e| e.pair.clone()).unwrap_or_default();
            let peer = out.iter().enumerate().find(|(i, e)| *i != index && e.pair == pair && !e.kind.is_empty());
            let kind = if peer.is_some_and(|(_, e)| e.kind == "output") { "input" } else { "output" };
            if let Some(e) = out.get_mut(index) {
                e.kind = kind.into();
            }
        }
    }
    Ok(out)
}
pub fn inspect(s: &Session) -> Result<Value> {
    let all = ends(s)?;
    let mut pairs: BTreeMap<&str, Vec<&End>> = BTreeMap::new();
    for e in &all {
        pairs.entry(&e.pair).or_default().push(e);
    }
    let list: Vec<_> = pairs.into_iter().map(|(id,v)| {
        let status = if v.len()==2 && v.first().zip(v.get(1)).is_some_and(|(a,b)|a.sheet!=b.sheet && a.kind!=b.kind) {"connected"} else if v.len()==1 {"pending"} else {"ambiguous"};
        json!({"id":id,"status":status,"ends":v.iter().map(|e|json!({"sheet":e.sheet,"number":e.number,"handle":e.handle,"zone":e.zone,"kind":e.kind})).collect::<Vec<_>>()})
    }).collect();
    Ok(json!(list))
}
/// Relabel only changed reference blocks; ordinary reads don't dirty a clean project.
pub fn refresh(s: &mut Session) -> Result<()> {
    let all = ends(s)?;
    let Some(p) = &mut s.project else { return Ok(()) };
    for st in s.docs.iter_mut().chain(&mut p.closed) {
        let mut changes = vec![];
        let mut roles = vec![];
        for end in all.iter().filter(|e| e.uid == st.uid) {
            let peers: Vec<_> = all.iter().filter(|e| e.pair == end.pair && (e.sheet != end.sheet || e.handle != end.handle)).collect();
            let label = if peers.len() == 1 && peers.first().is_some_and(|e| e.sheet != end.sheet) {
                let peer = peers.first().ok_or_else(|| error("Destinazione assente."))?;
                format!("{}{}", peer.number, if peer.zone.is_empty() { String::new() } else { format!(" / {}", peer.zone) })
            } else if peers.is_empty() {
                "DA COMPLETARE".into()
            } else {
                "RIMANDO AMBIGUO".into()
            };
            let h = Handle::parse_hex(&end.handle).ok_or_else(|| error("Rimando assente."))?;
            let Some(Entity { kind: EntityKind::Insert(i), .. }) = st.doc.entity(h).map(|e| e.as_ref()) else { continue };
            if st.doc.entity(h).and_then(|e| data(e, APP)).is_some_and(|m| m["kind"] != end.kind || m["version"] != 2) {
                roles.push((h, end.kind.clone()));
            }
            let Some(block) = st.doc.block(&i.block) else { continue };
            if block.entities.iter().any(|e| matches!(&e.kind,EntityKind::Text(t) if t.value!=label)) {
                changes.push((i.block.clone(), label));
            }
        }
        let changed = !changes.is_empty() || !roles.is_empty();
        if changed {
            let d = Arc::make_mut(&mut st.doc);
            for (h, kind) in roles {
                d.modify_entity(h, |e| {
                    if let Some(mut m) = data(e, APP) {
                        m["version"] = json!(2);
                        m["kind"] = json!(kind);
                        tags(&mut e.common, APP, &m);
                    }
                })?;
            }
            for (name, label) in changes {
                if let Some(block) = d.blocks.get_mut(&name) {
                    let block = Arc::make_mut(block);
                    let entities: Vec<_> = block.entities.iter().map(|e| e.as_ref().clone()).collect();
                    for mut e in entities {
                        if let EntityKind::Text(t) = &mut e.kind {
                            t.value = label.clone();
                            block.entities.replace(e);
                        }
                    }
                }
            }
        }
        // Do not detach an unchanged Arc: document dirtiness is based on its identity.
        let graphics = orientation_changes(&st.doc);
        let oriented = !graphics.is_empty();
        if oriented {
            apply_orientation(Arc::make_mut(&mut st.doc), graphics);
        }
        if oriented || changed {
            st.revision = st.revision.saturating_add(1);
        }
    }
    Ok(())
}
fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    if !active(s) || s.space() != Space::Model {
        return Err(error("Apri un foglio del progetto nello spazio modello."));
    }
    let at: [f64; 2] = serde_json::from_value(p["at"].clone()).map_err(error)?;
    if at.iter().any(|v| !v.is_finite() || v.abs() > 1000.0) {
        return Err(error("Punto di inserimento non valido."));
    }
    let pair = p["pair"].as_str().map(str::to_string).unwrap_or_else(id);
    if !valid_id(&pair) {
        return Err(error("Identità del rimando non valida."));
    }
    let uid = s.state()?.uid;
    let peers: Vec<_> = ends(s)?.into_iter().filter(|e| e.pair == pair).collect();
    if peers.len() >= 2 || peers.iter().any(|e| e.uid == uid) {
        return Err(error("Un rimando ammette due estremi su fogli diversi. Completa un rimando libero."));
    }
    let kind = p["kind"].as_str().unwrap_or_else(|| if peers.first().is_some_and(|e| e.kind == "output") { "input" } else { "output" });
    if !["input", "output"].contains(&kind) || peers.iter().any(|e| e.kind == kind) {
        return Err(error("Scegli Ingresso oppure Uscita: una coppia deve avere un rimando di ciascun tipo."));
    }
    let away = if kind == "input" { Vec3::new(-1., 0., 0.) } else { Vec3::new(1., 0., 0.) };
    let mut d = s.doc()?.clone();
    d.ensure_layer("UNI_RIMANDI");
    if d.layer("UNI_RIMANDI").is_some_and(|l| l.locked || l.frozen || !l.on) {
        return Err(error("Attiva e sblocca il layer UNI_RIMANDI."));
    }
    let name = format!("CC_RIMANDO_{}", id());
    let mut block = Block::new(&name);
    for line in arrow(away, kind) {
        block.entities.push(Entity::new(d.new_handle(), EntityKind::Line(line)));
    }
    block.entities.push(Entity::new(
        d.new_handle(),
        EntityKind::Text(Text {
            value: "DA COMPLETARE".into(),
            insert: Vec3::new(2.0, 3.0, 0.0),
            height: 2.0,
            width_factor: 1.0,
            rotation: 0.0,
            oblique: 0.0,
            style: "Standard".into(),
            halign: cadcraft_doc::HAlign::Left,
            valign: cadcraft_doc::VAlign::Baseline,
            align_pt: None,
        }),
    ));
    d.blocks.insert(name.clone(), Arc::new(block));
    let mut common = Common { layer: "UNI_RIMANDI".into(), ..Default::default() };
    tags(&mut common, APP, &json!({"version":2,"pair":pair,"kind":kind}));
    // Adapter to the existing wire engine: a real, unrestricted connection port, not a graphic line.
    tags(
        &mut common,
        "CC_UNIFILARE_DEVICE",
        &json!({"version":1,"model":"reference","symbol":"page_reference","name":"Rimando","kind":"device","column":null,
        "fields":{},"at":at,"ports":[{"id":"in","role":"power","position":[0,0,0]}]}),
    );
    let h = d.new_handle();
    let mut entity = Entity::new(
        h,
        EntityKind::Insert(Insert {
            block: name,
            insert: Vec3::new(at[0], at[1], 0.0),
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: 0.0,
            attribs: vec![],
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
        }),
    );
    entity.common = common;
    new_object_id(&mut entity);
    d.model.push(entity);
    orient(&mut d)?;
    s.state_mut()?.doc = Arc::new(d);
    Ok(json!({"pair":pair,"handle":h.hex(),"uid":uid}))
}
fn follow(s: &mut Session, p: &Value) -> Result<Value> {
    let h =
        p["handle"].as_str().and_then(Handle::parse_hex).or_else(|| s.selection().first().copied()).ok_or_else(|| error("Seleziona un rimando."))?;
    let m = s.doc()?.entity(h).and_then(|e| data(e, APP)).ok_or_else(|| error("L'oggetto non è un rimando."))?;
    let uid = s.state()?.uid;
    let peers: Vec<_> = ends(s)?.into_iter().filter(|e| e.pair == m["pair"].as_str().unwrap_or("") && e.uid != uid).collect();
    if peers.len() != 1 {
        return Err(error("Il rimando è incompleto o ambiguo. Controllalo nel gestore fogli."));
    }
    let peer = peers.first().ok_or_else(|| error("Destinazione assente."))?;
    let result = switch(s, &json!({"id":peer.sheet}))?;
    if let Some(h) = Handle::parse_hex(&peer.handle) {
        s.state_mut()?.selection = vec![h];
    }
    s.execute("zoom.extents", &json!({}))?;
    Ok(result)
}
struct Place {
    pair: Option<String>,
    kind: String,
}
impl Interactive for Place {
    fn name(&self) -> &'static str {
        "RIMANDO"
    }
    fn prompt(&self, _: &Session) -> Prompt {
        Prompt::new("Scegli il punto di connessione del rimando; Esc annulla", Accept::POINT)
    }
    fn input(&mut self, s: &mut Session, input: Input) -> Result<Step> {
        match input {
            Input::Cancel | Input::Enter => Ok(Step::Done),
            Input::Point(p) => {
                let pair = self.pair.clone().unwrap_or_else(id);
                insert(s, &json!({"at":[p.x,p.y],"pair":pair,"kind":self.kind}))?;
                Ok(Step::Done)
            }
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, cursor: cadcraft_geom::Vec2) -> Vec<EntityKind> {
        let origin = Vec3::new(cursor.x, cursor.y, 0.);
        let away = s
            .doc()
            .ok()
            .and_then(|d| d.model.iter().filter_map(|e| data(e, "CC_UNIFILARE_LINK")).find_map(|wire| wire_tangent(&wire, origin)))
            .unwrap_or_else(|| Vec3::new(if self.kind == "input" { -1. } else { 1. }, 0., 0.));
        arrow(away, &self.kind)
            .into_iter()
            .map(|mut l| {
                l.a = l.a + origin;
                l.b = l.b + origin;
                EntityKind::Line(l)
            })
            .collect()
    }
}
pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("electrical.project.reference.prepare", "Prepare page reference", |s, p| {
            let pair = p["pair"].as_str().map(str::to_string);
            if pair.as_ref().is_some_and(|v| !valid_id(v)) {
                return Err(error("Identità del rimando non valida."));
            }
            let kind = p["kind"].as_str().unwrap_or("output");
            if !["input", "output"].contains(&kind) {
                return Err(error("Tipo rimando non valido."));
            }
            let project = s.project.as_mut().ok_or_else(|| error("Crea un progetto."))?;
            project.reference_pair = pair;
            project.reference_kind = kind.into();
            Ok(json!({"prepared":true}))
        })
        .noundo(),
        CommandSpec::new("electrical.project.reference.insert", "Insert page reference", insert).params("{at:[x,y],pair?:uuid,kind?:input|output}"),
        CommandSpec::new("electrical.project.reference.follow", "Follow page reference", follow).noundo(),
        CommandSpec::new("electrical.project.reference.place", "Place page reference", |_, _| Err(error("Usa la modalità interattiva.")))
            .enabled(|s| if active(s) { Ok(()) } else { Err("Apri un foglio del progetto.".into()) })
            .interactive(|s| {
                if !active(s) {
                    return Err(error("Apri un foglio del progetto."));
                }
                Ok(Box::new(Place {
                    pair: s.project.as_ref().and_then(|p| p.reference_pair.clone()),
                    kind: s.project.as_ref().map(|p| p.reference_kind.clone()).unwrap_or_else(|| "output".into()),
                }))
            }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_follow_final_wire_tangent_in_every_direction_independent_of_drawing_order() {
        for flow in [[1., 0.], [-1., 0.], [0., 1.], [0., -1.], [1., 1.], [-1., 1.], [1., -1.], [-1., -1.]] {
            for kind in ["input", "output"] {
                for reverse in [false, true] {
                    let mut s = Session::empty();
                    s.open_drawing(Drawing::new_metric(), "Test", None);
                    s.execute("electrical.project.new", &json!({"name":"Test"})).unwrap();
                    s.execute("electrical.project.add", &json!({})).unwrap();
                    let end = insert(&mut s, &json!({"at":[40.,100.],"kind":kind})).unwrap();
                    let handle = end["handle"].as_str().unwrap();
                    let sign = if kind == "output" { -1. } else { 1. };
                    let neighbor = [40. + flow[0] * 20. * sign, 100. + flow[1] * 20. * sign];
                    // A bent route: only its segment at the attachment may influence the arrow.
                    let mut path = vec![[neighbor[0] + 30., neighbor[1]], neighbor, [40., 100.]];
                    if reverse {
                        path.reverse();
                    }
                    let wire = s
                        .doc_mut()
                        .unwrap()
                        .add(&Space::Model, Common::default(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(1., 0., 0.) }))
                        .unwrap();
                    s.doc_mut()
                        .unwrap()
                        .modify_entity(wire, |e| tags(&mut e.common, "CC_UNIFILARE_LINK", &json!({"link":{"from":{"handle":handle}},"path":path})))
                        .unwrap();
                    let preview = Place { pair: None, kind: kind.into() }.preview(&s, cadcraft_geom::Vec2::new(40., 100.));
                    let EntityKind::Line(a) = &preview[1] else { panic!() };
                    let EntityKind::Line(b) = &preview[2] else { panic!() };
                    let heading = a.b - (a.a + b.a) * 0.5;
                    assert!(heading.x * flow[0] + heading.y * flow[1] > 0., "preview must follow the existing wire endpoint");
                    orient(s.doc_mut().unwrap()).unwrap();
                    let EntityKind::Insert(i) = &s.doc().unwrap().entity(Handle::parse_hex(handle).unwrap()).unwrap().kind else { panic!() };
                    let block = s.doc().unwrap().block(&i.block).unwrap();
                    let lines: Vec<_> =
                        block.entities.iter().filter_map(|e| if let EntityKind::Line(l) = &e.kind { Some(l) } else { None }).collect();
                    let left = lines[1];
                    let right = lines[2];
                    let heading = left.b - (left.a + right.a) * 0.5;
                    assert!(heading.x * flow[0] + heading.y * flow[1] > 0., "{kind} {flow:?} reverse={reverse}");
                    assert_eq!(left.b, right.b);
                    if kind == "input" {
                        assert_eq!(left.b, Vec3::ZERO);
                    }
                    assert!(block.entities.iter().any(|e| matches!(&e.kind, EntityKind::Text(t) if t.rotation==0.)));
                    assert!(!orient(s.doc_mut().unwrap()).unwrap(), "unchanged graphics must not dirty the document");
                }
            }
        }
    }

    #[test]
    fn legacy_pairs_receive_opposite_roles_and_duplicate_preserves_role() {
        let mut s = Session::empty();
        s.open_drawing(Drawing::new_metric(), "A", None);
        s.execute("electrical.project.new", &json!({"name":"Test"})).unwrap();
        s.execute("electrical.project.add", &json!({})).unwrap();
        let a = insert(&mut s, &json!({"at":[20.,100.],"kind":"input"})).unwrap();
        s.open_drawing(Drawing::new_metric(), "B", None);
        s.execute("electrical.project.add", &json!({})).unwrap();
        assert!(insert(&mut s, &json!({"at":[20.,100.],"kind":"input","pair":a["pair"]})).is_err());
        let b = insert(&mut s, &json!({"at":[20.,100.],"pair":a["pair"]})).unwrap();
        let h = Handle::parse_hex(b["handle"].as_str().unwrap()).unwrap();
        let mut copy = s.doc().unwrap().entity(h).unwrap().as_ref().clone();
        duplicate_entity(&mut copy);
        assert_eq!(data(&copy, APP).unwrap()["kind"], "output");
        assert_ne!(data(&copy, APP).unwrap()["pair"], a["pair"]);
        s.doc_mut().unwrap().modify_entity(h, |e| tags(&mut e.common, APP, &json!({"version":1,"pair":a["pair"]}))).unwrap();
        refresh(&mut s).unwrap();
        assert_eq!(data(s.doc().unwrap().entity(h).unwrap(), APP).unwrap()["kind"], "output");
        assert_eq!(inspect(&s).unwrap()[0]["status"], "connected");
    }
}
