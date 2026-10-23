//! Column relocation preserves explicit anchors before rebuilding dependent routes.
use super::*;

fn fraction(path: &[Vec2], at: Vec2) -> Result<f64> {
    let total: f64 = path.windows(2).map(|s| s[0].dist(s[1])).sum();
    let mut before = 0.;
    for s in path.windows(2) {
        let v = s[1] - s[0];
        let n = v.len();
        if n > 1e-7 {
            let t = (at - s[0]).dot(v) / (n * n);
            if (-1e-7..=1. + 1e-7).contains(&t) && (s[0] + v * t).near(at, 1e-6) && total > 1e-7 {
                return Ok(((before + t.clamp(0., 1.) * n) / total).clamp(0., 1.));
            }
        }
        before += n;
    }
    Err(err("Una giunzione non può seguire il percorso. Modifica prima quel collegamento."))
}

/// Include taps as vertices so stretching a terminal leg does not slide a junction.
fn anchored(path: &[Vec2], anchors: &[Vec2]) -> Vec<Vec2> {
    let mut out = Vec::new();
    for s in path.windows(2) {
        out.push(s[0]);
        let v = s[1] - s[0];
        let n = v.dot(v);
        if n < 1e-12 {
            continue;
        }
        let mut points: Vec<_> = anchors
            .iter()
            .filter_map(|p| {
                let t = (*p - s[0]).dot(v) / n;
                (t > 1e-7 && t < 1. - 1e-7 && (s[0] + v * t).near(*p, 1e-6)).then_some((t, *p))
            })
            .collect();
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        out.extend(points.into_iter().map(|v| v.1));
    }
    out.extend(path.last().copied());
    out.dedup_by(|a, b| a.near(*b, 1e-7));
    out
}

fn stretched(path: &[Vec2], a: Vec2, b: Vec2) -> Vec<Vec2> {
    let Some(first) = path.first() else { return vec![] };
    let Some(last) = path.last() else { return vec![] };
    let da = a - *first;
    let db = b - *last;
    let prefix = path.iter().take_while(|p| (p.x - first.x).abs() < 1e-7).count();
    let suffix = path.iter().rev().take_while(|p| (p.x - last.x).abs() < 1e-7).count();
    path.iter()
        .enumerate()
        .map(|(i, p)| {
            if i == 0 {
                a
            } else if i + 1 == path.len() {
                b
            } else if da.near(db, 1e-7) || (i < prefix && da.len() > 1e-7) {
                *p + da
            } else if i >= path.len().saturating_sub(suffix) && db.len() > 1e-7 {
                *p + db
            } else {
                *p
            }
        })
        .collect()
}

fn waypoints(l: &mut Link, path: &[Vec2]) -> Result<()> {
    l.waypoints = path.iter().skip(1).take(path.len().saturating_sub(2)).map(|p| [p.x, p.y]).collect();
    if l.waypoints.len() > 32 {
        return Err(err("Troppi punti di passaggio per spostare il circuito."));
    }
    // Keep each leg constrained when a later device edit changes its endpoint.
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
    Ok(())
}

/// If stretching hits another column, reroute around its bodies and labels.
/// Explicit junctions/markers remain mandatory anchors; optional old bends may change.
fn reroute(env: &Env, l: &mut Link, mandatory: &[Vec2]) -> Result<()> {
    let a = env.start(l)?;
    let b = env.end(l)?;
    let ea = l.from.as_ref().map(|p| env.stub(p, "any").map(|v| v.1)).transpose()?.unwrap_or(a);
    let eb = l.to.as_ref().map(|p| env.stub(p, "any").map(|v| v.1)).transpose()?.unwrap_or(b);
    let mut stops = vec![ea];
    stops.extend(mandatory.iter().copied().filter(|p| !p.near(a, 1e-7) && !p.near(b, 1e-7)));
    stops.push(eb);
    let mut area = env.layout.drawing_area;
    area[1] = env.pe()?.0;
    area[3] -= 1.;
    let boxes: Vec<_> = env.boxes.iter().map(|o| o.rect).collect();
    let lanes: Vec<_> = env.layout.columns.iter().flat_map(|c| [c.bounds[0] + 2., c.bounds[2] - 2.]).collect();
    let root = net(l, &env.links)?;
    let wires: Vec<_> = env.wires.iter().filter(|(h, _, _)| env.roots.get(h) != Some(&root)).map(|(_, a, b)| (*a, *b)).collect();
    let mut path = vec![a, ea];
    for segment in stops.windows(2) {
        if !segment[0].near(segment[1], 1e-7) {
            path.extend(
                routing::route(segment[0], segment[1], (segment[0].y + segment[1].y) * 0.5, area, &boxes, &lanes, &wires)?.into_iter().skip(1),
            );
        }
    }
    path.push(b);
    waypoints(l, &routing::simplify(path))
}

