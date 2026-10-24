//! Logical-section deletion: graph planning, preview and atomic commit are separate.
use super::*;
mod graph;
use graph::{End, Graph, projection};
#[derive(Clone)]
pub struct Draft {
    uid: u64,
    baseline: Arc<Drawing>,
    layout: schema::Layout,
    target: Handle,
    selection: Vec<Handle>,
    graph: Graph,
    backup_dir: Option<String>,
    chosen: Option<(Vec<Vec2>, BTreeSet<usize>)>,
    pub error: Option<String>,
}
impl Draft {
    pub fn chosen(&self) -> bool {
        self.chosen.is_some()
    }
}
fn build(s: &Session, p: &Value) -> Result<Draft> {
    if s.running.is_some() {
        return Err(err("Termina il comando corrente."));
    }
    let (layout, _, _) = schema::load(p["model"].as_str().ok_or_else(|| err("Indica il modello."))?)?;
    let layout = placement::sheet(s, &layout)?;
    let target = selected(s, p)?;
    let env = Env::new(s.doc()?, &layout)?;
    Ok(Draft {
        uid: s.state()?.uid,
        baseline: s.state()?.doc.clone(),
        graph: Graph::build(&env, target)?,
        layout,
        target,
        selection: s.selection(),
        backup_dir: p["backup_dir"].as_str().map(str::to_owned),
        chosen: None,
        error: None,
    })
}
fn valid(s: &Session, d: &Draft) -> Result<()> {
    if s.state()?.uid != d.uid || !Arc::ptr_eq(&s.state()?.doc, &d.baseline) {
        return Err(err("Il disegno è cambiato. Riapri Elimina tratto."));
    }
    Ok(())
}
fn endpoint(l: &mut Link, end: bool, e: End) {
    if end {
        l.to = None;
        l.end_tap = None;
        l.pe_x = None;
        l.end_node = None;
    } else {
        l.from = None;
        l.tap = None;
        l.pe_start = None;
        l.start_node = None;
    }
    match (end, e) {
        (false, End::Port(p)) => l.from = Some(p),
        (true, End::Port(p)) => l.to = Some(p),
        (false, End::Bar(t)) => l.tap = Some(t),
        (true, End::Bar(t)) => l.end_tap = Some(t),
        (false, End::Pe(x)) => l.pe_start = Some(x),
        (true, End::Pe(x)) => l.pe_x = Some(x),
        (false, End::Node(n)) => l.start_node = Some(n),
        (true, End::Node(n)) => l.end_node = Some(n),
    }
}
fn rewritten(d: &Draft, removed: &BTreeSet<usize>) -> Result<Drawing> {
    let mut draw = (*d.baseline).clone();
    for h in &d.graph.owners {
        editable(&draw, *h)?;
    }
    let chains = d.graph.chains(removed);
    if chains.len()
        + draw.model.iter().filter(|e| e.common.xdata.iter().any(|t| t.code == 1001 && t.value == APP) && !d.graph.owners.contains(&e.handle)).count()
        > 256
    {
        return Err(err("Troppi collegamenti risultanti."));
    }
    let seed = draw.new_handle().hex();
    let mut used = BTreeSet::new();
    let mut outputs = Vec::new();
    for (nodes, edges) in chains {
        let path = routing::simplify(nodes.iter().map(|i| d.graph.points[*i]).collect());
        if path.len() < 2 || path.len() > 34 {
            return Err(err("Il tratto risultante ha troppi vertici."));
        }
        let owners: BTreeSet<_> = edges.iter().flat_map(|i| d.graph.edges[*i].owners.iter().copied()).collect();
        let source = *owners.first().ok_or_else(|| err("Proprietario del tratto assente."))?;
        let target = owners.iter().find(|h| !used.contains(*h)).copied();
        if let Some(h) = target {
            used.insert(h);
        }
        let mut l = read_link(&d.baseline, source)?;
        l.version = 3;
        l.kind = "wire".into();
        l.bus = None;
        l.span = None;
        l.vertical_x = None;
        l.automatic = false;
        let get = |i: usize| {
            d.graph
                .ends
                .get(&i)
                .cloned()
                .unwrap_or_else(|| End::Node(Anchor { id: format!("section-{seed}-{i}"), at: [d.graph.points[i].x, d.graph.points[i].y] }))
        };
        endpoint(&mut l, false, get(*nodes.first().ok_or_else(|| err("Nodo assente."))?));
        endpoint(&mut l, true, get(*nodes.last().ok_or_else(|| err("Nodo assente."))?));
        l.waypoints = path.iter().skip(1).take(path.len() - 2).map(|p| [p.x, p.y]).collect();
        l.segment_modes = path
            .windows(2)
            .map(|s| {
                let delta = s[1] - s[0];
                if delta.x.abs() < 1e-7 || delta.y.abs() < 1e-7 {
                    tracing::RouteMode::Orthogonal
                } else if (delta.x.abs() - delta.y.abs()).abs() < 1e-7 {
                    tracing::RouteMode::Diagonal
                } else {
                    tracing::RouteMode::Free
                }
            })
            .collect();
        let geometry = path.windows(2).map(|s| line(s[0], s[1])).collect();
        let h = store(&mut draw, target, &l, geometry, path.clone())?;
        if target.is_none() {
            let data = placement::data(draw.entity(h).ok_or_else(|| err("Filo assente."))?, APP)?.ok_or_else(|| err("Dati assenti."))?;
            let original = d.baseline.entity(source).ok_or_else(|| err("Filo assente."))?.common.clone();
            draw.modify_entity(h, |e| {
                let layer = e.common.layer.clone();
                e.common = original;
                e.common.layer = layer;
                placement::tags(&mut e.common, APP, &data);
            })?;
        }
        outputs.push((h, path, owners));
    }
    // All affected markers are remapped by their original physical position,
    // independently of which old wire handle survives graph compression.
    let mut markers = Vec::new();
    let mut remove_markers = Vec::new();
    for e in d.baseline.model.iter() {
        if let Some(mut m) = placement::data(e, "CC_UNIFILARE_DEVICE")?
            && m["kind"] == "line_marker"
            && let Some(owner) = m["line"].as_str().and_then(Handle::parse_hex)
            && d.graph.owners.contains(&owner)
        {
            editable(&draw, e.handle)?;
            let at: [f64; 2] = serde_json::from_value(m["at"].clone()).map_err(|_| err("Posizione marcatore non valida."))?;
            let at = Vec2::new(at[0], at[1]);
            let found = outputs.iter().filter(|(_, _, owners)| owners.contains(&owner)).find_map(|(h, path, _)| {
                projection(path, at)
                    .filter(|(_, distance)| *distance < 1e-6)
                    .map(|(along, _)| (*h, along / path.windows(2).map(|s| s[0].dist(s[1])).sum::<f64>()))
            });
            if let Some((h, fraction)) = found {
                m["line"] = json!(h.hex());
                m["attachment"]["part"] = json!("route");
                m["attachment"]["fraction"] = json!(fraction);
                markers.push((e.handle, m));
            } else {
                remove_markers.push(e.handle);
            }
        }
    }
    for h in d.graph.owners.difference(&used).chain(remove_markers.iter()) {
        draw.remove_entity(*h);
    }
    for (h, m) in markers {
        draw.modify_entity(h, |e| placement::tags(&mut e.common, "CC_UNIFILARE_DEVICE", &m))?;
    }
    refresh(&mut draw, &d.layout)?;
    Ok(draw)
}
fn commit(s: &mut Session, d: &Draft) -> Result<Value> {
    valid(s, d)?;
    let (_, removed) = d.chosen.as_ref().ok_or_else(|| err("Scegli prima il tratto."))?;
    let draw = rewritten(d, removed)?;
    let saved = backup(s.doc()?, d.backup_dir.as_deref())?;
    *s.doc_mut()? = draw;
    s.unifilar_section = None;
    s.set_selection(vec![]);
    Ok(json!({"deleted_section":true,"backup":saved}))
}
fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let mut d = build(s, p)?;
    let at: [f64; 2] = serde_json::from_value(p["at"].clone()).map_err(|_| err("Indica at: [x,y]."))?;
    d.chosen = Some(d.graph.pick(d.target, Vec2::new(at[0], at[1]), 1.)?);
    commit(s, &d)
}
fn prepare(s: &mut Session, p: &Value) -> Result<Value> {
    s.unifilar_section = Some(build(s, p)?);
    s.set_selection(vec![]);
    Ok(json!({"prepared":true}))
}
pub(crate) fn cancel(s: &mut Session) {
    if let Some(d) = s.unifilar_section.take()
        && s.state().is_ok_and(|st| st.uid == d.uid)
    {
        s.set_selection(d.selection);
    }
}
pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("electrical.unifilar.section.delete", "Delete unifilar section", run).params("{model,handle?,at:[x,y],backup_dir?}"),
        CommandSpec::new("electrical.unifilar.section.prepare", "Prepare unifilar section deletion", prepare).noundo(),
        CommandSpec::new("electrical.unifilar.section.place", "Choose unifilar section", |_, _| Err(err("Avvia Elimina tratto dal menu.")))
            .enabled(|s| {
                s.unifilar_section
                    .as_ref()
                    .ok_or_else(|| "Scegli prima un collegamento.".to_string())
                    .and_then(|d| valid(s, d).map_err(|e| e.to_string()))
            })
            .interactive(|s| {
                if s.unifilar_section.is_none() {
                    return Err(err("Prepara la selezione del tratto."));
                }
                Ok(Box::new(Place))
            }),
    ]
}
struct Place;
impl Interactive for Place {
    fn name(&self) -> &'static str {
        "electrical.unifilar.section.place"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let chosen = s.unifilar_section.as_ref().is_some_and(Draft::chosen);
        Prompt::new(
            if chosen {
                "Controlla il tratto evidenziato; Invio conferma, Esc annulla"
            } else {
                "Clicca sul tratto da eliminare: termina alle connessioni; Esc annulla"
            },
            Accept::POINT,
        )
        .kw(&["Conferma", "Altro"])
    }
    fn input(&mut self, s: &mut Session, input: Input) -> Result<Step> {
        let mut d = s.unifilar_section.clone().ok_or_else(|| err("Selezione del tratto assente."))?;
        valid(s, &d)?;
        match input {
            Input::Point(p) => {
                let picked = d.graph.pick(d.target, p, (s.pixel_size() * s.settings.pickbox * 1.5).max(1.));
                match picked.and_then(|picked| {
                    rewritten(&d, &picked.1)?;
                    Ok(picked)
                }) {
                    Ok(picked) => {
                        d.chosen = Some(picked);
                        d.error = None;
                    }
                    Err(e) => {
                        d.chosen = None;
                        d.error = Some(e.to_string());
                    }
                }
                s.unifilar_section = Some(d);
                Ok(Step::Continue)
            }
            Input::Enter => {
                commit(s, &d)?;
                Ok(Step::Done)
            }
            Input::Keyword(k) if k.eq_ignore_ascii_case("conferma") => {
                commit(s, &d)?;
                Ok(Step::Done)
            }
            Input::Keyword(k) if k.eq_ignore_ascii_case("altro") => {
                d.chosen = None;
                d.error = None;
                s.unifilar_section = Some(d);
                Ok(Step::Continue)
            }
            Input::Cancel => Ok(Step::Cancel),
            _ => Ok(Step::Continue),
        }
    }
    fn preview(&self, s: &Session, p: Vec2) -> Vec<EntityKind> {
        let Some(d) = &s.unifilar_section else { return vec![] };
        if valid(s, d).is_err() {
            return vec![];
        }
        let picked = d.chosen.clone().or_else(|| d.graph.pick(d.target, p, (s.pixel_size() * s.settings.pickbox * 1.5).max(1.)).ok());
        picked
            .map(|(path, _)| {
                path.windows(2)
                    .filter_map(|p| {
                        let delta = p[1] - p[0];
                        let len = delta.len();
                        if len < 1e-7 {
                            return None;
                        }
                        let n = Vec2::new(-delta.y, delta.x) / len * (s.pixel_size() * 2.).max(0.08);
                        Some(EntityKind::Solid(Solid { corners: [(p[0] + n).to3(0.), (p[1] + n).to3(0.), (p[1] - n).to3(0.), (p[0] - n).to3(0.)] }))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}
