//! Electrical placement is provisional until the final confirmation. No catalog is embedded.
use std::sync::Arc;

use cadcraft_doc::{Block, Common, Entity, EntityKind, HAlign, Insert, Line, RawTag, Space, Text, VAlign};
use cadcraft_geom::{Bounds2, Mat3, Vec2, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{CommandSpec, bad};
use crate::{Accept, Input, Interactive, Prompt, Result, Session, Step};

#[derive(Clone, Deserialize, Serialize)]
pub struct Component {
    pub name: String,
    pub block: String,
    pub modules: u8,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Composition {
    pub capacity: u8,
    pub items: Vec<Component>,
    pub factor: f64,
    pub installation_height_cm: Option<f64>,
}

pub struct Placement {
    pub composition: Composition,
    pub point: Option<Vec2>,
    pub rotation: f64,
    /// 0: above, 1: below, 2: right, 3: left, in drawing coordinates.
    pub side: u8,
    /// 0: pick point, 1: rotation, 2: label position.
    pub phase: u8,
    pub error: Option<String>,
    uid: u64,
    geometry: Vec<EntityKind>,
    backup_dir: Option<String>,
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("electrical.prepare", "Prepare electrical box", prepare).noundo(),
        CommandSpec::new("electrical.options", "Electrical box options", options).noundo(),
        CommandSpec::new("electrical.insert", "Insert electrical box", insert).params(
            "{library, composition:{capacity,items:[{name,block,modules}],factor,installation_height_cm},at:[x,y],rotation?,side?:0..3,backup_dir?}",
        ),
        CommandSpec::new("electrical.place", "Insert electrical box", |_, _| {
            Err(bad("electrical.place", "Use the interactive command after electrical.prepare"))
        })
        .enabled(|s| {
            if s.electrical.as_ref().is_some_and(|p| p.phase == 0 && s.state().is_ok_and(|st| st.uid == p.uid)) {
                Ok(())
            } else {
                Err("Prepara una scatola nel compositore.".into())
            }
        })
        .interactive(|s| {
            if !s.electrical.as_ref().is_some_and(|p| p.phase == 0 && s.state().is_ok_and(|st| st.uid == p.uid)) {
                return Err(bad("electrical.place", "Prepare a composition first"));
            }
            Ok(Box::new(Place))
        }),
    ]
}

fn validate(c: &Composition) -> Result<()> {
    if c.items.len() > 6 {
        return Err(bad("electrical.prepare", "Troppi componenti."));
    }
    let occupied: u16 = c.items.iter().map(|i| u16::from(i.modules)).sum();
    if ![3, 4, 6].contains(&c.capacity)
        || c.items.is_empty()
        || c.items.len() > 6
        || occupied > u16::from(c.capacity)
        || c.items.iter().any(|i| i.modules == 0 || i.modules > 6 || i.name.len() > 255 || i.block.len() > 255)
        || !c.factor.is_finite()
        || !(0.000005..=3000.0).contains(&c.factor)
        || c.installation_height_cm.is_some_and(|v| !v.is_finite() || !(0.0..=10000.0).contains(&v))
    {
        return Err(bad("electrical.prepare", "Composizione, dimensioni o altezza non validi."));
    }
    Ok(())
}

fn prepare(s: &mut Session, p: &Value) -> Result<Value> {
    if s.running.is_some() || s.space() != Space::Model {
        return Err(bad("electrical.prepare", "Termina il comando corrente e passa allo spazio modello."));
    }
    let c: Composition =
        serde_json::from_value(p.get("composition").cloned().unwrap_or(Value::Null)).map_err(|e| bad("electrical.prepare", e.to_string()))?;
    validate(&c)?;
    let path = p.get("library").and_then(Value::as_str).ok_or_else(|| bad("electrical.prepare", "Indica la libreria DXF."))?;
    let geometry = read_geometry(path, &c)?;
    let backup_dir = p.get("backup_dir").and_then(Value::as_str).map(str::to_string);
    if backup_dir.as_ref().is_some_and(|p| p.len() > 32768) {
        return Err(bad("electrical.prepare", "Percorso copia di sicurezza troppo lungo."));
    }
    s.electrical =
        Some(Placement { composition: c, point: None, rotation: 0.0, side: 0, phase: 0, error: None, uid: s.state()?.uid, geometry, backup_dir });
    Ok(json!({"prepared": true}))
}

fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    let at = p
        .get("at")
        .and_then(Value::as_array)
        .filter(|a| a.len() == 2)
        .and_then(|a| Some(Vec2::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?)))
        .filter(|p| p.is_finite())
        .ok_or_else(|| bad("electrical.insert", "Indica at: [x,y]."))?;
    prepare(s, p)?;
    let result = (|| {
        options(s, p)?;
        if let Some(placement) = &mut s.electrical {
            placement.point = Some(at);
        }
        commit(s)
    })();
    s.electrical = None;
    result
}

