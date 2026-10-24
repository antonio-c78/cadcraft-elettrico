//! Electrical project workspace. Local electrical metadata remains authoritative in each DXF.
//! Stable sheet identities, bounded ZIP persistence and derived cross-sheet references.
mod archive;
pub mod references;
use crate::{
    DocState, Result, Session,
    cmd::{CommandSpec, bad},
};
use cadcraft_doc::Drawing;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const CMD: &str = "electrical.project";
pub(super) fn error(e: impl std::fmt::Display) -> crate::EngineError {
    bad(CMD, e.to_string())
}
pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn valid_id(s: &str) -> bool {
    uuid::Uuid::parse_str(s).is_ok_and(|v| v.to_string() == s)
}
fn text(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 160 && !s.chars().any(char::is_control)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sheet {
    pub id: String,
    pub number: String,
    pub title: String,
    pub panel: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(skip)]
    pub uid: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub id: String,
    pub name: String,
    pub sheets: Vec<Sheet>,
}
impl Metadata {
    fn validate(&self) -> Result<()> {
        if !valid_id(&self.id) || !text(&self.name) || self.sheets.len() > 128 {
            return Err(error("Nome o numero di fogli non valido (massimo 128)."));
        }
        let mut ids = BTreeSet::new();
        let mut numbers = BTreeSet::new();
        for s in &self.sheets {
            if !valid_id(&s.id)
                || !ids.insert(&s.id)
                || !text(&s.number)
                || s.number.len() > 20
                || !numbers.insert(s.number.to_lowercase())
                || !text(&s.title)
                || s.panel.len() > 80
                || s.panel.chars().any(char::is_control)
                || s.model.as_ref().is_some_and(|p| !p.starts_with("cce:/resources/") || p.contains("..") || p.contains('\\'))
            {
                return Err(error("Fogli con identità, numeri o nomi non validi o duplicati."));
            }
        }
        Ok(())
    }
}
pub struct Project {
    pub reference_pair: Option<String>,
    pub reference_direction: String,
    pub meta: Metadata,
    pub path: Option<String>,
    pub generation: u64,
    saved_meta: Value,
    pub resources: BTreeMap<String, Vec<u8>>,
    pub closed: Vec<DocState>,
    /// Full previous archive hash: optimistic concurrency check, including same-generation edits.
    disk_hash: Option<String>,
}
impl Project {
    pub fn sheet(&self, uid: u64) -> Option<&Sheet> {
        self.meta.sheets.iter().find(|s| s.uid == uid)
    }
    pub fn dirty(&self, session: &Session) -> bool {
        serde_json::to_value(&self.meta).ok().as_ref() != Some(&self.saved_meta)
            || session.docs.iter().chain(&self.closed).any(|d| self.sheet(d.uid).is_some() && d.is_dirty())
    }
}
pub fn contains(s: &Session, uid: u64) -> bool {
    s.project.as_ref().is_some_and(|p| p.sheet(uid).is_some())
}
pub fn active(s: &Session) -> bool {
    s.state().is_ok_and(|st| contains(s, st.uid))
}
pub fn dirty(s: &Session) -> bool {
    s.project.as_ref().is_some_and(|p| p.dirty(s))
}
pub fn specs() -> Vec<CommandSpec> {
    let mut v = vec![
        CommandSpec::new("electrical.project.new", "Create electrical project", new),
        CommandSpec::new("electrical.project.list", "List project sheets", list),
        CommandSpec::new("electrical.project.add", "Add drawing to project", add),
        CommandSpec::new("electrical.project.sheet", "Update project sheet", update),
        CommandSpec::new("electrical.project.switch", "Open project sheet", switch),
        CommandSpec::new("electrical.project.duplicate", "Duplicate project sheet", duplicate),
        CommandSpec::new("electrical.project.remove", "Remove project sheet", remove),
        CommandSpec::new("electrical.project.save", "Save electrical project", save),
        CommandSpec::new("electrical.project.open", "Open electrical project", open),
        CommandSpec::new("electrical.project.close", "Close electrical project", close),
    ]
    .into_iter()
    .map(|c| {
        let params = match c.id {
            "electrical.project.new" => "{name}",
            "electrical.project.add" => "{title?,panel?,model?:path_to_unifilar_template}",
            "electrical.project.sheet" => "{id?:sheet_uuid,name?:project_name,number?,title?,panel?,position?:zero_based,renumber?:bool}",
            "electrical.project.switch" | "electrical.project.duplicate" => "{id:sheet_uuid}",
            "electrical.project.remove" => "{id:sheet_uuid,confirm:true}",
            "electrical.project.save" => "{path?:cce_path}",
            "electrical.project.open" => "{path:cce_path}",
            "electrical.project.close" => "{discard?:bool}",
            _ => "{}",
        };
        c.params(params).noundo().enabled(|_| Ok(()))
    })
    .collect::<Vec<_>>();
    v.extend(references::specs());
    v
}
fn idle(s: &Session) -> Result<()> {
    if s.running.is_some() || s.docs.iter().any(|d| d.block_edit.is_some() || d.doc.refedit.is_some()) || s.electrical_editor.is_some() {
        return Err(error("Termina il comando o l'editor corrente."));
    }
    Ok(())
}
fn new(s: &mut Session, p: &Value) -> Result<Value> {
    idle(s)?;
    if s.project.is_some() {
        return Err(error("È già aperto un progetto. Chiudilo prima di crearne un altro."));
    }
    let name = p["name"].as_str().unwrap_or("Nuovo progetto");
    let meta = Metadata { id: id(), name: name.into(), sheets: vec![] };
    meta.validate()?;
    s.project = Some(Project {
        reference_pair: None,
        reference_direction: "right".into(),
        meta,
        path: None,
        generation: 0,
        saved_meta: Value::Null,
        resources: BTreeMap::new(),
        closed: vec![],
        disk_hash: None,
    });
    list(s, &Value::Null)
}
pub fn list(s: &mut Session, _: &Value) -> Result<Value> {
    references::refresh(s)?;
    let Some(p) = &s.project else {
        return Ok(json!({"open":false,"sheets":[]}));
    };
    let sheets: Vec<_> = p
        .meta
        .sheets
        .iter()
        .map(|sheet| {
            let state = s.docs.iter().chain(&p.closed).find(|d| d.uid == sheet.uid);
            json!({"id":sheet.id,"number":sheet.number,"title":sheet.title,"panel":sheet.panel,"uid":sheet.uid,
            "index":s.docs.iter().position(|d|d.uid == sheet.uid),"dirty":state.is_some_and(DocState::is_dirty),"model":sheet.model})
        })
        .collect();
    Ok(json!({"open":true,"id":p.meta.id,"name":p.meta.name,"path":p.path,"generation":p.generation,"dirty":p.dirty(s),"sheets":sheets,
        "references":references::inspect(s)?}))
}
fn number(p: &Project) -> String {
    (1..=999).map(|i| format!("{i:02}")).find(|n| !p.meta.sheets.iter().any(|v| v.number == *n)).unwrap_or_else(id)
}
pub fn add(s: &mut Session, params: &Value) -> Result<Value> {
    idle(s)?;
    let st = s.state()?;
    let p = s.project.as_ref().ok_or_else(|| error("Crea prima un progetto."))?;
    if p.sheet(st.uid).is_some() {
        return list(s, &Value::Null);
    }
    let sheet = Sheet {
        id: id(),
        uid: st.uid,
        number: number(p),
        title: params["title"].as_str().unwrap_or(&st.title).into(),
        panel: params["panel"].as_str().unwrap_or("").into(),
        model: None,
    };
    let mut meta = p.meta.clone();
    meta.sheets.push(sheet);
    meta.validate()?;
    let old_meta = p.meta.clone();
    let old_resources = p.resources.clone();
    if let Some(p) = &mut s.project {
        p.meta = meta;
    }
    if let Some(model) = params["model"].as_str()
        && let Err(e) = capture_model(s, model)
    {
        if let Some(p) = &mut s.project {
            p.meta = old_meta;
            p.resources = old_resources;
        }
        return Err(e);
    }
    // The imported source is never a save destination while it belongs to a project.
    s.state_mut()?.path = None;
    sync_titles(s);
    list(s, &Value::Null)
}
fn update(s: &mut Session, value: &Value) -> Result<Value> {
    idle(s)?;
    let p = s.project.as_ref().ok_or_else(|| error("Nessun progetto aperto."))?;
    let mut meta = p.meta.clone();
    if let Some(name) = value["name"].as_str() {
        meta.name = name.into();
    }
    if value["renumber"] == true {
        for (i, sheet) in meta.sheets.iter_mut().enumerate() {
            sheet.number = format!("{:02}", i + 1);
        }
    }
    if let Some(id) = value["id"].as_str() {
        let sheet = meta.sheets.iter_mut().find(|s| s.id == id).ok_or_else(|| error("Foglio assente."))?;
        for (key, field) in [("number", &mut sheet.number), ("title", &mut sheet.title), ("panel", &mut sheet.panel)] {
            if let Some(t) = value[key].as_str() {
                *field = t.into();
            }
        }
        if let Some(to) = value["position"].as_u64() {
            let to = usize::try_from(to).map_err(error)?;
            if to >= meta.sheets.len() {
                return Err(error("Posizione fuori elenco."));
            }
            let from = meta.sheets.iter().position(|v| v.id == id).ok_or_else(|| error("Foglio assente."))?;
            let sheet = meta.sheets.remove(from);
            meta.sheets.insert(to, sheet);
        }
    }
    meta.validate()?;
    if let Some(p) = &mut s.project {
        p.meta = meta;
    }
    sync_titles(s);
    list(s, &Value::Null)
}
fn sync_titles(s: &mut Session) {
    let Some(p) = &mut s.project else { return };
    let count = p.meta.sheets.len();
    for st in s.docs.iter_mut().chain(&mut p.closed) {
        if let Some(sheet) = p.meta.sheets.iter().find(|a| a.uid == st.uid) {
            st.title = format!("{} · {}", sheet.number, sheet.title);
            let value = format!("{} / {count:02}", sheet.number);
            let handles:Vec<_>=st.doc.model.iter().filter(|e|e.common.xdata.iter().any(|t|t.code==1001&&t.value=="CC_UNIFILARE_MODELO"))
                .filter(|e|matches!(&e.kind,cadcraft_doc::EntityKind::Insert(i) if i.attribs.iter().any(|a|a.tag.eq_ignore_ascii_case("foglio")&&a.text.value!=value)))
                .map(|e|e.handle).collect();
            for h in handles {
                let _ = Arc::make_mut(&mut st.doc).modify_entity(h, |e| {
                    if let cadcraft_doc::EntityKind::Insert(i) = &mut e.kind {
                        for a in &mut i.attribs {
                            if a.tag.eq_ignore_ascii_case("foglio") {
                                a.text.value = value.clone();
                            }
                        }
                    }
                });
                st.revision = st.revision.saturating_add(1);
            }
        }
    }
}
pub fn switch(s: &mut Session, value: &Value) -> Result<Value> {
    idle(s)?;
    let uid = s
        .project
        .as_ref()
        .and_then(|p| p.meta.sheets.iter().find(|v| Some(v.id.as_str()) == value["id"].as_str()))
        .map(|v| v.uid)
        .ok_or_else(|| error("Foglio assente."))?;
    if let Some(p) = &mut s.project
        && let Some(i) = p.closed.iter().position(|d| d.uid == uid)
    {
        s.docs.push(p.closed.remove(i));
    }
    let i = s.docs.iter().position(|d| d.uid == uid).ok_or_else(|| error("Disegno del foglio assente."))?;
    s.execute("document.switch", &json!({"index":i}))?;
    Ok(json!({"index":i,"uid":uid}))
}
fn duplicate(s: &mut Session, value: &Value) -> Result<Value> {
    idle(s)?;
    let p = s.project.as_ref().ok_or_else(|| error("Nessun progetto aperto."))?;
    let sheet = p.meta.sheets.iter().find(|v| Some(v.id.as_str()) == value["id"].as_str()).ok_or_else(|| error("Foglio assente."))?.clone();
    let original = s.docs.iter().chain(&p.closed).find(|d| d.uid == sheet.uid).ok_or_else(|| error("Disegno assente."))?;
    let mut d = (*original.doc).clone();
    // New reference identities preserve local wire handles and never join the original circuit.
    let mut entities: Vec<_> = d.model.iter().map(|e| e.as_ref().clone()).collect();
    for e in &mut entities {
        references::duplicate_entity(e);
    }
    for e in entities {
        d.model.replace(e);
    }
    let model = sheet.model.clone();
    s.open_drawing(d, &format!("{} copia", sheet.title), None);
    add(s, &json!({"panel":sheet.panel}))?;
    let uid = s.state()?.uid;
    if let Some(p) = &mut s.project
        && let Some(sheet) = p.meta.sheets.iter_mut().find(|a| a.uid == uid)
    {
        sheet.model = model;
    }
    sync_titles(s);
    list(s, &Value::Null)
}
fn remove(s: &mut Session, v: &Value) -> Result<Value> {
    idle(s)?;
    if v["confirm"] != true {
        return Err(error("Conferma esplicitamente la rimozione del foglio."));
    }
    let p = s.project.as_mut().ok_or_else(|| error("Nessun progetto aperto."))?;
    let index = p.meta.sheets.iter().position(|a| Some(a.id.as_str()) == v["id"].as_str()).ok_or_else(|| error("Foglio assente."))?;
    let sheet = p.meta.sheets.remove(index);
    p.closed.retain(|d| d.uid != sheet.uid);
    s.docs.retain(|d| d.uid != sheet.uid);
    s.active = s.active.min(s.docs.len().saturating_sub(1));
    list(s, &Value::Null)
}
/// A tab can close without deleting its sheet or losing its unsaved data.
pub fn close_tab(s: &mut Session, i: usize) -> bool {
    let Some(st) = s.docs.get(i) else { return false };
    if !contains(s, st.uid) {
        return false;
    }
    if let Some(p) = &mut s.project {
        p.closed.push(s.docs.remove(i));
    }
    if i < s.active {
        s.active -= 1;
    }
    s.active = s.active.min(s.docs.len().saturating_sub(1));
    true
}
fn close(s: &mut Session, v: &Value) -> Result<Value> {
    idle(s)?;
    if dirty(s) && v["discard"] != true {
        return Err(error("Salva il progetto o conferma di scartare le modifiche."));
    }
    if let Some(p) = s.project.take() {
        s.docs.retain(|d| p.sheet(d.uid).is_none());
    }
    s.active = s.active.min(s.docs.len().saturating_sub(1));
    Ok(json!({"closed":true}))
}
pub fn resource(s: &Session, path: &str, limit: u64) -> Result<Option<Vec<u8>>> {
    let Some(name) = path.replace('\\', "/").strip_prefix("cce:/").map(str::to_string) else { return Ok(None) };
    let b = s.project.as_ref().and_then(|p| p.resources.get(&name)).ok_or_else(|| error("Risorsa incorporata nel progetto assente."))?;
    if b.len() as u64 > limit {
        return Err(error("Risorsa incorporata troppo grande."));
    }
    Ok(Some(b.clone()))
}
pub fn model(s: &Session) -> Option<&str> {
    s.project.as_ref()?.sheet(s.state().ok()?.uid)?.model.as_deref()
}
/// Freeze the template folder's declared dependencies, without copying arbitrary sibling files.
pub fn capture_model(s: &mut Session, path: &str) -> Result<()> {
    if path.starts_with("cce:/") {
        let uid = s.state()?.uid;
        if let Some(p) = &mut s.project
            && let Some(sheet) = p.meta.sheets.iter_mut().find(|a| a.uid == uid)
        {
            sheet.model = Some(path.into());
        }
        return Ok(());
    }
    let Some(sheet_id) = s.project.as_ref().and_then(|p| p.sheet(s.state().ok()?.uid)).map(|a| a.id.clone()) else { return Ok(()) };
    let root = std::path::Path::new(path).parent().ok_or_else(|| error("Cartella del modello assente."))?;
    let bytes = read_file(path, 524288)?;
    let raw: Value = serde_json::from_slice(&bytes).map_err(error)?;
    let mut names = vec![
        raw["catalog"].as_str().ok_or_else(|| error("Catalogo del modello assente."))?.to_owned(),
        raw["template"].as_str().ok_or_else(|| error("Cartiglio assente."))?.to_owned(),
    ];
    let mut out = BTreeMap::new();
    for name in names.clone() {
        if name.contains(['/', '\\', ':']) || name == ".." {
            return Err(error("Dipendenza del modello non locale."));
        }
        let data = read_file(&root.join(&name).to_string_lossy(), 32 * 1024 * 1024)?;
        if name == raw["catalog"].as_str().unwrap_or("") {
            let catalog: Value = serde_json::from_slice(&data).map_err(error)?;
            names.push(catalog["library"].as_str().ok_or_else(|| error("Libreria assente."))?.into());
        }
        out.insert(name, data);
    }
    for name in names {
        if name.contains(['/', '\\', ':']) || name == ".." {
            return Err(error("Dipendenza del modello non locale."));
        }
        if !out.contains_key(&name) {
            out.insert(name.clone(), read_file(&root.join(&name).to_string_lossy(), 32 * 1024 * 1024)?);
        }
    }
    let filename = std::path::Path::new(path).file_name().and_then(|f| f.to_str()).ok_or_else(|| error("Nome del modello non valido."))?;
    out.insert(filename.into(), bytes);
    if let Some(p) = &mut s.project {
        for (name, bytes) in out {
            p.resources.insert(format!("resources/templates/{sheet_id}/{name}"), bytes);
        }
        if let Some(sheet) = p.meta.sheets.iter_mut().find(|a| a.id == sheet_id) {
            sheet.model = Some(format!("cce:/resources/templates/{sheet_id}/{filename}"));
        }
    }
    Ok(())
}
pub fn read_file(path: &str, max: u64) -> Result<Vec<u8>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::io::Read;
        let f = std::fs::File::open(path).map_err(error)?;
        let mut bytes = Vec::new();
        f.take(max + 1).read_to_end(&mut bytes).map_err(error)?;
        if bytes.len() as u64 > max {
            return Err(error("File troppo grande."));
        }
        Ok(bytes)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (path, max);
        Err(error("Il percorso locale richiede la versione desktop."))
    }
}
pub struct Loaded {
    meta: Metadata,
    generation: u64,
    resources: BTreeMap<String, Vec<u8>>,
    drawings: Vec<Drawing>,
    hash: String,
}
pub fn decode(bytes: &[u8]) -> Result<Loaded> {
    let (meta, generation, mut files) = archive::decode(bytes)?;
    let hooks = crate::cmd::file::io().ok_or_else(|| error("Lettore DXF non disponibile."))?;
    let mut drawings = Vec::new();
    for sheet in &meta.sheets {
        let name = format!("drawings/{}.dxf", sheet.id);
        let data = files.remove(&name).ok_or_else(|| error("Disegno del foglio mancante."))?;
        let drawing = (hooks.read)(&data, &name).map_err(error)?;
        if drawing.entity_count() > 200000 {
            return Err(error("Disegno troppo complesso."));
        }
        if let Some(model) = &sheet.model
            && !files.contains_key(model.trim_start_matches("cce:/"))
        {
            return Err(error("Definizione del modello incorporato mancante."));
        }
        drawings.push(drawing);
    }
    files.remove("project.json");
    Ok(Loaded { meta, generation, resources: files, drawings, hash: hash(bytes) })
}
pub fn finish_open(s: &mut Session, mut loaded: Loaded, path: Option<String>) -> Result<Value> {
    idle(s)?;
    if s.project.is_some() {
        return Err(error("Chiudi il progetto corrente prima di aprirne un altro."));
    }
    let old_count = s.docs.len();
    let old_active = s.active;
    for (sheet, drawing) in loaded.meta.sheets.iter_mut().zip(loaded.drawings) {
        let i = s.open_drawing(drawing, &format!("{} · {}", sheet.number, sheet.title), None);
        sheet.uid = s.docs.get(i).map(|d| d.uid).ok_or_else(|| error("Impossibile aprire il foglio."))?;
    }
    let saved_meta = serde_json::to_value(&loaded.meta).map_err(error)?;
    s.project = Some(Project {
        reference_pair: None,
        reference_direction: "right".into(),
        meta: loaded.meta,
        path,
        generation: loaded.generation,
        saved_meta,
        resources: loaded.resources,
        closed: vec![],
        disk_hash: Some(loaded.hash),
    });
    let result = list(s, &Value::Null);
    if result.is_err() {
        s.project = None;
        s.docs.truncate(old_count);
        s.active = old_active;
    }
    result
}
fn hash(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(b))
}
fn open(s: &mut Session, p: &Value) -> Result<Value> {
    let path = p["path"].as_str().ok_or_else(|| error("Indica il file .cce."))?;
    let loaded = decode(&read_file(path, archive::MAX_TOTAL as u64)?)?;
    finish_open(s, loaded, Some(path.into()))
}
pub fn save(s: &mut Session, v: &Value) -> Result<Value> {
    idle(s)?;
    references::refresh(s)?;
    let p = s.project.as_ref().ok_or_else(|| error("Nessun progetto aperto."))?;
    let path = v["path"].as_str().map(str::to_string).or_else(|| p.path.clone()).ok_or_else(|| error("Scegli dove salvare il progetto .cce."))?;
    if !path.to_lowercase().ends_with(".cce") || path.len() > 32768 {
        return Err(error("Il file di progetto deve avere estensione .cce."));
    }
    let hooks = crate::cmd::file::io().ok_or_else(|| error("Scrittore DXF non disponibile."))?;
    p.meta.validate()?;
    let mut files = p.resources.clone();
    for sheet in &p.meta.sheets {
        let st = s.docs.iter().chain(&p.closed).find(|d| d.uid == sheet.uid).ok_or_else(|| error("Foglio del progetto assente."))?;
        files.insert(format!("drawings/{}.dxf", sheet.id), (hooks.write)(&st.doc, "drawing.dxf").map_err(error)?);
    }
    let generation = p.generation.checked_add(1).ok_or_else(|| error("Troppe revisioni."))?;
    let bytes = archive::encode(&p.meta, generation, files)?;
    // Validate the complete package before replacing anything; never overwrite a divergent copy.
    let _ = decode(&bytes)?;
    store(&path, &bytes, if p.path.as_deref() == Some(&path) { p.disk_hash.as_deref() } else { None })?;
    if let Some(p) = &mut s.project {
        p.path = Some(path.clone());
        p.generation = generation;
        p.disk_hash = Some(hash(&bytes));
        p.saved_meta = serde_json::to_value(&p.meta).map_err(error)?;
        for st in s.docs.iter_mut().chain(&mut p.closed) {
            if p.meta.sheets.iter().any(|a| a.uid == st.uid) {
                st.saved = st.doc.clone();
            }
        }
    }
    Ok(json!({"path":path,"generation":generation,"bytes":bytes.len()}))
}
#[cfg(not(target_arch = "wasm32"))]
fn store(path: &str, bytes: &[u8], expected: Option<&str>) -> Result<()> {
    use std::io::Write;
    let lockpath = format!("{path}.lock");
    let lock = std::fs::OpenOptions::new().write(true).create_new(true).open(&lockpath).map_err(|_| {
        error("Progetto occupato da un altro salvataggio. Se il programma è stato interrotto, conserva e rimuovi il file .lock prima di riprovare.")
    })?;
    let temp = format!("{path}.{}.tmp", id());
    let result = (|| {
        if let Some(expected) = expected
            && hash(&read_file(path, archive::MAX_TOTAL as u64)?) != expected
        {
            return Err(error("Il progetto su disco è cambiato. Salva una copia con un altro nome."));
        }
        let mut file = std::fs::OpenOptions::new().create_new(true).write(true).open(&temp).map_err(error)?;
        file.write_all(bytes).map_err(error)?;
        file.sync_all().map_err(error)?;
        drop(file);
        let _ = archive::decode(&read_file(&temp, archive::MAX_TOTAL as u64)?)?;
        if std::path::Path::new(path).exists() {
            std::fs::copy(path, format!("{path}.bak")).map_err(error)?;
        }
        std::fs::rename(&temp, path).map_err(error)?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&temp);
    drop(lock);
    let _ = std::fs::remove_file(lockpath);
    result
}
#[cfg(target_arch = "wasm32")]
fn store(path: &str, bytes: &[u8], _: Option<&str>) -> Result<()> {
    crate::cmd::file::deliver().ok_or_else(|| error("Download non disponibile."))?(path, "application/zip", bytes).map_err(error)
}
