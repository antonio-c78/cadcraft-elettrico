//! Read native composition snapshots and replace one reference, never a shared definition.
use super::*;
use crate::cmd::machines::{SelOutcome, SelectPhase};
use cadcraft_doc::Handle;

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("electrical.read", "Read electrical box", read).noundo(),
        CommandSpec::new("electrical.edit.prepare", "Prepare electrical box edit", prepare_edit).noundo(),
        CommandSpec::new("electrical.update", "Update electrical box", update),
        CommandSpec::new("electrical.select", "Select electrical box", read)
            .noundo()
            .interactive(|_| Ok(Box::new(Select { phase: SelectPhase::single() }))),
    ]
}

fn target(s: &Session, p: &Value) -> Result<Handle> {
    let handles = crate::cmd::targets(s, p)?;
    if handles.len() != 1 {
        return Err(bad("electrical.read", "Seleziona una sola scatola elettrica."));
    }
    handles.first().copied().ok_or_else(|| bad("electrical.read", "Seleziona una scatola."))
}

fn read(s: &mut Session, p: &Value) -> Result<Value> {
    let h = target(s, p)?;
    let d = s.doc()?;
    if s.space() != Space::Model || d.space_of(h) != Some(Space::Model) {
        return Err(bad("electrical.read", "La modifica richiede lo spazio modello."));
    }
    let e = d.entity(h).ok_or_else(|| bad("electrical.read", "Scatola assente."))?;
    let EntityKind::Insert(ins) = &e.kind else {
        return Err(bad("electrical.read", "L'oggetto non è una scatola del compositore."));
    };
    let block = d.block(&ins.block).ok_or_else(|| bad("electrical.read", "Blocco assente."))?;
    if ins.rows != 1
        || ins.cols != 1
        || !ins.attribs.is_empty()
        || !ins.insert.is_finite()
        || ins.insert.z != 0.0
        || !ins.rotation.is_finite()
        || ins.rotation.abs() > 1e6
        || !ins.scale.x.is_finite()
        || ins.scale.x <= 0.0
        || ins.scale.x != ins.scale.y
        || block.base != Vec3::ZERO
        || block.entities.len() > 20001
    {
        return Err(bad("electrical.read", "Trasformazione non supportata: usa una scatola senza specchiatura, scala non uniforme o serie."));
    }
    let mut active = false;
    let mut text = String::new();
    for tag in &e.common.xdata {
        if tag.code == 1001 {
            active = tag.value == "CC_COMPOSITORE";
        } else if active && tag.code == 1000 {
            if text.len().saturating_add(tag.value.len()) > 32768 {
                return Err(bad("electrical.read", "Dati scatola troppo grandi."));
            }
            text.push_str(&tag.value);
        }
    }
    let mut metadata: Value = serde_json::from_str(&text).map_err(|_| bad("electrical.read", "Mancano i dati del compositore nativo."))?;
    if metadata["version"] != 1 {
        return Err(bad("electrical.read", "Versione scatola non supportata."));
    }
    let mut c: Composition = serde_json::from_value(metadata["composition"].clone()).map_err(|e| bad("electrical.read", e.to_string()))?;
    validate(&c)?;
    let text_count = block.entities.iter().filter(|e| matches!(&e.kind, EntityKind::Text(_))).count();
    if text_count != usize::from(c.installation_height_cm.is_some())
        || block.entities.iter().any(|e| match &e.kind {
            EntityKind::Line(l) => !l.a.is_finite() || !l.b.is_finite(),
            EntityKind::Text(t) => c.installation_height_cm.is_none_or(|h| t.value != format!("H{h}")),
            _ => true,
        })
    {
        return Err(bad("electrical.read", "La geometria del blocco è stata modificata manualmente e non è compatibile."));
    }
    c.factor *= ins.scale.x;
    validate(&c)?;
    let rotation = metadata["rotation"]
        .as_f64()
        .filter(|r| r.is_finite() && r.abs() <= 360000.0)
        .ok_or_else(|| bad("electrical.read", "Rotazione salvata non valida."))?;
    let side = match metadata["installation_label_position"].as_str() {
        Some("sopra") => 0,
        Some("sotto") => 1,
        Some("destra") => 2,
        Some("sinistra") => 3,
        _ => return Err(bad("electrical.read", "Posizione testo non valida.")),
    };
    metadata["composition"] = json!(c);
    metadata["rotation"] = json!((rotation + ins.rotation.to_degrees()).rem_euclid(360.0));
    metadata["side"] = json!(side);
    metadata["at"] = json!([ins.insert.x, ins.insert.y]);
    metadata["handle"] = json!(h.hex());
    metadata["uid"] = json!(s.state()?.uid);
    metadata["revision"] = json!(s.state()?.revision);
    metadata["baked_rotation"] = json!(rotation);
    Ok(metadata)
}

