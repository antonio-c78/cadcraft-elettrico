//! Standalone symbols share the atomic placement transaction, never the box frame.
use super::*;

#[derive(Clone, Deserialize, Serialize)]
pub struct Symbol {
    pub id: String,
    pub name: String,
    pub block: String,
    pub category: Vec<String>,
    pub factor: f64,
    pub width_mm: f64,
    pub height_mm: f64,
    pub anchor: [f64; 2],
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("electrical.symbol.read", "Read electrical symbol", super::edit::read_symbol).noundo(),
        CommandSpec::new("electrical.symbol.update", "Update electrical symbol", super::edit::update_symbol),
        CommandSpec::new("electrical.symbol.place", "Insert electrical symbol", |_, _| {
            Err(bad("electrical.symbol.place", "Prepara un simbolo, poi avvia il comando interattivo."))
        })
        .enabled(|s| if ready(s) { Ok(()) } else { Err("Prepara un simbolo dal catalogo.".into()) })
        .interactive(|s| {
            if !ready(s) {
                return Err(bad("electrical.symbol.place", "Prepara un simbolo."));
            }
            Ok(Box::new(Place { symbol: true }))
        }),
        CommandSpec::new("electrical.symbol.prepare", "Prepare electrical symbol", prepare)
            .noundo()
            .params("{library,symbol:{id,name,block,category,factor,width_mm,height_mm,anchor},backup_dir?}"),
        CommandSpec::new("electrical.symbol.insert", "Insert electrical symbol", insert)
            .params("{library,symbol:{id,name,block,category,factor,width_mm,height_mm,anchor},at:[x,y],rotation?,side?,height_cm?,backup_dir?}"),
    ]
}

fn ready(s: &Session) -> bool {
    s.electrical.as_ref().is_some_and(|p| p.is_symbol() && p.phase == 0 && s.state().is_ok_and(|st| st.uid == p.uid))
}

pub(super) fn prepare(s: &mut Session, p: &Value) -> Result<Value> {
    if s.running.is_some() || s.space() != Space::Model {
        return Err(bad("electrical.symbol.prepare", "Termina il comando e passa allo spazio modello."));
    }
    let symbol: Symbol =
        serde_json::from_value(p.get("symbol").cloned().unwrap_or(Value::Null)).map_err(|e| bad("electrical.symbol.prepare", e.to_string()))?;
    if [&symbol.id, &symbol.name, &symbol.block].iter().any(|v| v.is_empty() || v.len() > 255)
        || symbol.category.len() > 4
        || symbol.category.iter().any(|v| v.is_empty() || v.len() > 80)
        || !symbol.factor.is_finite()
        || !(0.000005..=3000.0).contains(&symbol.factor)
        || [symbol.width_mm, symbol.height_mm].iter().any(|v| !v.is_finite() || !(0.1..=200.0).contains(v))
        || symbol.anchor.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return Err(bad("electrical.symbol.prepare", "Proprietà del simbolo non valide."));
    }
    let path = p.get("library").and_then(Value::as_str).ok_or_else(|| bad("electrical.symbol.prepare", "Indica la libreria DXF."))?;
    let backup_dir = p.get("backup_dir").and_then(Value::as_str).map(str::to_string);
    if backup_dir.as_ref().is_some_and(|v| v.len() > 32768) {
        return Err(bad("electrical.symbol.prepare", "Percorso backup troppo lungo."));
    }
    let drawing = read_drawing(path)?;
    let block = drawing.block(&symbol.block).ok_or_else(|| bad("electrical.symbol.prepare", "Blocco assente dalla libreria."))?;
    let rough = cadcraft_render::build_entities(&drawing, block.entities.iter().map(|e| e.as_ref()), &Default::default());
    let bounds = rough.bounds;
    let size = bounds.width().max(bounds.height());
    if bounds.is_empty() || !size.is_finite() || !bounds.center().is_finite() {
        return Err(bad("electrical.symbol.prepare", "Simbolo privo di geometria valida."));
    }
    let list = cadcraft_render::build_entities(
        &drawing,
        block.entities.iter().map(|e| e.as_ref()),
        &cadcraft_render::Options { tolerance: (size / 800.0).max(1e-9), ..Default::default() },
    );
    let ratio = (symbol.width_mm / bounds.width().max(1e-9)).min(symbol.height_mm / bounds.height().max(1e-9));
    let anchor = Vec2::new(bounds.width() * symbol.anchor[0], bounds.height() * symbol.anchor[1]);
    let map = |p: Vec2| (p - bounds.min - anchor) * (ratio * symbol.factor);
    let mut geometry = Vec::new();
    for primitive in &list.prims {
        let start = primitive.start as usize;
        let end = start.checked_add(primitive.len as usize).ok_or_else(|| bad("electrical.symbol.prepare", "Geometria troppo complessa."))?;
        match primitive.kind {
            cadcraft_render::Kind::Polyline => {
                if let Some(points) = list.verts.get(start..end) {
                    for pair in points.windows(2) {
                        geometry.push(line(map(pair[0]), map(pair[1])));
                    }
                }
            }
            cadcraft_render::Kind::Tris => {
                if let Some(points) = list.tris.get(start..end) {
                    for t in points.as_chunks::<3>().0 {
                        geometry.extend([line(map(t[0]), map(t[1])), line(map(t[1]), map(t[2])), line(map(t[2]), map(t[0]))]);
                    }
                }
            }
            _ => {}
        }
        if geometry.len() > 20000 {
            return Err(bad("electrical.symbol.prepare", "Simbolo troppo complesso."));
        }
    }
    if geometry.is_empty() {
        return Err(bad("electrical.symbol.prepare", "Nessuna geometria inseribile."));
    }
    s.electrical = Some(Placement {
        layers: layers::Layers::parse(p.get("layers"))?,
        composition: Composition { capacity: 3, items: vec![], factor: symbol.factor, installation_height_cm: None },
        symbol: Some(symbol),
        point: None,
        rotation: 0.0,
        side: 0,
        phase: 0,
        error: None,
        uid: s.state()?.uid,
        geometry,
        geometry_rotation: 0.0,
        backup_dir,
        target: None,
    });
    Ok(json!({"prepared":true,"kind":"symbol"}))
}

fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    let at = p
        .get("at")
        .and_then(Value::as_array)
        .filter(|a| a.len() == 2)
        .and_then(|a| Some(Vec2::new(a.first()?.as_f64()?, a.get(1)?.as_f64()?)))
        .filter(|p| p.is_finite())
        .ok_or_else(|| bad("electrical.symbol.insert", "Indica at:[x,y]."))?;
    prepare(s, p)?;
    let result = (|| {
        options(s, p)?;
        if let Some(p) = &mut s.electrical {
            p.point = Some(at);
        }
        commit(s)
    })();
    s.electrical = None;
    result
}
