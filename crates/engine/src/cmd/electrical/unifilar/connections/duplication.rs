//! Duplication: bounded scope planning, identity remapping and atomic publication.
use super::*;
use crate::project::{id, references};
mod copying;
mod scope;
use scope::Plan;
const DEVICE: &str = "CC_UNIFILARE_DEVICE";
const CMD_COPY: &str = "electrical.unifilar.duplicate";
type Cached = Arc<Mutex<Option<(usize, std::result::Result<(Arc<Drawing>, Vec<Handle>, Vec<String>), String>)>>>;

#[derive(Clone)]
pub struct Draft {
    uid: u64,
    baseline: Arc<Drawing>,
    layout: schema::Layout,
    plan: Plan,
    source: usize,
    copies: usize,
    step: i64,
    renumber: bool,
    selection: Vec<Handle>,
    backup_dir: Option<String>,
    cache: Cached,
    pub summary: String,
    pub error: Option<String>,
}
fn fail(text: impl Into<String>) -> crate::EngineError {
    bad(CMD_COPY, text)
}
fn build(s: &Session, p: &Value) -> Result<Draft> {
    if s.running.is_some() || s.space() != Space::Model || s.state()?.block_edit.is_some() {
        return Err(fail("Termina il comando corrente e usa lo spazio modello."));
    }
    let mode = p["mode"].as_str().unwrap_or("column");
    if !["column", "circuit"].contains(&mode) {
        return Err(fail("Scegli colonna oppure circuito."));
    }
    let copies = match p.get("copies") {
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| (1..=32).contains(n))
            .ok_or_else(|| fail("Numero copie non valido (1–32)."))?,
        None => 1,
    };
    if mode == "circuit" && copies != 1 {
        return Err(fail("La serie è disponibile per la singola colonna."));
    }
    let step = match p.get("step") {
        Some(v) => v
            .as_i64()
            .filter(|n| *n != 0 && (-128..=128).contains(n))
            .ok_or_else(|| fail("Passo non valido: indica un numero di colonne diverso da zero."))?,
        None => 1,
    };
    let selected = s.selection();
    let root = if let Some(value) = p.get("handle") {
        value.as_str().and_then(Handle::parse_hex).ok_or_else(|| fail("Identificativo dell'apparecchio non valido."))?
    } else {
        selected
            .first()
            .copied()
            .filter(|_| selected.len() == 1)
            .ok_or_else(|| fail("Seleziona un solo apparecchio: nella colonna oppure all'inizio del circuito."))?
    };
    let meta = placement::data(s.doc()?.entity(root).ok_or_else(|| fail("Apparecchio assente."))?, DEVICE)?
        .ok_or_else(|| fail("Seleziona un apparecchio unifilare."))?;
    if meta["kind"] != "device" || references::is_reference(s.doc()?.entity(root).ok_or_else(|| fail("Apparecchio assente."))?) {
        return Err(fail("Seleziona un apparecchio della tavola, non un rimando o un marcatore."));
    }
    let source = meta["column"].as_u64().and_then(|v| usize::try_from(v).ok()).ok_or_else(|| fail("Apparecchio senza colonna."))?;
    let (fallback, _, _) = schema::load(s, p["model"].as_str().ok_or_else(|| fail("Indica il modello della tavola."))?)?;
    let layout = placement::sheet(s, &fallback)?;
    let env = Env::new(s.doc()?, &layout)?;
    let plan = scope::build(s.doc()?, &env, root, source, mode)?;
    let summary = format!(
        "{} · {} colonne · {} oggetti · {} copie",
        if mode == "column" { "Duplica colonna" } else { "Duplica circuito" },
        plan.columns.len(),
        plan.devices.len(),
        copies
    );
    Ok(Draft {
        uid: s.state()?.uid,
        baseline: s.state()?.doc.clone(),
        layout,
        plan,
        source,
        copies,
        step,
        renumber: p["renumber"].as_bool().unwrap_or(true),
        selection: selected,
        backup_dir: p["backup_dir"].as_str().map(str::to_owned),
        cache: Arc::new(Mutex::new(None)),
        summary,
        error: None,
    })
}
fn candidate(d: &Draft, target: usize) -> Result<(Arc<Drawing>, Vec<Handle>, Vec<String>)> {
    if let Ok(cache) = d.cache.lock()
        && let Some((key, result)) = &*cache
        && *key == target
    {
        return result.clone().map_err(fail);
    }
    let result = copying::transform(d, target).map(|(draw, handles, ids)| (Arc::new(draw), handles, ids)).map_err(|e| e.to_string());
    if let Ok(mut cache) = d.cache.lock() {
        *cache = Some((target, result.clone()));
    }
    result.map_err(fail)
}
fn valid(s: &Session, d: &Draft) -> Result<()> {
    if s.state()?.uid != d.uid || !Arc::ptr_eq(&s.state()?.doc, &d.baseline) {
        return Err(fail("Il disegno è cambiato. Riapri la duplicazione."));
    }
    Ok(())
}
fn commit(s: &mut Session, d: &Draft, target: usize) -> Result<Value> {
    valid(s, d)?;
    let (draw, handles, groups) = candidate(d, target)?;
    let saved = backup(s.doc()?, d.backup_dir.as_deref())?;
    *s.doc_mut()? = (*draw).clone();
    s.set_selection(handles.clone());
    s.unifilar_duplicate = None;
    Ok(json!({"copies":d.copies,"mode":d.plan.mode,"handles":handles.iter().map(|h|h.hex()).collect::<Vec<_>>(),"groups":groups,"backup":saved}))
}
fn prepare(s: &mut Session, p: &Value) -> Result<Value> {
    let d = build(s, p)?;
    let result =
        json!({"summary":d.summary,"devices":d.plan.devices.iter().map(|h|h.hex()).collect::<Vec<_>>(),"columns":d.plan.columns,"copies":d.copies});
    s.set_selection(d.plan.devices.iter().copied().collect());
    s.unifilar_duplicate = Some(d);
    Ok(result)
}
pub(crate) fn cancel(s: &mut Session) {
    if let Some(d) = s.unifilar_duplicate.take()
        && s.state().is_ok_and(|st| st.uid == d.uid)
    {
        s.set_selection(d.selection);
    }
}
pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new(CMD_COPY, "Duplicate electrical column or circuit", |s, p| {
            let d = build(s, p)?;
            let target = p["column"].as_u64().and_then(|v| usize::try_from(v).ok()).ok_or_else(|| fail("Indica la colonna di destinazione."))?;
            commit(s, &d, target)
        })
        .params("{model,handle?,mode?:column|circuit,column,copies?:1..32,step?:signed_columns,renumber?:bool,backup_dir?}"),
        CommandSpec::new("electrical.unifilar.duplicate.prepare", "Prepare electrical duplication", prepare).noundo(),
        CommandSpec::new("electrical.unifilar.duplicate.place", "Place electrical duplicates", |_, _| Err(fail("Usa la modalità interattiva.")))
            .enabled(|s| s.unifilar_duplicate.as_ref().map(|_| ()).ok_or_else(|| "Prepara la duplicazione.".into()))
            .interactive(|s| if s.unifilar_duplicate.is_some() { Ok(Box::new(Place)) } else { Err(fail("Prepara la duplicazione.")) }),
    ]
}
struct Place;
impl Interactive for Place {
    fn name(&self) -> &'static str {
        "electrical.unifilar.duplicate.place"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        Prompt::new(
            s.unifilar_duplicate
                .as_ref()
                .and_then(|d| d.error.as_deref())
                .unwrap_or("Scegli la colonna della prima copia e controlla l'anteprima; Esc annulla"),
            Accept::POINT,
        )
    }
    fn input(&mut self, s: &mut Session, input: Input) -> Result<Step> {
        match input {
            Input::Cancel => {
                cancel(s);
                Ok(Step::Cancel)
            }
            Input::Point(p) => {
                let d = s.unifilar_duplicate.as_ref().cloned().ok_or_else(|| fail("Prepara la duplicazione."))?;
                let target = d
                    .layout
                    .columns
                    .iter()
                    .find(|c| p.x >= c.bounds[0] && p.x <= c.bounds[2])
                    .map(|c| c.index)
                    .ok_or_else(|| fail("Scegli una colonna della tavola."));
                match target.and_then(|t| commit(s, &d, t)) {
                    Ok(_) => Ok(Step::Done),
                    Err(e) => {
                        if let Some(d) = &mut s.unifilar_duplicate {
                            d.error = Some(e.to_string());
                        }
                        Err(e)
                    }
                }
            }
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, p: Vec2) -> Vec<EntityKind> {
        let Some(d) = &s.unifilar_duplicate else { return vec![] };
        if valid(s, d).is_err() {
            return vec![];
        }
        let mut out: Vec<_> =
            if d.plan.mode == "column" { d.plan.wires.iter().flat_map(|w| w.path.windows(2).map(|p| line(p[0], p[1]))).collect() } else { vec![] };
        let Some(target) = d.layout.columns.iter().find(|c| p.x >= c.bounds[0] && p.x <= c.bounds[2]) else { return out };
        let Ok((draw, handles, _)) = candidate(d, target.index) else { return out };
        let display = cadcraft_render::build_entities(&draw, handles.iter().filter_map(|h| draw.entity(*h)).map(AsRef::as_ref), &Default::default());
        for prim in display.prims.iter().take(20000) {
            let Some(end) = (prim.start as usize).checked_add(prim.len as usize) else { continue };
            if prim.kind == cadcraft_render::Kind::Polyline
                && let Some(points) = display.verts.get(prim.start as usize..end)
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
