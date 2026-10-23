//! Atomic column-level circuit moves. No catalog artwork is regenerated or replaced.
use super::super::*;
use super::{connections, placement, schema};
use crate::EngineError;
use cadcraft_doc::{Drawing, Handle};
use std::collections::BTreeSet;
use std::sync::Mutex;
const CMD: &str = "electrical.unifilar.circuit.move";
const APP: &str = "CC_UNIFILARE_DEVICE";
type Cached = Arc<Mutex<Option<(usize, std::result::Result<Arc<Drawing>, String>)>>>;
#[derive(Clone)]
pub struct Draft {
    uid: u64,
    baseline: Arc<Drawing>,
    layout: schema::Layout,
    handles: BTreeSet<Handle>,
    source: usize,
    selection: Vec<Handle>,
    backup_dir: Option<String>,
    cache: Cached,
    pub summary: String,
    pub error: Option<String>,
}
fn error(s: impl Into<String>) -> EngineError {
    bad(CMD, s)
}
fn message(e: EngineError) -> String {
    match e {
        EngineError::BadParams { msg, .. } => msg,
        other => other.to_string(),
    }
}
fn build(s: &Session, p: &Value) -> Result<Draft> {
    let (fallback, _, _) = schema::load(p["model"].as_str().ok_or_else(|| error("Indica il modello della tavola."))?)?;
    let layout = placement::sheet(s, &fallback)?;
    let selected = p["handle"]
        .as_str()
        .and_then(Handle::parse_hex)
        .or_else(|| {
            let selection = s.selection();
            selection.first().copied().filter(|_| selection.len() == 1)
        })
        .ok_or_else(|| error("Seleziona un apparecchio del circuito da spostare."))?;
    let entity = s.doc()?.entity(selected).ok_or_else(|| error("Apparecchio assente."))?;
    let meta = placement::data(entity, APP)?.filter(|m| m["kind"] == "device").ok_or_else(|| error("Seleziona un apparecchio unifilare."))?;
    let source = meta["column"].as_u64().and_then(|v| usize::try_from(v).ok()).ok_or_else(|| error("Colonna dell'apparecchio non valida."))?;
    let mut handles = BTreeSet::new();
    let mut count = 0;
    for e in s.doc()?.model.iter() {
        if let Some(m) = placement::data(e, APP)?
            && m["column"].as_u64() == Some(source as u64)
        {
            handles.insert(e.handle);
            if m["kind"] == "device" {
                count += 1;
            }
        }
    }
    if handles.is_empty() || handles.len() > 256 {
        return Err(error("Circuito vuoto o troppo complesso."));
    }
    Ok(Draft {
        uid: s.state()?.uid,
        baseline: s.state()?.doc.clone(),
        layout,
        handles,
        source,
        selection: s.selection(),
        backup_dir: p["backup_dir"].as_str().map(str::to_owned),
        cache: Arc::new(Mutex::new(None)),
        summary: format!("Colonna {source} · {count} apparecchi"),
        error: None,
    })
}
fn column(d: &Draft, x: f64) -> Result<usize> {
    if !x.is_finite() {
        return Err(error("Posizione non valida."));
    }
    d.layout.columns.iter().find(|c| x >= c.bounds[0] && x <= c.bounds[2]).map(|c| c.index).ok_or_else(|| error("Scegli una colonna della tavola."))
}
fn validate(s: &Session, d: &Draft) -> Result<()> {
    if s.state()?.uid != d.uid || !Arc::ptr_eq(&s.state()?.doc, &d.baseline) {
        return Err(error("Il disegno è cambiato. Riapri Sposta circuito."));
    }
    Ok(())
}
fn transformed(d: &Draft, target: usize) -> Result<Arc<Drawing>> {
    if target == d.source {
        return Err(error("Scegli una colonna diversa da quella di partenza."));
    }
    let source = d.layout.columns.iter().find(|c| c.index == d.source).ok_or_else(|| error("Colonna di partenza assente."))?;
    let dest = d.layout.columns.iter().find(|c| c.index == target).ok_or_else(|| error("Colonna di destinazione assente."))?;
    let dx = dest.axis_x - source.axis_x;
    if (dest.bounds[2] - dest.bounds[0] - (source.bounds[2] - source.bounds[0])).abs() > 1e-6
        || source.cells.iter().any(|(k, v)| {
            dest.cells
                .get(k)
                .is_none_or(|w| (w.text_center[0] - v.text_center[0] - dx).abs() > 1e-6 || (w.text_center[1] - v.text_center[1]).abs() > 1e-6)
        })
    {
        return Err(error("Le due colonne hanno formati diversi."));
    }
    for e in d.baseline.model.iter() {
        if !d.handles.contains(&e.handle)
            && let Some(m) = placement::data(e, APP)?
            && m["column"].as_u64() == Some(target as u64)
        {
            return Err(error("La colonna di destinazione contiene già apparecchi o dati. Scegli una colonna libera."));
        }
    }
    let mut drawing = (*d.baseline).clone();
    for layer in ["UNI_APPARECCHI", "UNI_TESTI", "UNI_COLLEGAMENTI", "UNI_PE"] {
        if drawing.layer(layer).is_some_and(|l| l.locked || l.frozen || !l.on) {
            return Err(error(format!("Attiva e sblocca il layer {layer}.")));
        }
    }
    for h in &d.handles {
        let e = drawing.entity(*h).ok_or_else(|| error("Apparecchio assente."))?;
        if drawing.layer(&e.common.layer).is_some_and(|l| l.locked || l.frozen || !l.on) {
            return Err(error("Il layer dell'apparecchio è bloccato o nascosto."));
        }
        let mut meta = placement::data(e, APP)?.ok_or_else(|| error("Dati dell'apparecchio assenti."))?;
        let at: [f64; 2] = serde_json::from_value(meta["at"].clone()).map_err(|_| error("Posizione dell'apparecchio non valida."))?;
        let EntityKind::Insert(i) = &e.kind else {
            return Err(error("Apparecchio non valido."));
        };
        if i.insert.xy() != Vec2::new(at[0], at[1]) || i.rotation != 0. || i.scale != Vec3::new(1., 1., 1.) {
            return Err(error("Annulla la trasformazione generica dell'apparecchio."));
        }
        meta["at"] = json!([at[0] + dx, at[1]]);
        meta["column"] = json!(target);
        drawing.modify_entity(*h, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                i.insert.x += dx;
            }
            placement::tags(&mut e.common, APP, &meta);
        })?;
    }
    connections::relocate(&d.baseline, &mut drawing, &d.layout, &d.handles, dx)?;
    Ok(Arc::new(drawing))
}
fn candidate(d: &Draft, target: usize) -> Result<Arc<Drawing>> {
    if let Ok(cache) = d.cache.lock()
        && let Some((i, value)) = &*cache
        && *i == target
    {
        return value.clone().map_err(error);
    }
    let result = transformed(d, target).map_err(message);
    if let Ok(mut cache) = d.cache.lock() {
        *cache = Some((target, result.clone()));
    }
    result.map_err(error)
}
fn commit(s: &mut Session, d: &Draft, target: usize) -> Result<Value> {
    validate(s, d)?;
    let drawing = candidate(d, target)?;
    let saved = backup(s.doc()?, d.backup_dir.as_deref())?;
    *s.doc_mut()? = (*drawing).clone();
    s.unifilar_circuit = None;
    Ok(json!({"source":d.source,"column":target,"devices":d.handles.iter().map(|h|h.hex()).collect::<Vec<_>>(),"backup":saved}))
}
fn prepare(s: &mut Session, p: &Value) -> Result<Value> {
    if s.running.is_some() {
        return Err(error("Termina il comando corrente."));
    }
    let d = build(s, p)?;
    let out = json!({"source":d.source,"summary":d.summary,"handles":d.handles.iter().map(|h|h.hex()).collect::<Vec<_>>()});
    s.set_selection(d.handles.iter().copied().collect());
    s.unifilar_circuit = Some(d);
    Ok(out)
}
fn run(s: &mut Session, p: &Value) -> Result<Value> {
    if s.running.is_some() {
        return Err(error("Termina il comando corrente."));
    }
    let d = build(s, p)?;
    let target = p["column"].as_u64().and_then(|v| usize::try_from(v).ok()).ok_or_else(|| error("Indica la colonna di destinazione."))?;
    commit(s, &d, target)
}
pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new(CMD, "Move electrical circuit", run).params("{model,handle?,column,backup_dir?}"),
        CommandSpec::new("electrical.unifilar.circuit.prepare", "Prepare circuit move", prepare).noundo(),
        CommandSpec::new("electrical.unifilar.circuit.place", "Place electrical circuit", |_, _| {
            Err(error("Avvia Sposta circuito dal menu Elettrico."))
        })
        .enabled(|s| s.unifilar_circuit.as_ref().ok_or_else(|| "Seleziona un circuito.".to_string()).map(|_| ()))
        .interactive(|s| {
            if s.unifilar_circuit.is_none() {
                return Err(error("Prepara il circuito."));
            }
            Ok(Box::new(Place))
        }),
    ]
}
pub(crate) fn cancel(s: &mut Session) {
    if let Some(d) = s.unifilar_circuit.take()
        && s.state().is_ok_and(|st| st.uid == d.uid)
    {
        s.set_selection(d.selection);
    }
}
struct Place;
impl Interactive for Place {
    fn name(&self) -> &'static str {
        "electrical.unifilar.circuit.place"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        Prompt::new(
            s.unifilar_circuit.as_ref().and_then(|d| d.error.as_deref()).unwrap_or("Scegli la colonna libera di destinazione; Esc annulla"),
            Accept::POINT,
        )
    }
    fn input(&mut self, s: &mut Session, input: Input) -> Result<Step> {
        match input {
            Input::Cancel => {
                if let Some(d) = s.unifilar_circuit.take() {
                    s.set_selection(d.selection);
                }
                Ok(Step::Cancel)
            }
            Input::Point(p) => {
                let d = s.unifilar_circuit.as_ref().cloned().ok_or_else(|| error("Prepara il circuito."))?;
                let result = column(&d, p.x).and_then(|target| commit(s, &d, target));
                match result {
                    Ok(_) => {
                        s.echo("Circuito spostato. Annulla ripristina l'intero circuito.");
                        Ok(Step::Done)
                    }
                    Err(e) => {
                        if let Some(d) = &mut s.unifilar_circuit {
                            d.error = Some(match &e {
                                EngineError::BadParams { msg, .. } => msg.clone(),
                                _ => e.to_string(),
                            });
                        }
                        Err(e)
                    }
                }
            }
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, p: Vec2) -> Vec<EntityKind> {
        let Some(d) = &s.unifilar_circuit else { return vec![] };
        if validate(s, d).is_err() {
            return vec![];
        }
        let Ok(target) = column(d, p.x) else { return vec![] };
        let Some(col) = d.layout.columns.iter().find(|c| c.index == target) else { return vec![] };
        let mut out = vec![
            line(Vec2::new(col.bounds[0], col.bounds[1]), Vec2::new(col.bounds[0], col.bounds[3])),
            line(Vec2::new(col.bounds[2], col.bounds[1]), Vec2::new(col.bounds[2], col.bounds[3])),
        ];
        let Ok(draw) = candidate(d, target) else { return out };
        // Rendering expands inserts, including owned data and regenerated attached wires.
        let changed: Vec<_> = draw
            .model
            .iter()
            .filter(|e| {
                d.handles.contains(&e.handle)
                    || d.baseline.entity(e.handle).is_some_and(|old| old.kind != e.kind || old.common.xdata != e.common.xdata)
            })
            .cloned()
            .collect();
        let display = cadcraft_render::build_entities(&draw, changed.iter().map(AsRef::as_ref), &Default::default());
        for prim in display.prims.iter().take(20000) {
            let start = prim.start as usize;
            let Some(end) = start.checked_add(prim.len as usize) else { continue };
            if prim.kind == cadcraft_render::Kind::Polyline
                && let Some(points) = display.verts.get(start..end)
            {
                out.extend(points.windows(2).take(20000usize.saturating_sub(out.len())).map(|p| line(p[0], p[1])));
            }
            if out.len() >= 20000 {
                break;
            }
        }
        out
    }
}
