//! A catalog symbol is edited in an isolated drawing. One catalog replacement publishes a revision.
use super::*;
use cadcraft_doc::{Drawing, Point};
use std::path::{Path, PathBuf};
const CMD: &str = "electrical.library.open";
const MARK: &str = "CC_SYMBOL_EDITOR_POINT";
const MAX_FILE: u64 = 32 * 1024 * 1024;

#[derive(Clone)]
pub struct Draft {
    uid: u64,
    project: Option<u64>,
    path: PathBuf,
    original: Vec<u8>,
    library_bytes: Vec<u8>,
    library_path: PathBuf,
    catalog: Value,
    index: usize,
    list: &'static str,
    source: Drawing,
    id: String,
    name: String,
    point: String,
}
pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new(CMD, "Edit catalog symbol in new drawing", open).noundo(),
        CommandSpec::new("electrical.library.read", "Read catalog symbol editor", inspect).noundo(),
        CommandSpec::new("electrical.library.point", "Set catalog symbol connection point", set_point)
            .enabled(|s| current(s).map(|_| ()).map_err(|e| e.to_string()))
            .interactive(|s| {
                let id = current(s)?.point.clone();
                Ok(Box::new(PickPoint { id }))
            }),
        CommandSpec::new("electrical.library.choose", "Choose catalog symbol connection point", choose).noundo(),
        CommandSpec::new("electrical.library.save", "Save catalog symbol and return", save).noundo(),
        CommandSpec::new("electrical.library.cancel", "Cancel catalog symbol editor", cancel).noundo(),
    ]
}
fn error(msg: impl Into<String>) -> crate::EngineError {
    bad(CMD, msg)
}
#[cfg(not(target_arch = "wasm32"))]
fn read(path: &Path, limit: u64) -> Result<Vec<u8>> {
    use std::io::Read;
    let f = std::fs::File::open(path).map_err(|e| error(e.to_string()))?;
    let mut bytes = Vec::new();
    f.take(limit + 1).read_to_end(&mut bytes).map_err(|e| error(e.to_string()))?;
    if bytes.len() as u64 > limit {
        return Err(error("File troppo grande."));
    }
    Ok(bytes)
}
#[cfg(target_arch = "wasm32")]
fn read(_: &Path, _: u64) -> Result<Vec<u8>> {
    Err(error("L'editor catalogo richiede la versione desktop."))
}
fn local(parent: &Path, name: &str) -> Result<PathBuf> {
    let p = Path::new(name);
    if p.is_absolute() || p.components().any(|c| !matches!(c, std::path::Component::Normal(_))) || name.len() > 32768 {
        return Err(error("La libreria deve trovarsi nella cartella del catalogo o in una sua sottocartella."));
    }
    Ok(parent.join(p))
}
fn role(e: &Entity) -> Option<&str> {
    e.common.xdata.windows(2).find(|t| t[0].code == 1001 && t[0].value == MARK && t[1].code == 1000).map(|t| t[1].value.as_str())
}
fn points(d: &Drawing) -> Result<serde_json::Map<String, Value>> {
    let mut out = serde_json::Map::new();
    for e in d.model.iter() {
        if let Some(id) = role(e) {
            let EntityKind::Point(p) = &e.kind else {
                return Err(error("Un riferimento logico deve essere un punto."));
            };
            if !["origin", "in", "out"].contains(&id)
                || !p.p.xy().is_finite()
                || p.p.x.abs() > 1000.0
                || p.p.y.abs() > 1000.0
                || p.p.z.abs() > 1e-7
                || out.contains_key(id)
            {
                return Err(error("Riferimenti duplicati o non validi: conserva un solo punto per Origine, IN e OUT."));
            }
            out.insert(id.into(), json!([p.p.x, p.p.y]));
        }
    }
    if !out.contains_key("origin") {
        return Err(error("Definisci il punto di inserimento (Origine)."));
    }
    Ok(out)
}
fn add_point(d: &mut Drawing, id: &str, at: Vec2) -> Result<()> {
    let existing: Vec<_> = d.model.iter().filter(|e| role(e) == Some(id)).map(|e| e.handle).collect();
    for h in existing {
        d.remove_entity(h);
    }
    let mut common = Common { layer: "EL_EDITOR_PUNTI".into(), ..Default::default() };
    common.xdata = vec![RawTag { code: 1001, value: MARK.into() }, RawTag { code: 1000, value: id.into() }];
    d.add(&Space::Model, common, EntityKind::Point(Point { p: Vec3::new(at.x, at.y, 0.0), angle: 0.0 }))?;
    Ok(())
}
fn current(s: &Session) -> Result<&Draft> {
    s.electrical_editor
        .as_ref()
        .filter(|e| s.state().is_ok_and(|st| st.uid == e.uid && st.block_edit.is_none()) && s.space() == Space::Model)
        .ok_or_else(|| error("Torna alla scheda di modifica del simbolo."))
}
pub fn active(s: &Session) -> bool {
    current(s).is_ok()
}
pub fn is_editor(s: &Session, index: usize) -> bool {
    s.electrical_editor.as_ref().is_some_and(|e| s.docs.get(index).is_some_and(|d| d.uid == e.uid))
}
fn open(s: &mut Session, p: &Value) -> Result<Value> {
    if s.running.is_some() || s.state().is_ok_and(|d| d.block_edit.is_some()) {
        return Err(error("Termina il comando corrente."));
    }
    if s.electrical_editor.as_ref().is_some_and(|e| s.docs.iter().any(|st| st.uid == e.uid)) {
        return Err(error("Chiudi prima l'editor simbolo già aperto."));
    }
    let path = p["catalog"].as_str().filter(|v| !v.is_empty() && v.len() <= 32768).ok_or_else(|| error("Indica il catalogo."))?;
    let id = p["id"].as_str().filter(|v| !v.is_empty() && v.len() <= 255).ok_or_else(|| error("Scegli un simbolo."))?;
    let path = PathBuf::from(path);
    let original = read(&path, 2 * 1024 * 1024)?;
    let raw = original.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&original);
    let catalog: Value = serde_json::from_slice(raw).map_err(|e| error(e.to_string()))?;
    let list = if catalog["format"] == "cadcraft-unifilar-symbols" && catalog["version"] == 1 {
        "symbols"
    } else if [Some(1), Some(2)].contains(&catalog["version"].as_u64()) {
        "components"
    } else {
        return Err(error("Catalogo non supportato."));
    };
    let entries = catalog[list].as_array().filter(|v| v.len() <= 2048).ok_or_else(|| error("Catalogo non valido."))?;
    let index = entries.iter().position(|v| v["id"] == id).ok_or_else(|| error("Simbolo assente dal catalogo."))?;
    let sym = entries.get(index).ok_or_else(|| error("Simbolo assente."))?;
    if sym.get("ports").is_some_and(|v| !v.is_array() || v.as_array().is_some_and(|v| v.len() > 16)) {
        return Err(error("Massimo 16 punti di collegamento per simbolo."));
    }
    let name = sym["name"].as_str().filter(|v| v.len() <= 255).ok_or_else(|| error("Nome non valido."))?.to_string();
    let library_path = local(path.parent().unwrap_or(Path::new(".")), catalog["library"].as_str().ok_or_else(|| error("Libreria assente."))?)?;
    let library_bytes = read(&library_path, MAX_FILE)?;
    let hooks = crate::cmd::file::io().ok_or_else(|| error("Formati file non disponibili."))?;
    let source = (hooks.read)(&library_bytes, &library_path.to_string_lossy()).map_err(error)?;
    let block = source.block(sym["block"].as_str().ok_or_else(|| error("Blocco assente."))?).ok_or_else(|| error("Blocco non trovato."))?;
    if block.entities.is_empty() || block.entities.len() > 20000 || source.entity_count() > 200000 {
        return Err(error("Simbolo o libreria troppo complessi."));
    }
    let geometry: Vec<_> = block.entities.iter().map(|e| e.as_ref().clone()).collect();
    let base = block.base.xy();
    let mut drawing = source.clone();
    drawing.model = Default::default();
    drawing.ensure_layer("EL_EDITOR_PUNTI");
    if let Some(layer) = drawing.layers.iter_mut().find(|l| l.name == "EL_EDITOR_PUNTI") {
        layer.plot = false;
    }
    drawing.header.set_i64("INSUNITS", 4);
    for mut e in geometry {
        e.kind.transform(&Mat3::translate(-base));
        drawing.add(&Space::Model, e.common, e.kind)?;
    }
    let origin = if list == "components" {
        if let Some(raw) = sym.get("editor_origin") {
            let a: [f64; 2] = serde_json::from_value(raw.clone()).map_err(|_| error("Origine catalogo non valida."))?;
            Vec2::new(a[0], a[1]) - base
        } else {
            let a: [f64; 2] = serde_json::from_value(sym.get("anchor").cloned().unwrap_or_else(|| json!([0.5, 0.5])))
                .map_err(|_| error("Ancora catalogo non valida."))?;
            if a.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) {
                return Err(error("Ancora catalogo non valida."));
            }
            let bounds = cadcraft_render::build_entities(&drawing, drawing.model.iter().map(|e| e.as_ref()), &Default::default()).bounds;
            if bounds.is_empty() {
                return Err(error("Simbolo senza geometria visibile."));
            }
            bounds.min + Vec2::new(bounds.width() * a[0], bounds.height() * a[1])
        }
    } else {
        Vec2::ZERO
    };
    if !origin.is_finite() || origin.x.abs() > 1000. || origin.y.abs() > 1000. {
        return Err(error("Origine fuori dai limiti."));
    }
    add_point(&mut drawing, "origin", origin)?;
    if let Some(ports) = sym["ports"].as_array() {
        for port in ports.iter().take(16) {
            if let Some(id) = port["id"].as_str().filter(|v| ["in", "out"].contains(v)) {
                let a = port["position"].as_array().ok_or_else(|| error("Punto non valido."))?;
                let x = a.first().and_then(Value::as_f64).ok_or_else(|| error("Punto non valido."))?;
                let y = a.get(1).and_then(Value::as_f64).ok_or_else(|| error("Punto non valido."))?;
                if !x.is_finite() || !y.is_finite() || x.abs() > 1000.0 || y.abs() > 1000.0 {
                    return Err(error("Punto fuori dai limiti."));
                }
                add_point(&mut drawing, id, Vec2::new(x, y))?;
            }
        }
    }
    let project = s.state().ok().map(|d| d.uid);
    s.open_drawing(drawing, &format!("Modifica simbolo — {name}"), None);
    let uid = s.state()?.uid;
    s.state_mut()?.saved = Arc::new(Drawing::new_metric());
    s.electrical_editor = Some(Draft {
        uid,
        project,
        path,
        original,
        library_bytes,
        library_path,
        catalog,
        index,
        list,
        source,
        id: id.into(),
        name,
        point: "in".into(),
    });
    inspect(s, &Value::Null)
}
fn inspect(s: &mut Session, _: &Value) -> Result<Value> {
    let e = current(s)?;
    let points = points(s.doc()?);
    Ok(
        json!({"uid":e.uid,"id":e.id,"name":e.name,"catalog":e.path,"points":points.as_ref().ok(),"points_error":points.as_ref().err().map(ToString::to_string),"revision":s.state()?.revision,"unifilar":e.list=="symbols"}),
    )
}
fn set_point(s: &mut Session, p: &Value) -> Result<Value> {
    current(s)?;
    let id = p["point"].as_str().filter(|v| ["origin", "in", "out"].contains(v)).ok_or_else(|| error("Scegli Origine, IN o OUT."))?;
    let at: [f64; 2] = serde_json::from_value(p["at"].clone()).map_err(|_| error("Indica un punto X,Y."))?;
    if at.iter().any(|v| !v.is_finite() || v.abs() > 1000.0) {
        return Err(error("Punto fuori dai limiti."));
    }
    let mut d = s.doc()?.clone();
    add_point(&mut d, id, Vec2::new(at[0], at[1]))?;
    *s.doc_mut()? = d;
    Ok(json!({"point":id,"at":at}))
}
fn choose(s: &mut Session, p: &Value) -> Result<Value> {
    current(s)?;
    let id = p["point"].as_str().filter(|v| ["origin", "in", "out"].contains(v)).ok_or_else(|| error("Scegli Origine, IN o OUT."))?;
    if let Some(e) = &mut s.electrical_editor {
        e.point = id.into();
    }
    Ok(json!({"point":id}))
}
fn finish(s: &mut Session, e: &Draft) {
    s.cancel();
    if let Some(i) = s.docs.iter().position(|d| d.uid == e.uid) {
        s.docs.remove(i);
    }
    s.active = e.project.and_then(|uid| s.docs.iter().position(|d| d.uid == uid)).unwrap_or_else(|| s.docs.len().saturating_sub(1));
    s.electrical_editor = None;
}
fn cancel(s: &mut Session, p: &Value) -> Result<Value> {
    let e = current(s)?.clone();
    if s.state()?.is_dirty() && p["discard"] != true {
        return Err(error("Ci sono modifiche: salva oppure conferma Annulla modifiche."));
    }
    finish(s, &e);
    Ok(json!({"closed":true}))
}
fn save(s: &mut Session, p: &Value) -> Result<Value> {
    let e = current(s)?.clone();
    if s.running.is_some() {
        return Err(error("Termina il comando prima di salvare."));
    }
    let name = p["name"].as_str().unwrap_or(&e.name).trim();
    if name.is_empty() || name.len() > 255 || name.chars().any(char::is_control) {
        return Err(error("Nome non valido."));
    }
    let copy = p["copy"].as_bool().unwrap_or(true);
    let pts = points(s.doc()?)?;
    let origin: [f64; 2] = serde_json::from_value(pts.get("origin").cloned().unwrap_or(Value::Null)).map_err(|_| error("Origine assente."))?;
    let translate = Vec2::new(-origin[0], -origin[1]);
    let mut d = s.doc()?.clone();
    d.model = e.source.model.clone();
    let mut geometry = Vec::new();
    for part in s.doc()?.model.iter() {
        if role(part).is_some() {
            continue;
        }
        if geometry.len() >= 20000 {
            return Err(error("Simbolo troppo complesso."));
        }
        let mut part = part.as_ref().clone();
        part.kind.transform(&Mat3::translate(translate));
        if matches!(&part.kind,EntityKind::Insert(i) if i.block==e.catalog[e.list][e.index]["block"].as_str().unwrap_or("")) {
            return Err(error("Il simbolo non può contenere un riferimento a sé stesso."));
        }
        geometry.push(part);
    }
    if geometry.is_empty() {
        return Err(error("Il simbolo non contiene geometria."));
    }
    let display = cadcraft_render::build_entities(s.doc()?, geometry.iter(), &Default::default());
    let bounds = display.bounds;
    if bounds.is_empty()
        || !bounds.min.is_finite()
        || !bounds.max.is_finite()
        || [bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y].iter().any(|v| v.abs() > 1000.0)
    {
        return Err(error("Geometria vuota o fuori dai limiti."));
    }
    let stamp = timestamp()?;
    let id = if copy { format!("custom-{stamp}") } else { e.id.clone() };
    // Variants can share a source block: even replacing one entry must not alter its siblings.
    let block_name = format!("EL_CUSTOM_{stamp}");
    // Retain any new CAD-created resources (layers, blocks and styles) from the editor.
    d.blocks = s.doc()?.blocks.clone();
    d.layers = s.doc()?.layers.clone();
    d.text_styles = s.doc()?.text_styles.clone();
    let mut block = Block::new(&block_name);
    for mut part in geometry {
        part.handle = d.new_handle();
        block.entities.push(part);
    }
    d.blocks.insert(block_name.clone(), Arc::new(block));
    let mut sym = e.catalog[e.list].as_array().and_then(|v| v.get(e.index)).cloned().ok_or_else(|| error("Voce catalogo assente."))?;
    sym["id"] = json!(id);
    sym["name"] = json!(name);
    sym["block"] = json!(block_name);
    let mut ports = Vec::new();
    for port in ["in", "out"] {
        if let Some(pos) = pts.get(port) {
            let a: [f64; 2] = serde_json::from_value(pos.clone()).map_err(|_| error("Punto non valido."))?;
            let mut old = sym["ports"]
                .as_array()
                .and_then(|v| v.iter().find(|v| v["id"] == port))
                .cloned()
                .unwrap_or_else(|| json!({"id":port,"role":"power"}));
            old["position"] = json!([a[0] - origin[0], a[1] - origin[1], 0]);
            ports.push(old);
        }
    }
    if let Some(old) = sym["ports"].as_array() {
        ports.extend(old.iter().filter(|v| v["id"] != "in" && v["id"] != "out").cloned().map(|mut v| {
            if let Some(a) = v["position"].as_array() {
                let x = a.first().and_then(Value::as_f64).unwrap_or(0.0);
                let y = a.get(1).and_then(Value::as_f64).unwrap_or(0.0);
                v["position"] = json!([x - origin[0], y - origin[1], 0]);
            }
            v
        }));
    }
    sym["ports"] = json!(ports);
    let mut port_ids = std::collections::BTreeSet::new();
    for port in sym["ports"].as_array().ok_or_else(|| error("Punti non validi."))? {
        let id = port["id"].as_str().filter(|v| !v.is_empty() && v.len() <= 64).ok_or_else(|| error("Nome punto non valido."))?;
        let a: [f64; 3] = serde_json::from_value(port["position"].clone()).map_err(|_| error("Coordinate punto non valide."))?;
        if !port_ids.insert(id) || a.iter().any(|v| !v.is_finite() || v.abs() > 1000.) {
            return Err(error("Punti duplicati o fuori dai limiti."));
        }
    }
    sym["insertion"] = json!([0, 0, 0]);
    if e.list == "components" {
        sym["editor_origin"] = json!([0, 0]);
    }
    sym["bounds"] = json!({"min":[bounds.min.x,bounds.min.y,0],"max":[bounds.max.x,bounds.max.y,0]});
    sym["editor_revision"] = json!(stamp);
    sym["source_geometry_preserved"] = json!(false);
    if let Some(obj) = sym.as_object_mut() {
        obj.remove("geometry_sha256");
    }
    let mut catalog = e.catalog.clone();
    let entries = catalog[e.list].as_array_mut().ok_or_else(|| error("Catalogo non valido."))?;
    if copy {
        if entries.len() >= if e.list == "symbols" { 512 } else { 2048 } {
            return Err(error("Catalogo pieno."));
        }
        entries.push(sym);
    } else if let Some(entry) = entries.get_mut(e.index) {
        *entry = sym;
    }
    let library_name = format!("simboli-editor-{stamp}.dxf");
    catalog["library"] = json!(library_name);
    let catalog_bytes = serde_json::to_vec_pretty(&catalog).map_err(|v| error(v.to_string()))?;
    if catalog_bytes.len() > if e.list == "symbols" { 2 * 1024 * 1024 } else { 1024 * 1024 } {
        return Err(error("Catalogo troppo grande."));
    }
    let hooks = crate::cmd::file::io().ok_or_else(|| error("Formati file assenti."))?;
    let bytes = (hooks.write)(&d, &library_name).map_err(error)?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(error("Libreria troppo grande per le anteprime."));
    }
    let restored = (hooks.read)(&bytes, &library_name).map_err(error)?;
    if restored.block(&block_name).is_none() {
        return Err(error("Verifica del simbolo salvato fallita."));
    }
    publish(&e, &library_name, &bytes, &catalog_bytes, &stamp)?;
    let result = json!({"saved":true,"copy":copy,"id":id,"catalog":e.path,"library":library_name,"name":name,"unifilar":e.list=="symbols","bounds":catalog[e.list].as_array().and_then(|v|v.iter().find(|v|v["id"]==id)).map(|v|v["bounds"].clone())});
    finish(s, &e);
    Ok(result)
}
#[cfg(not(target_arch = "wasm32"))]
fn timestamp() -> Result<String> {
    Ok(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| error(e.to_string()))?.as_nanos().to_string())
}
#[cfg(target_arch = "wasm32")]
fn timestamp() -> Result<String> {
    Err(error("Richiede desktop."))
}
#[cfg(not(target_arch = "wasm32"))]
fn publish(e: &Draft, library: &str, bytes: &[u8], catalog: &[u8], stamp: &str) -> Result<()> {
    use std::io::Write;
    if read(&e.path, 2 * 1024 * 1024)? != e.original || read(&e.library_path, MAX_FILE)? != e.library_bytes {
        return Err(error("Il catalogo o la libreria sono cambiati sul disco. Riapri l'editor."));
    }
    let parent = e.path.parent().unwrap_or(Path::new("."));
    let backups = parent.join("backups");
    std::fs::create_dir_all(&backups).map_err(|e| error(e.to_string()))?;
    let write_new = |path: &Path, data: &[u8]| -> Result<()> {
        let mut f = std::fs::OpenOptions::new().create_new(true).write(true).open(path).map_err(|e| error(e.to_string()))?;
        f.write_all(data).and_then(|()| f.sync_all()).map_err(|e| error(e.to_string()))
    };
    write_new(&backups.join(format!("catalogo-editor-{stamp}.json")), &e.original)?;
    let lib = parent.join(library);
    write_new(&lib, bytes)?;
    let temp = parent.join(format!("catalogo-editor-{stamp}.tmp"));
    write_new(&temp, catalog)?;
    if read(&e.path, 2 * 1024 * 1024)? != e.original {
        return Err(error("Il catalogo è cambiato durante il salvataggio."));
    }
    if let Err(err) = std::fs::rename(&temp, &e.path) {
        let _ = std::fs::remove_file(&temp);
        return Err(error(err.to_string()));
    }
    Ok(())
}
#[cfg(target_arch = "wasm32")]
fn publish(_: &Draft, _: &str, _: &[u8], _: &[u8], _: &str) -> Result<()> {
    Err(error("Richiede desktop."))
}
struct PickPoint {
    id: String,
}
impl Interactive for PickPoint {
    fn name(&self) -> &'static str {
        "electrical.library.point"
    }
    fn prompt(&self, _: &Session) -> Prompt {
        Prompt::new(format!("Indica {} nel simbolo; Esc annulla", self.id.to_uppercase()), Accept::POINT)
    }
    fn input(&mut self, s: &mut Session, input: Input) -> Result<Step> {
        match input {
            Input::Point(p) => {
                set_point(s, &json!({"point":self.id,"at":[p.x,p.y]}))?;
                Ok(Step::Done)
            }
            Input::Cancel => Ok(Step::Cancel),
            _ => Ok(Step::Continue),
        }
    }
}