fn foreign_overlap(env: &Env, l: &Link, path: &[Vec2]) -> Result<bool> {
    let root = net(l, &env.links)?;
    Ok(path.windows(2).any(|s| env.wires.iter().any(|(h, a, b)| env.roots.get(h) != Some(&root) && routing::overlaps_wire(s[0], s[1], *a, *b))))
}

/// Move only devices in the selected column. Shared bars and foreign terminals stay fixed.
pub(crate) fn relocate(
    before: &Drawing,
    drawing: &mut Drawing,
    layout: &schema::Layout,
    moved: &BTreeSet<Handle>,
    dx: f64,
) -> Result<BTreeSet<Handle>> {
    let old = Env::new(before, layout)?;
    let mut env = Env::new(drawing, layout)?;
    let mut changed = BTreeSet::new();
    let mut positions: BTreeMap<Handle, Vec<(Vec2, Vec2)>> = BTreeMap::new();
    let mut anchors: BTreeMap<Handle, Vec<Vec2>> = BTreeMap::new();
    for l in old.links.values() {
        for t in [l.tap.as_ref(), l.end_tap.as_ref()].into_iter().flatten().filter(|t| t.part == "route") {
            anchors.entry(handle(&t.wire)?).or_default().push(old.tap_point(t)?.0);
        }
    }
    let mut markers = Vec::new();
    for e in before.model.iter() {
        if let Some(m) = placement::data(e, "CC_UNIFILARE_DEVICE")?
            && m["kind"] == "line_marker"
            && m["attachment"]["part"] == "route"
            && let Some(owner) = m["line"].as_str().and_then(Handle::parse_hex)
        {
            let t: Tap = serde_json::from_value(json!({"wire":owner.hex(),"part":"route","fraction":m["attachment"]["fraction"]}))
                .map_err(|_| err("Marcatore non valido."))?;
            let at = old.tap_point(&t)?.0;
            anchors.entry(owner).or_default().push(at);
            markers.push((e.handle, owner, at));
        }
        if moved.contains(&e.handle)
            && let Some(m) = placement::data(e, "CC_UNIFILARE_DEVICE")?
            && m["kind"] == "line_marker"
            && m["attachment"]["part"] == "bar"
            && let Some(owner) = m["line"].as_str()
        {
            let (_, span) = env.bus(owner)?;
            let x = m["at"].get(0).and_then(Value::as_f64).ok_or_else(|| err("Posizione del marcatore non valida."))? + dx;
            if x < span[0] || x > span[1] {
                return Err(err("La barra comune non raggiunge la colonna del marcatore."));
            }
            let mut meta = placement::data(drawing.entity(e.handle).ok_or_else(|| err("Marcatore assente."))?, "CC_UNIFILARE_DEVICE")?
                .ok_or_else(|| err("Dati del marcatore assenti."))?;
            meta["attachment"]["fraction"] = json!((x - span[0]) / (span[1] - span[0]));
            drawing.modify_entity(e.handle, |e| placement::tags(&mut e.common, "CC_UNIFILARE_DEVICE", &meta))?;
        }
    }
    for h in network::order(&old.links)? {
        let original = old.links.get(&h).ok_or_else(|| err("Collegamento assente."))?;
        let mut l = original.clone();
        let path = old.paths.get(&h).ok_or_else(|| err("Percorso assente."))?;
        if l.kind == "bus" {
            // The common distribution span never follows one circuit's column.
            if l.from.as_ref().is_some_and(|p| Handle::parse_hex(&p.handle).is_some_and(|h| moved.contains(&h))) {
                editable(drawing, h)?;
                let (g, p) = env.geometry(&l)?;
                env.wires.retain(|(owner, _, _)| *owner != h);
                env.wires.extend(g.iter().filter_map(|k| if let EntityKind::Line(v) = k { Some((h, v.a.xy(), v.b.xy())) } else { None }));
                store(drawing, Some(h), &l, g, p.clone())?;
                env.paths.insert(h, p);
                changed.insert(h);
            }
            continue;
        }
        tracing::upgrade(&old, &mut l, Some(h))?;
        let upgraded = serde_json::to_value(&l).map_err(|e| err(e.to_string()))?;
        let start_slide = l.to.as_ref().is_some_and(|p| Handle::parse_hex(&p.handle).is_some_and(|h| moved.contains(&h)));
        let end_slide = l.from.as_ref().is_some_and(|p| Handle::parse_hex(&p.handle).is_some_and(|h| moved.contains(&h)));
        for (t, slide, opposite) in [(&mut l.tap, start_slide, path.last()), (&mut l.end_tap, end_slide, path.first())] {
            let Some(t) = t else { continue };
            if t.part == "bar" {
                let at = old.tap_point(t)?.0;
                if slide && opposite.is_some_and(|p| (p.x - at.x).abs() < 1e-6) {
                    let (_, span) = env.bus(&t.wire)?;
                    let x = at.x + dx;
                    if x < span[0] || x > span[1] {
                        return Err(err("La barra comune non raggiunge la colonna scelta."));
                    }
                    t.fraction = (x - span[0]) / (span[1] - span[0]);
                }
                continue;
            }
            let owner = handle(&t.wire)?;
            let at = old.tap_point(t)?.0;
            if let Some(mapped) = positions.get(&owner).and_then(|v| v.iter().find(|(p, _)| p.near(at, 1e-6))) {
                t.fraction = fraction(env.paths.get(&owner).ok_or_else(|| err("Percorso della giunzione assente."))?, mapped.1)?;
            }
        }
        let a = env.start(&l)?;
        // A dedicated PE lead follows horizontally along the fixed PE bar.
        if l.pe_x.is_some() && path.first().is_some_and(|p| !a.near(*p, 1e-7)) {
            l.pe_x = l.pe_x.map(|x| x + dx);
        }
        if l.pe_start.is_some() && l.to.as_ref().is_some_and(|p| Handle::parse_hex(&p.handle).is_some_and(|h| moved.contains(&h))) {
            l.pe_start = l.pe_start.map(|x| x + dx);
        }
        let a = env.start(&l)?;
        let b = env.end(&l)?;
        let augmented = anchored(path, anchors.get(&h).map(Vec::as_slice).unwrap_or_default());
        let desired = stretched(&augmented, a, b);
        let affects =
            augmented.iter().zip(&desired).any(|(p, q)| !p.near(*q, 1e-7)) || serde_json::to_value(&l).map_err(|e| err(e.to_string()))? != upgraded;
        if !affects {
            continue;
        }
        editable(drawing, h)?;
        let mut route = Vec::new();
        for (index, segment) in desired.windows(2).enumerate() {
            let diagonal = (segment[1].x - segment[0].x).abs() > 1e-7 && (segment[1].y - segment[0].y).abs() > 1e-7;
            let rigid = augmented.windows(2).nth(index).is_some_and(|s| (segment[0] - s[0]).near(segment[1] - s[1], 1e-7));
            let mode = if diagonal && rigid { tracing::RouteMode::Free } else { tracing::RouteMode::Orthogonal };
            let leg = tracing::leg(segment[0], segment[1], mode, false);
            if route.is_empty() {
                route.extend(leg);
            } else {
                route.extend(leg.into_iter().skip(1));
            }
        }
        route = routing::simplify(route);
        waypoints(&mut l, &route)?;
        env.links.insert(h, l.clone());
        env.wires.retain(|(owner, _, _)| *owner != h);
        let direct = env.geometry(&l);
        let valid = match &direct {
            Ok((_, path)) => !foreign_overlap(&env, &l, path)?,
            Err(_) => false,
        };
        let (g, new_path) = if valid {
            direct?
        } else {
            let mandatory: Vec<_> = augmented
                .iter()
                .zip(&desired)
                .filter(|(old, _)| anchors.get(&h).is_some_and(|points| points.iter().any(|p| p.near(**old, 1e-6))))
                .map(|(_, new)| *new)
                .collect();
            reroute(&env, &mut l, &mandatory).map_err(|_| {
                err("Il circuito non entra nella destinazione: un collegamento interferisce con simboli o testi. Scegli un'altra colonna.")
            })?;
            let value = env.geometry(&l)?;
            if foreign_overlap(&env, &l, &value.1)? {
                return Err(err("Non c'è un percorso libero dai fili degli altri circuiti. Scegli un'altra colonna."));
            }
            value
        };
        env.links.insert(h, l.clone());
        positions.insert(h, augmented.iter().copied().zip(desired).collect());
        env.paths.insert(h, new_path.clone());
        env.wires.extend(g.iter().filter_map(|k| if let EntityKind::Line(v) = k { Some((h, v.a.xy(), v.b.xy())) } else { None }));
        store(drawing, Some(h), &l, g, new_path)?;
        changed.insert(h);
    }
    for (h, owner, at) in markers {
        if let Some(mapped) = positions.get(&owner).and_then(|v| v.iter().find(|(p, _)| p.near(at, 1e-6))) {
            let mut meta = placement::data(drawing.entity(h).ok_or_else(|| err("Marcatore assente."))?, "CC_UNIFILARE_DEVICE")?
                .ok_or_else(|| err("Dati del marcatore assenti."))?;
            meta["attachment"]["fraction"] = json!(fraction(env.paths.get(&owner).ok_or_else(|| err("Percorso assente."))?, mapped.1)?);
            drawing.modify_entity(h, |e| placement::tags(&mut e.common, "CC_UNIFILARE_DEVICE", &meta))?;
        }
    }
    refresh(drawing, layout)?;
    Ok(changed)
}
