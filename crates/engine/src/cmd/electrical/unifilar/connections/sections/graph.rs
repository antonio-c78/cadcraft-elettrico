//! Bounded physical graph: explicit attachments join vertices; mere crossings do not.
use super::*;
#[derive(Clone)]
pub(super) enum End {
    Port(Port),
    Bar(Tap),
    Pe(f64),
    Node(Anchor),
}
#[derive(Clone)]
pub(super) struct Edge {
    pub a: usize,
    pub b: usize,
    pub owners: BTreeSet<Handle>,
}
#[derive(Clone)]
pub(super) struct Graph {
    pub points: Vec<Vec2>,
    pub ends: BTreeMap<usize, End>,
    pub edges: Vec<Edge>,
    pub owners: BTreeSet<Handle>,
}
fn root(parent: &[usize], mut i: usize) -> usize {
    for _ in 0..parent.len() {
        let Some(next) = parent.get(i) else { break };
        if *next == i {
            break;
        }
        i = *next;
    }
    i
}
fn join(parent: &mut [usize], a: usize, b: usize) {
    let a = root(parent, a);
    let b = root(parent, b);
    if let Some(p) = parent.get_mut(a.max(b)) {
        *p = a.min(b);
    }
}
pub(super) fn projection(path: &[Vec2], p: Vec2) -> Option<(f64, f64)> {
    let mut before = 0.;
    let mut best = None;
    for s in path.windows(2) {
        let d = s[1] - s[0];
        let len = d.len();
        if len < 1e-7 {
            continue;
        }
        let t = ((p - s[0]).dot(d) / (len * len)).clamp(0., 1.);
        let distance = (s[0] + d * t).dist(p);
        if best.is_none_or(|(_, old)| distance < old) {
            best = Some((before + t * len, distance));
        }
        before += len;
    }
    best
}
fn on(a: Vec2, b: Vec2, p: Vec2) -> bool {
    projection(&[a, b], p).is_some_and(|(_, d)| d < 1e-6)
}
impl Graph {
    pub fn build(env: &Env, selected: Handle) -> Result<Self> {
        let selected_link = env.links.get(&selected).ok_or_else(|| err("Seleziona un filo."))?;
        if selected_link.kind == "bus" {
            return Err(err("Per la barra usa il comando dedicato."));
        }
        let component = net(selected_link, &env.links)?;
        let owners: BTreeSet<_> = env
            .links
            .iter()
            .filter(|(_, l)| l.kind != "bus")
            .filter_map(|(h, l)| (net(l, &env.links).ok().as_ref() == Some(&component)).then_some(*h))
            .collect();
        let mut paths = BTreeMap::new();
        let mut links = BTreeMap::new();
        for h in &owners {
            let path = env.paths.get(h).cloned().ok_or_else(|| err("Percorso assente."))?;
            let mut link = env.links.get(h).cloned().ok_or_else(|| err("Filo assente."))?;
            tracing::upgrade(env, &mut link, Some(*h))?;
            paths.insert(*h, path);
            links.insert(*h, link);
        }
        let segment_count: usize = paths.values().map(|p| p.len().saturating_sub(1)).sum();
        if segment_count > 2048 {
            return Err(err("Rete troppo complessa per la modifica dei tratti."));
        }
        let mut cuts: BTreeMap<Handle, Vec<(f64, Vec2)>> = BTreeMap::new();
        for (h, path) in &paths {
            let mut along = 0.;
            let mut cut = Vec::new();
            for (i, p) in path.iter().enumerate() {
                if let Some(previous) = i.checked_sub(1).and_then(|j| path.get(j)) {
                    along += p.dist(*previous);
                }
                cut.push((along, *p));
            }
            cuts.insert(*h, cut);
        }
        for l in links.values() {
            for t in [l.tap.as_ref(), l.end_tap.as_ref()].into_iter().flatten().filter(|t| t.part == "route") {
                if let Some(points) = cuts.get_mut(&handle(&t.wire)?) {
                    let length: f64 = paths.get(&handle(&t.wire)?).into_iter().flat_map(|p| p.windows(2)).map(|s| s[0].dist(s[1])).sum();
                    points.push((t.fraction * length, env.tap_point(t)?.0));
                }
            }
        }
        // Collinear shared runs are one physical conductor. Subdivide at both ends
        // of each overlap, without introducing nodes at perpendicular crossings.
        let segments: Vec<_> = paths
            .iter()
            .flat_map(|(h, p)| {
                let mut along = 0.;
                p.windows(2).map(move |s| {
                    let before = along;
                    along += s[0].dist(s[1]);
                    (*h, s[0], s[1], before)
                })
            })
            .collect();
        for (i, (h, a, b, before)) in segments.iter().enumerate() {
            for (k, c, d, other_before) in segments.iter().skip(i + 1) {
                if h == k || !routing::overlaps_wire(*a, *b, *c, *d) {
                    continue;
                }
                for p in [*c, *d] {
                    if on(*a, *b, p)
                        && let Some(points) = cuts.get_mut(h)
                    {
                        points.push((*before + a.dist(p), p));
                    }
                }
                for p in [*a, *b] {
                    if on(*c, *d, p)
                        && let Some(points) = cuts.get_mut(k)
                    {
                        points.push((*other_before + c.dist(p), p));
                    }
                }
            }
        }
        let mut points = Vec::new();
        let mut indices: BTreeMap<Handle, Vec<usize>> = BTreeMap::new();
        let mut distances: BTreeMap<Handle, Vec<f64>> = BTreeMap::new();
        for (h, mut at) in cuts {
            at.sort_by(|a, b| a.0.total_cmp(&b.0));
            at.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-6);
            let mut row = Vec::new();
            let mut distance = Vec::new();
            for (along, p) in at {
                distance.push(along);
                row.push(points.len());
                points.push(p);
            }
            indices.insert(h, row);
            distances.insert(h, distance);
        }
        if points.len() > 8192 {
            return Err(err("Troppi nodi nella rete."));
        }
        let mut parent: Vec<_> = (0..points.len()).collect();
        let mut bindings = Vec::new();
        let mut terminals: BTreeMap<String, usize> = BTreeMap::new();
        for (h, l) in &links {
            let row = indices.get(h).ok_or_else(|| err("Nodi assenti."))?;
            for (end, i) in [(false, row.first()), (true, row.last())] {
                let i = *i.ok_or_else(|| err("Percorso vuoto."))?;
                let port = if end { &l.to } else { &l.from };
                let tap = if end { &l.end_tap } else { &l.tap };
                let node = if end { &l.end_node } else { &l.start_node };
                let pe = if end { l.pe_x } else { l.pe_start };
                let (key, binding) = if let Some(p) = port {
                    (Some(format!("P:{}:{}", p.handle, p.port)), Some(End::Port(p.clone())))
                } else if let Some(n) = node {
                    (Some(format!("N:{}", n.id)), Some(End::Node(n.clone())))
                } else if let Some(x) = pe {
                    (None, Some(End::Pe(x)))
                } else if let Some(t) = tap {
                    if t.part == "route"
                        && let Some(other) = indices.get(&handle(&t.wire)?)
                    {
                        let distance = distances.get(&handle(&t.wire)?).ok_or_else(|| err("Distanze assenti."))?;
                        let at = t.fraction * distance.last().copied().ok_or_else(|| err("Percorso vuoto."))?;
                        let j = other
                            .iter()
                            .zip(distance)
                            .find(|(_, n)| (**n - at).abs() < 1e-6)
                            .map(|(i, _)| *i)
                            .ok_or_else(|| err("Nodo della derivazione assente."))?;
                        join(&mut parent, i, j);
                        (None, None)
                    } else {
                        (None, Some(End::Bar(t.clone())))
                    }
                } else {
                    return Err(err("Estremità del filo assente."));
                };
                if let Some(key) = key
                    && let Some(j) = terminals.insert(key, i)
                {
                    if !points[i].near(points[j], 1e-6) {
                        return Err(err("Posizioni discordanti della giunzione."));
                    }
                    join(&mut parent, i, j);
                }
                if let Some(binding) = binding {
                    bindings.push((i, binding));
                }
            }
        }
        let raw: Vec<_> = indices.iter().flat_map(|(h, row)| row.windows(2).map(move |v| (*h, v[0], v[1]))).collect();
        for (i, (h, a, b)) in raw.iter().enumerate() {
            for (k, c, d) in raw.iter().skip(i + 1) {
                if h == k {
                    continue;
                }
                if points[*a].near(points[*c], 1e-6) && points[*b].near(points[*d], 1e-6) {
                    join(&mut parent, *a, *c);
                    join(&mut parent, *b, *d);
                } else if points[*a].near(points[*d], 1e-6) && points[*b].near(points[*c], 1e-6) {
                    join(&mut parent, *a, *d);
                    join(&mut parent, *b, *c);
                }
            }
        }
        let mut ends = BTreeMap::new();
        for (i, binding) in bindings {
            let i = root(&parent, i);
            if !matches!(ends.get(&i), Some(End::Port(_) | End::Pe(_) | End::Bar(_))) {
                ends.insert(i, binding);
            }
        }
        let mut unique: BTreeMap<(usize, usize), BTreeSet<Handle>> = BTreeMap::new();
        for (h, a, b) in raw {
            let a = root(&parent, a);
            let b = root(&parent, b);
            if a != b {
                unique.entry((a.min(b), a.max(b))).or_default().insert(h);
            }
        }
        let edges = unique.into_iter().map(|((a, b), owners)| Edge { a, b, owners }).collect();
        Ok(Self { points, ends, edges, owners })
    }
    fn adjacency(&self, removed: &BTreeSet<usize>) -> BTreeMap<usize, Vec<usize>> {
        let mut out: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (i, e) in self.edges.iter().enumerate().filter(|(i, _)| !removed.contains(i)) {
            out.entry(e.a).or_default().push(i);
            out.entry(e.b).or_default().push(i);
        }
        out
    }
    fn stop(&self, node: usize, adj: &BTreeMap<usize, Vec<usize>>) -> bool {
        adj.get(&node).is_none_or(|v| v.len() != 2) || matches!(self.ends.get(&node), Some(End::Port(_) | End::Pe(_) | End::Bar(_)))
    }
    fn walk(&self, first: usize, start: usize, adj: &BTreeMap<usize, Vec<usize>>, seen: &mut BTreeSet<usize>) -> (Vec<usize>, Vec<usize>) {
        let mut nodes = vec![start];
        let mut used = Vec::new();
        let mut edge = first;
        let mut at = start;
        for _ in 0..=self.edges.len() {
            if !seen.insert(edge) {
                break;
            }
            used.push(edge);
            let Some(e) = self.edges.get(edge) else { break };
            at = if e.a == at { e.b } else { e.a };
            nodes.push(at);
            if self.stop(at, adj) {
                break;
            }
            let Some(next) = adj.get(&at).and_then(|v| v.iter().find(|i| !seen.contains(i))).copied() else { break };
            edge = next;
        }
        (nodes, used)
    }
    pub fn chains(&self, removed: &BTreeSet<usize>) -> Vec<(Vec<usize>, Vec<usize>)> {
        let adj = self.adjacency(removed);
        let mut seen = removed.clone();
        let mut out = Vec::new();
        for (&node, edges) in &adj {
            if self.stop(node, &adj) {
                for e in edges {
                    if !seen.contains(e) {
                        out.push(self.walk(*e, node, &adj, &mut seen));
                    }
                }
            }
        }
        for (i, e) in self.edges.iter().enumerate() {
            if !seen.contains(&i) {
                out.push(self.walk(i, e.a, &adj, &mut seen));
            }
        }
        out
    }
    pub fn pick(&self, h: Handle, p: Vec2, tol: f64) -> Result<(Vec<Vec2>, BTreeSet<usize>)> {
        let nearest = self
            .edges
            .iter()
            .enumerate()
            .filter(|(_, e)| e.owners.contains(&h))
            .filter_map(|(i, e)| projection(&[self.points[e.a], self.points[e.b]], p).map(|(_, d)| (i, d)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .filter(|(_, d)| *d <= tol)
            .ok_or_else(|| err("Clicca sul tratto del filo da eliminare."))?
            .0;
        let (nodes, edges) = self.chains(&BTreeSet::new()).into_iter().find(|(_, e)| e.contains(&nearest)).ok_or_else(|| err("Tratto assente."))?;
        Ok((nodes.into_iter().map(|i| self.points[i]).collect(), edges.into_iter().collect()))
    }
}