fn prepare_edit(s: &mut Session, p: &Value) -> Result<Value> {
    if s.running.is_some() {
        return Err(bad("electrical.update", "Termina il comando corrente."));
    }
    let old = read(s, p)?;
    if p["uid"].as_u64() != old["uid"].as_u64() || p["revision"].as_u64() != old["revision"].as_u64() {
        return Err(bad("electrical.update", "Il disegno è cambiato. Riapri Modifica scatola."));
    }
    let h = target(s, p)?;
    let c: Composition = serde_json::from_value(p.get("composition").cloned().unwrap_or_else(|| old["composition"].clone()))
        .map_err(|e| bad("electrical.update", e.to_string()))?;
    validate(&c)?;
    let mut previous = old["composition"].clone();
    previous["installation_height_cm"] = json!(c.installation_height_cm);
    let (geometry, geometry_rotation) = if previous == json!(c) {
        let d = s.doc()?;
        let e = d.entity(h).ok_or_else(|| bad("electrical.update", "Scatola assente."))?;
        let EntityKind::Insert(ins) = &e.kind else {
            return Err(bad("electrical.update", "Scatola non valida."));
        };
        let b = d.block(&ins.block).ok_or_else(|| bad("electrical.update", "Blocco assente."))?;
        let mut geometry = Vec::new();
        for entity in b.entities.iter() {
            match &entity.kind {
                EntityKind::Line(_) => {
                    let mut kind = entity.kind.clone();
                    kind.transform(&Mat3::scale(ins.scale.x, ins.scale.x));
                    geometry.push(kind);
                }
                EntityKind::Text(_) => {}
                _ => return Err(bad("electrical.update", "Il blocco è stato modificato manualmente e non è compatibile.")),
            }
        }
        (geometry, old["baked_rotation"].as_f64().unwrap_or(0.0))
    } else {
        let library = p["library"].as_str().ok_or_else(|| bad("electrical.update", "Carica la libreria per cambiare la composizione."))?;
        (read_geometry(library, &c)?, 0.0)
    };
    let at = p.get("at").unwrap_or(&old["at"]);
    let at = at
        .as_array()
        .filter(|v| v.len() == 2)
        .and_then(|v| Some(Vec2::new(v.first()?.as_f64()?, v.get(1)?.as_f64()?)))
        .filter(|v| v.is_finite())
        .ok_or_else(|| bad("electrical.update", "Punto non valido."))?;
    let rotation = p.get("rotation").unwrap_or(&old["rotation"]).as_f64().ok_or_else(|| bad("electrical.update", "Rotazione non valida."))?;
    let side =
        p.get("side").unwrap_or(&old["side"]).as_u64().filter(|v| *v <= 3).ok_or_else(|| bad("electrical.update", "Posizione testo non valida."))?;
    if !rotation.is_finite() || rotation.abs() > 360000.0 {
        return Err(bad("electrical.update", "Rotazione non valida."));
    }
    let backup_dir = p.get("backup_dir").and_then(Value::as_str).map(str::to_string);
    if backup_dir.as_ref().is_some_and(|v| v.len() > 32768) {
        return Err(bad("electrical.update", "Percorso troppo lungo."));
    }
    s.electrical = Some(Placement {
        composition: c,
        point: Some(at),
        rotation: rotation.rem_euclid(360.0),
        side: side as u8,
        phase: 0,
        error: None,
        uid: s.state()?.uid,
        geometry,
        geometry_rotation,
        backup_dir,
        target: Some((h, s.state()?.revision)),
    });
    Ok(json!({"prepared":true}))
}

fn update(s: &mut Session, p: &Value) -> Result<Value> {
    prepare_edit(s, p)?;
    let result = commit(s);
    s.electrical = None;
    result
}

struct Select {
    phase: SelectPhase,
}
impl Interactive for Select {
    fn name(&self) -> &'static str {
        "electrical.select"
    }
    fn prompt(&self, _: &Session) -> Prompt {
        Prompt::new("Seleziona la scatola da modificare", Accept::SELECT)
    }
    fn input(&mut self, s: &mut Session, i: Input) -> Result<Step> {
        match self.phase.feed(s, &i)? {
            SelOutcome::Done(hs) => {
                let result = read(s, &json!({"handles": hs}));
                if let Err(e) = result {
                    self.phase = SelectPhase::single();
                    return Err(e);
                }
                s.set_selection(hs);
                Ok(Step::Done)
            }
            SelOutcome::Empty => Ok(Step::Done),
            SelOutcome::More => Ok(Step::Continue),
        }
    }
}