fn options(s: &mut Session, p: &Value) -> Result<Value> {
    let rotation = p.get("rotation").and_then(Value::as_f64);
    let side = p.get("side").and_then(Value::as_u64);
    if rotation.is_some_and(|v| !v.is_finite() || v.abs() > 360000.0) || side.is_some_and(|v| v > 3) {
        return Err(bad("electrical.options", "Rotazione o posizione non valida."));
    }
    let placement = s.electrical.as_mut().ok_or_else(|| bad("electrical.options", "Nessuna scatola preparata."))?;
    if let Some(v) = rotation {
        placement.rotation = v.rem_euclid(360.0);
    }
    if let Some(v) = side {
        placement.side = v as u8;
    }
    Ok(json!({"rotation": placement.rotation, "side": placement.side}))
}

fn line(a: Vec2, b: Vec2) -> EntityKind {
    EntityKind::Line(Line { a: Vec3::new(a.x, a.y, 0.0), b: Vec3::new(b.x, b.y, 0.0) })
}

fn read_geometry(_path: &str, _c: &Composition) -> Result<Vec<EntityKind>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::io::Read;
        if _path.len() > 32768 {
            return Err(bad("electrical.prepare", "Percorso troppo lungo."));
        }
        let f = std::fs::File::open(_path).map_err(|e| bad("electrical.prepare", e.to_string()))?;
        let mut bytes = Vec::new();
        f.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes).map_err(|e| bad("electrical.prepare", e.to_string()))?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err(bad("electrical.prepare", "Limite libreria: 16 MB."));
        }
        let hooks = super::file::io().ok_or_else(|| bad("electrical.prepare", "Lettore DXF non disponibile."))?;
        let drawing = (hooks.read)(&bytes, _path).map_err(|e| bad("electrical.prepare", e))?;
        geometry(&drawing, _c)
    }
    #[cfg(target_arch = "wasm32")]
    Err(bad("electrical.prepare", "La libreria locale richiede la versione desktop."))
}

/// Cache vector strokes, including rendered catalog lettering, so no client block or style
/// can change the appearance. Original catalog identifiers remain in the composition metadata.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn geometry(d: &cadcraft_doc::Drawing, c: &Composition) -> Result<Vec<EntityKind>> {
    let w = f64::from(c.capacity) * 3.0;
    let corners = [Vec2::ZERO, Vec2::new(w, 0.0), Vec2::new(w, 5.0), Vec2::new(0.0, 5.0), Vec2::ZERO];
    let mut out: Vec<_> = corners.windows(2).map(|p| line(p[0], p[1])).collect();
    let mut x = 0.0;
    for item in &c.items {
        if x > 0.0 {
            out.push(line(Vec2::new(x, 0.0), Vec2::new(x, 5.0)));
        }
        let block = d.block(&item.block).ok_or_else(|| bad("electrical.prepare", format!("Simbolo assente: {}", item.name)))?;
        let rough = cadcraft_render::build_entities(d, block.entities.iter().map(|e| e.as_ref()), &Default::default());
        let b = rough.bounds;
        let size = b.width().max(b.height());
        if b.is_empty() || !size.is_finite() || !b.center().is_finite() {
            return Err(bad("electrical.prepare", "Simbolo senza geometria valida."));
        }
        let list = cadcraft_render::build_entities(
            d,
            block.entities.iter().map(|e| e.as_ref()),
            &cadcraft_render::Options { tolerance: (size / 800.0).max(1e-9), ..Default::default() },
        );
        let span = f64::from(item.modules) * 3.0;
        let ratio = ((span - 0.65) / b.width().max(1e-9)).min(3.9 / b.height().max(1e-9));
        let map = |p: Vec2| (p - b.center()) * ratio + Vec2::new(x + span / 2.0, 2.5);
        for primitive in &list.prims {
            let start = primitive.start as usize;
            let end = start.checked_add(primitive.len as usize).ok_or_else(|| bad("electrical.prepare", "Simbolo troppo complesso."))?;
            match primitive.kind {
                cadcraft_render::Kind::Polyline => {
                    if let Some(points) = list.verts.get(start..end) {
                        for pair in points.windows(2) {
                            out.push(line(map(pair[0]), map(pair[1])));
                        }
                    }
                }
                cadcraft_render::Kind::Tris => {
                    if let Some(points) = list.tris.get(start..end) {
                        for t in points.as_chunks::<3>().0 {
                            out.extend([line(map(t[0]), map(t[1])), line(map(t[1]), map(t[2])), line(map(t[2]), map(t[0]))]);
                        }
                    }
                }
                _ => {}
            }
            if out.len() > 20000 {
                return Err(bad("electrical.prepare", "Simbolo troppo complesso."));
            }
        }
        x += span;
    }
    if x < w {
        out.push(line(Vec2::new(x, 0.0), Vec2::new(x, 5.0)));
    }
    for kind in &mut out {
        kind.transform(&Mat3::scale(c.factor, c.factor));
    }
    Ok(out)
}

