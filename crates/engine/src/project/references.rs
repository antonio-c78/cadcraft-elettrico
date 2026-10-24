//! Page continuations are stable, undirected endpoint pairs; labels are derived from sheets.
use super::*;
use crate::{Accept, Input, Interactive, Prompt, Step};
use cadcraft_doc::{Block, Common, Entity, EntityKind, Handle, Insert, Line, RawTag, Space, Text};
use cadcraft_geom::Vec3;
const APP: &str = "CC_PAGE_REFERENCE";

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
        tags(&mut e.common, APP, &json!({"version":1,"pair":id()}));
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
            let pair = m["pair"].as_str().filter(|v| valid_id(v)).ok_or_else(|| error("Identità del rimando non valida."))?;
            let EntityKind::Insert(insert) = &e.kind else { return Err(error("Grafica del rimando non valida.")) };
            let zone = if let Some(g) = st.unifilar_grid {
                format!("col. {:.0}", ((insert.insert.x - g.origin.x) / (g.spacing.x * 2.0)).floor() + 1.0)
            } else {
                String::new()
            };
            out.push(End { sheet: sheet.id.clone(), number: sheet.number.clone(), handle: e.handle.hex(), pair: pair.into(), uid: st.uid, zone });
            if out.len() > 1024 {
                return Err(error("Troppi rimandi nel progetto (massimo 1024)."));
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
        let status = if v.len()==2 && v.first().zip(v.get(1)).is_some_and(|(a,b)|a.sheet!=b.sheet) {"connected"} else if v.len()==1 {"pending"} else {"ambiguous"};
        json!({"id":id,"status":status,"ends":v.iter().map(|e|json!({"sheet":e.sheet,"number":e.number,"handle":e.handle,"zone":e.zone})).collect::<Vec<_>>()})
    }).collect();
    Ok(json!(list))
}
/// Relabel only changed reference blocks; ordinary reads don't dirty a clean project.
pub fn refresh(s: &mut Session) -> Result<()> {
    let all = ends(s)?;
    let Some(p) = &mut s.project else { return Ok(()) };
    for st in s.docs.iter_mut().chain(&mut p.closed) {
        let mut changes = vec![];
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
            let Some(block) = st.doc.block(&i.block) else { continue };
            if block.entities.iter().any(|e| matches!(&e.kind,EntityKind::Text(t) if t.value!=label)) {
                changes.push((i.block.clone(), label));
            }
        }
        if !changes.is_empty() {
            let d = Arc::make_mut(&mut st.doc);
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
    let dx: f64 = if p["direction"] == "left" { -10.0 } else { 10.0 };
    let mut d = s.doc()?.clone();
    d.ensure_layer("UNI_RIMANDI");
    if d.layer("UNI_RIMANDI").is_some_and(|l| l.locked || l.frozen || !l.on) {
        return Err(error("Attiva e sblocca il layer UNI_RIMANDI."));
    }
    let name = format!("CC_RIMANDO_{}", id());
    let mut block = Block::new(&name);
    for (a, b) in [([0.0, 0.0], [dx, 0.0]), ([dx - 2.0 * dx.signum(), 2.0], [dx, 0.0]), ([dx - 2.0 * dx.signum(), -2.0], [dx, 0.0])] {
        block.entities.push(Entity::new(d.new_handle(), EntityKind::Line(Line { a: Vec3::new(a[0], a[1], 0.0), b: Vec3::new(b[0], b[1], 0.0) })));
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
    tags(&mut common, APP, &json!({"version":1,"pair":pair}));
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
    direction: String,
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
                insert(s, &json!({"at":[p.x,p.y],"pair":pair,"direction":self.direction}))?;
                Ok(Step::Done)
            }
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, _: &Session, cursor: cadcraft_geom::Vec2) -> Vec<EntityKind> {
        let dx = if self.direction == "left" { -10.0 } else { 10.0 };
        let a = Vec3::new(cursor.x, cursor.y, 0.0);
        let b = Vec3::new(cursor.x + dx, cursor.y, 0.0);
        vec![
            EntityKind::Line(Line { a, b }),
            EntityKind::Line(Line { a: b, b: Vec3::new(b.x - dx.signum() * 2.0, b.y + 2.0, 0.0) }),
            EntityKind::Line(Line { a: b, b: Vec3::new(b.x - dx.signum() * 2.0, b.y - 2.0, 0.0) }),
        ]
    }
}
pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("electrical.project.reference.prepare", "Prepare page reference", |s, p| {
            let pair = p["pair"].as_str().map(str::to_string);
            if pair.as_ref().is_some_and(|v| !valid_id(v)) {
                return Err(error("Identità del rimando non valida."));
            }
            let project = s.project.as_mut().ok_or_else(|| error("Crea un progetto."))?;
            project.reference_pair = pair;
            project.reference_direction = if p["direction"] == "left" { "left" } else { "right" }.into();
            Ok(json!({"prepared":true}))
        })
        .noundo(),
        CommandSpec::new("electrical.project.reference.insert", "Insert page reference", insert)
            .params("{at:[x,y],pair?:uuid,direction?:left|right}"),
        CommandSpec::new("electrical.project.reference.follow", "Follow page reference", follow).noundo(),
        CommandSpec::new("electrical.project.reference.place", "Place page reference", |_, _| Err(error("Usa la modalità interattiva.")))
            .enabled(|s| if active(s) { Ok(()) } else { Err("Apri un foglio del progetto.".into()) })
            .interactive(|s| {
                if !active(s) {
                    return Err(error("Apri un foglio del progetto."));
                }
                Ok(Box::new(Place {
                    pair: s.project.as_ref().and_then(|p| p.reference_pair.clone()),
                    direction: s.project.as_ref().map(|p| p.reference_direction.clone()).unwrap_or_else(|| "right".into()),
                }))
            }),
    ]
}
