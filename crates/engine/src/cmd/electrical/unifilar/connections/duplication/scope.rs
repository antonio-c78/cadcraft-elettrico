//! Rooted downstream traversal and physical-axis clipping are independent scope policies.
use super::super::sections::graph::{End, Graph};
use super::*;
use std::collections::VecDeque;
#[derive(Clone)]
pub(super) struct Wire {
    pub link: Link,
    pub path: Vec<Vec2>,
    pub owners: BTreeSet<Handle>,
    pub original: Option<Handle>,
}
#[derive(Clone)]
pub(super) struct Plan {
    pub root: Handle,
    pub mode: String,
    pub devices: BTreeSet<Handle>,
    pub columns: BTreeSet<usize>,
    pub wires: Vec<Wire>,
}
fn meta(d: &Drawing, h: Handle) -> Result<Value> {
    placement::data(d.entity(h).ok_or_else(|| fail("Oggetto assente."))?, DEVICE)?.ok_or_else(|| fail("Dati apparecchio assenti."))
}
fn pe(l: &Link) -> bool {
    l.kind == "pe" || l.pe_start.is_some() || l.pe_x.is_some() || [l.from.as_ref(), l.to.as_ref()].into_iter().flatten().any(|p| p.port == "pe")
}

pub(super) fn build(d: &Drawing, env: &Env, root: Handle, column: usize, mode: &str) -> Result<Plan> {
    let (mut devices, wires) = if mode == "column" { column_scope(d, env, column)? } else { circuit(d, env, root)? };
    // Markers belong to retained physical wire segments, not merely to a table column.
    for e in d.model.iter() {
        if let Some(m) = placement::data(e, DEVICE)?
            && m["kind"] == "line_marker"
        {
            let owner = m["line"].as_str().and_then(Handle::parse_hex);
            let at: [f64; 2] = serde_json::from_value(m["at"].clone()).map_err(|_| fail("Posizione marcatore non valida."))?;
            if wires.iter().any(|w| {
                owner.is_some_and(|h| w.owners.contains(&h))
                    && (mode == "circuit"
                        || super::super::sections::graph::projection(&w.path, Vec2::new(at[0], at[1])).is_some_and(|(_, distance)| distance < 1e-6))
            }) {
                devices.insert(e.handle);
            }
        }
    }
    let mut columns = BTreeSet::new();
    for h in &devices {
        if let Some(c) = meta(d, *h)?["column"].as_u64().and_then(|n| usize::try_from(n).ok()) {
            columns.insert(c);
        }
    }
    if devices.len() > 256 || wires.len() > 256 || !devices.contains(&root) {
        return Err(fail("Ambito vuoto o troppo complesso."));
    }
    Ok(Plan { root, mode: mode.into(), devices, columns, wires })
}
fn circuit(d: &Drawing, env: &Env, root: Handle) -> Result<(BTreeSet<Handle>, Vec<Wire>)> {
    let node = env.nodes.get(&root).ok_or_else(|| fail("Apparecchio iniziale assente."))?;
    if !node.ports.contains_key("out") {
        return Err(fail("Scegli il dispositivo iniziale con un punto di uscita."));
    }
    let mut groups: BTreeMap<(String, String), Vec<Handle>> = BTreeMap::new();
    let mut ports = BTreeMap::new();
    let mut earth = BTreeSet::new();
    for (h, l) in &env.links {
        let net = network::net(l, &env.links)?;
        if pe(l) {
            earth.insert(net.clone());
        }
        for p in [l.from.as_ref(), l.to.as_ref()].into_iter().flatten() {
            ports.insert((p.handle.clone(), p.port.clone()), net.clone());
        }
        groups.entry(net).or_default().push(*h);
    }
    let upstream = ports.get(&(root.hex(), "in".into())).cloned();
    let mut queue = VecDeque::new();
    if let Some(net) = ports.get(&(root.hex(), "out".into())) {
        queue.push_back(net.clone());
    }
    let mut seen = BTreeSet::new();
    let mut devices = BTreeSet::from([root]);
    let mut wires = BTreeSet::new();
    while let Some(net) = queue.pop_front() {
        if !seen.insert(net.clone()) {
            continue;
        }
        if upstream.as_ref() == Some(&net) {
            return Err(fail("Il circuito ritorna sull'alimentazione a monte: il confine è ambiguo. Separa prima quel collegamento."));
        }
        if earth.contains(&net) {
            return Err(fail("La rete di alimentazione raggiunge il PE: verifica i collegamenti prima di duplicare."));
        }
        for h in groups.get(&net).into_iter().flatten() {
            wires.insert(*h);
            let l = env.links.get(h).ok_or_else(|| fail("Filo assente."))?;
            for p in [l.from.as_ref(), l.to.as_ref()].into_iter().flatten() {
                let h = handle(&p.handle)?;
                if h == root || !devices.insert(h) {
                    continue;
                }
                let e = d.entity(h).ok_or_else(|| fail("Apparecchio a valle assente."))?;
                if references::is_reference(e) {
                    continue;
                }
                for (id, _) in env.nodes.get(&h).into_iter().flat_map(|n| &n.ports).filter(|(id, _)| id.as_str() != "pe") {
                    if let Some(next) = ports.get(&(h.hex(), id.clone())) {
                        queue.push_back(next.clone());
                    }
                }
            }
        }
        if seen.len() > 256 || devices.len() > 256 {
            return Err(fail("Circuito troppo complesso."));
        }
    }
    // Retain only the selected devices' PE drops, without traversing the common PE network.
    for (h, l) in &env.links {
        if pe(l)
            && [l.from.as_ref(), l.to.as_ref()].into_iter().flatten().any(|p| Handle::parse_hex(&p.handle).is_some_and(|h| devices.contains(&h)))
            && [l.from.as_ref(), l.to.as_ref()].into_iter().flatten().all(|p| Handle::parse_hex(&p.handle).is_some_and(|h| devices.contains(&h)))
        {
            wires.insert(*h);
        }
    }
    let wires = wires
        .into_iter()
        .map(|h| {
            Ok(Wire {
                link: env.links.get(&h).cloned().ok_or_else(|| fail("Filo assente."))?,
                path: env.paths.get(&h).cloned().ok_or_else(|| fail("Percorso assente."))?,
                owners: BTreeSet::from([h]),
                original: Some(h),
            })
        })
        .collect::<Result<_>>()?;
    Ok((devices, wires))
}
fn column_scope(d: &Drawing, env: &Env, column: usize) -> Result<(BTreeSet<Handle>, Vec<Wire>)> {
    let axis = env.layout.columns.iter().find(|c| c.index == column).ok_or_else(|| fail("Colonna assente."))?.axis_x;
    let mut devices = BTreeSet::new();
    for e in d.model.iter() {
        if let Some(m) = placement::data(e, DEVICE)?
            && m["kind"] == "device"
            && m["column"].as_u64() == u64::try_from(column).ok()
        {
            devices.insert(e.handle);
        }
    }
    let mut visited: BTreeSet<Handle> = BTreeSet::new();
    let mut wires = Vec::new();
    for h in env.links.keys() {
        if visited.contains(h) || env.links.get(h).is_some_and(|l| l.kind == "bus") {
            continue;
        }
        let graph = Graph::build(env, *h)?;
        visited.extend(graph.owners.iter().copied());
        let mut kept: BTreeSet<_> = graph
            .edges
            .iter()
            .enumerate()
            .filter(|(_, e)| [e.a, e.b].into_iter().all(|i| graph.points.get(i).is_some_and(|p| (p.x - axis).abs() < 1e-6)))
            .map(|(i, _)| i)
            .collect();
        let protected: BTreeSet<_> = graph
            .ends
            .iter()
            .filter_map(
                |(i, end)| if let End::Port(p) = end { Handle::parse_hex(&p.handle).filter(|h| devices.contains(h)).map(|_| *i) } else { None },
            )
            .collect();
        let mut adjacent: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        for i in &kept {
            let e = graph.edges.get(*i).ok_or_else(|| fail("Tratto assente."))?;
            adjacent.entry(e.a).or_default().insert(*i);
            adjacent.entry(e.b).or_default().insert(*i);
        }
        let mut leaves: VecDeque<_> = adjacent.iter().filter(|(n, e)| e.len() < 2 && !protected.contains(n)).map(|(n, _)| *n).collect();
        while let Some(node) = leaves.pop_front() {
            if protected.contains(&node) {
                continue;
            }
            let Some(edge) = adjacent.get(&node).filter(|e| e.len() == 1).and_then(|e| e.first()).copied() else { continue };
            kept.remove(&edge);
            let e = graph.edges.get(edge).ok_or_else(|| fail("Tratto assente."))?;
            for n in [e.a, e.b] {
                if let Some(edges) = adjacent.get_mut(&n) {
                    edges.remove(&edge);
                    if edges.len() == 1 && !protected.contains(&n) {
                        leaves.push_back(n);
                    }
                }
            }
        }
        let removed = (0..graph.edges.len()).filter(|i| !kept.contains(i)).collect();
        let graph_id = id();
        for (nodes, edges) in graph.chains(&removed) {
            let path = routing::simplify(nodes.iter().filter_map(|i| graph.points.get(*i).copied()).collect());
            if path.len() < 2 {
                continue;
            }
            let owners: BTreeSet<_> = edges.iter().filter_map(|i| graph.edges.get(*i)).flat_map(|e| e.owners.iter().copied()).collect();
            let source = owners.first().and_then(|h| env.links.get(h)).ok_or_else(|| fail("Dati filo assenti."))?;
            let mut link = source.clone();
            link.version = 3;
            link.kind = "wire".into();
            link.bus = None;
            link.span = None;
            link.vertical_x = None;
            link.automatic = false;
            let end = |index: usize| -> Result<End> {
                if let Some(End::Port(p)) = graph.ends.get(&index)
                    && Handle::parse_hex(&p.handle).is_some_and(|h| devices.contains(&h))
                {
                    return Ok(End::Port(p.clone()));
                }
                let p = graph.points.get(index).ok_or_else(|| fail("Nodo assente."))?;
                Ok(End::Node(Anchor { id: format!("{graph_id}-{index}"), at: [p.x, p.y] }))
            };
            super::super::sections::endpoint(&mut link, false, end(*nodes.first().ok_or_else(|| fail("Nodo assente."))?)?);
            super::super::sections::endpoint(&mut link, true, end(*nodes.last().ok_or_else(|| fail("Nodo assente."))?)?);
            link.waypoints = path.iter().skip(1).take(path.len().saturating_sub(2)).map(|p| [p.x, p.y]).collect();
            link.segment_modes = vec![tracing::RouteMode::Orthogonal; path.len() - 1];
            wires.push(Wire { link, path, owners, original: None });
        }
    }
    Ok((devices, wires))
}