impl Placement {
    fn kinds(&self, at: Vec2) -> Vec<EntityKind> {
        let mut kinds = self.geometry.clone();
        let rotation = Mat3::rotate(self.rotation.to_radians());
        for kind in &mut kinds {
            kind.transform(&rotation);
        }
        if let Some(h) = self.composition.installation_height_cm {
            let f = self.composition.factor;
            let w = f64::from(self.composition.capacity) * 3.0 * f;
            let points = [Vec2::ZERO, Vec2::new(w, 0.0), Vec2::new(w, 5.0 * f), Vec2::new(0.0, 5.0 * f)].map(|p| rotation.apply(p));
            let bounds = Bounds2::from_points(points);
            let center = bounds.center();
            let (p, halign, valign) = match self.side {
                1 => (Vec2::new(center.x, bounds.min.y - f), HAlign::Center, VAlign::Top),
                2 => (Vec2::new(bounds.max.x + f, center.y), HAlign::Left, VAlign::Middle),
                3 => (Vec2::new(bounds.min.x - f, center.y), HAlign::Right, VAlign::Middle),
                _ => (Vec2::new(center.x, bounds.max.y + f), HAlign::Center, VAlign::Bottom),
            };
            let p = Vec3::new(p.x, p.y, 0.0);
            kinds.push(EntityKind::Text(Text {
                insert: p,
                align_pt: Some(p),
                height: 1.8 * f,
                value: format!("H{h}"),
                rotation: 0.0,
                width_factor: 1.0,
                oblique: 0.0,
                style: "Standard".into(),
                halign,
                valign,
            }));
        }
        for kind in &mut kinds {
            kind.transform(&Mat3::translate(at));
        }
        kinds
    }
}

fn commit(s: &mut Session) -> Result<Value> {
    let p = s.electrical.as_ref().ok_or_else(|| bad("electrical.place", "Composizione mancante."))?;
    if s.state()?.uid != p.uid || s.space() != Space::Model {
        return Err(bad("electrical.place", "Il disegno attivo è cambiato."));
    }
    let at = p.point.ok_or_else(|| bad("electrical.place", "Scegli il punto."))?;
    let mut d = s.doc()?.clone();
    if d.layers.iter().any(|l| l.name.eq_ignore_ascii_case("EL_SCATOLE") && (l.locked || !l.on || l.frozen)) {
        return Err(bad("electrical.place", "Attiva e sblocca il layer EL_SCATOLE."));
    }
    d.ensure_layer("EL_SCATOLE");
    let name = (1..=100000)
        .map(|n| format!("CC_SCATOLA_{n}"))
        .find(|n| d.block(n).is_none())
        .ok_or_else(|| bad("electrical.place", "Troppi blocchi scatola."))?;
    let mut block = Block::new(&name);
    for kind in p.kinds(Vec2::ZERO) {
        let mut entity = Entity::new(d.new_handle(), kind);
        entity.common.color = cadcraft_color::Color::ByBlock;
        block.entities.push(entity);
    }
    d.blocks.insert(name.clone(), Arc::new(block));
    let side = ["sopra", "sotto", "destra", "sinistra"].get(usize::from(p.side)).unwrap_or(&"sopra");
    let metadata = json!({"version":1,"composition":p.composition,"rotation":p.rotation,"installation_label_position":side}).to_string();
    let mut common = Common { layer: "EL_SCATOLE".into(), color: cadcraft_color::Color::Index(7), ..Default::default() };
    common.xdata.push(RawTag { code: 1001, value: "CC_COMPOSITORE".into() });
    let mut chunk = String::new();
    for ch in metadata.chars() {
        if chunk.len() + ch.len_utf8() > 200 {
            common.xdata.push(RawTag { code: 1000, value: std::mem::take(&mut chunk) });
        }
        chunk.push(ch);
    }
    if !chunk.is_empty() {
        common.xdata.push(RawTag { code: 1000, value: chunk });
    }
    let handle = d.add(
        &Space::Model,
        common,
        EntityKind::Insert(Insert {
            block: name.clone(),
            insert: Vec3::new(at.x, at.y, 0.0),
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: 0.0,
            attribs: vec![],
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
        }),
    )?;
    let backup = backup(s.doc()?, p.backup_dir.as_deref())?;
    *s.doc_mut()? = d;
    Ok(json!({"handle":handle,"block":name,"backup":backup}))
}

fn backup(_d: &cadcraft_doc::Drawing, _directory: Option<&str>) -> Result<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::io::Write;
        let root = _directory
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("CADCRAFT_ELECTRICAL_BACKUPS")
                    .map(std::path::PathBuf::from)
                    .or_else(|| std::env::var_os("LOCALAPPDATA").map(|p| std::path::PathBuf::from(p).join("CADCraft Elettrico/backups")))
            })
            .ok_or_else(|| bad("electrical.place", "Cartella copie di sicurezza non configurata."))?;
        std::fs::create_dir_all(&root).map_err(|e| bad("electrical.place", format!("Copia di sicurezza: {e}")))?;
        let hooks = super::file::io().ok_or_else(|| bad("electrical.place", "Scrittore DXF non disponibile."))?;
        let bytes = (hooks.write)(_d, "backup.dxf").map_err(|e| bad("electrical.place", e))?;
        let stamp =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| bad("electrical.place", e.to_string()))?.as_nanos();
        let path = root.join(format!("prima-scatola-{stamp}.dxf"));
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| bad("electrical.place", e.to_string()))?;
        file.write_all(&bytes).and_then(|()| file.sync_all()).map_err(|e| bad("electrical.place", format!("Copia di sicurezza: {e}")))?;
        Ok(path.to_string_lossy().into_owned())
    }
    #[cfg(target_arch = "wasm32")]
    Err(bad("electrical.place", "La copia di sicurezza richiede la versione desktop."))
}

struct Place;
impl Interactive for Place {
    fn name(&self) -> &'static str {
        "electrical.place"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let picking = s.electrical.as_ref().is_some_and(|p| p.phase == 0);
        Prompt::new(
            if picking { "Scegli il punto della scatola" } else { "Completa la finestra di inserimento" },
            if picking { Accept::POINT } else { Accept { enter: true, ..Default::default() } },
        )
    }
    fn input(&mut self, s: &mut Session, input: Input) -> Result<Step> {
        let p = s.electrical.as_mut().ok_or_else(|| bad("electrical.place", "Composizione mancante."))?;
        match input {
            Input::Point(at) if p.phase == 0 && at.is_finite() => {
                p.point = Some(at);
                p.phase = 1;
            }
            Input::Enter if p.phase == 1 && p.composition.installation_height_cm.is_some() => {
                p.phase = 2;
            }
            Input::Enter if p.phase > 0 => match commit(s) {
                Ok(result) => {
                    s.echo(format!("Scatola inserita. Copia di sicurezza: {}", result["backup"].as_str().unwrap_or("")));
                    s.electrical = None;
                    return Ok(Step::Done);
                }
                Err(e) => {
                    if let Some(p) = &mut s.electrical {
                        p.error = Some(e.to_string());
                    }
                    return Err(e);
                }
            },
            _ => {}
        }
        Ok(Step::Continue)
    }
    fn preview(&self, s: &Session, cursor: Vec2) -> Vec<EntityKind> {
        s.electrical.as_ref().filter(|p| s.state().is_ok_and(|st| st.uid == p.uid)).map(|p| p.kinds(p.point.unwrap_or(cursor))).unwrap_or_default()
    }
}
