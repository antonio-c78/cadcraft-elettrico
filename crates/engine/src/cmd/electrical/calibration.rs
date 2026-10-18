//! Known-distance calibration uses the ordinary SCALE transform, after a full preflight and backup.
use super::*;
use cadcraft_doc::Handle;

pub struct Pick {
    pub points: Vec<Vec2>,
    uid: u64,
    revision: u64,
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("electrical.calibration.prepare", "Prepare known-distance scale", prepare)
            .noundo()
            .params("{scope?:all|selected,handles?,check?:bool}"),
        CommandSpec::new("electrical.calibration.pick", "Pick known distance", |_, _| Err(bad("electrical.calibration.pick", "Prepara la misura.")))
            .noundo()
            .enabled(|s| ready(s).map_err(|e| e.to_string()))
            .interactive(|s| {
                ready(s)?;
                Ok(Box::new(Measure))
            }),
        CommandSpec::new("electrical.calibration.apply", "Scale from known distance", apply)
            .params("{uid,revision,handles,p1:[x,y],p2:[x,y],length,unit:mm|cm|m,model_unit:mm|cm|m,backup_dir?}"),
    ]
}

fn ready(s: &Session) -> Result<()> {
    if !current(s)?.points.is_empty() {
        return Err(bad("electrical.calibration.pick", "Prepara una nuova misura."));
    }
    Ok(())
}

fn current(s: &Session) -> Result<&Pick> {
    let draft = s.electrical_measure.as_ref().ok_or_else(|| bad("electrical.calibration.pick", "Prepara la misura."))?;
    if s.space() != Space::Model || s.state()?.uid != draft.uid || s.state()?.revision != draft.revision {
        return Err(bad("electrical.calibration.pick", "Il disegno è cambiato. Riapri lo strumento."));
    }
    Ok(draft)
}

fn check_targets(s: &Session, handles: &[Handle], locked: bool) -> Result<usize> {
    if s.space() != Space::Model || handles.is_empty() || handles.len() > 100000 {
        return Err(bad("electrical.calibration.prepare", "Seleziona da 1 a 100000 oggetti nello spazio modello."));
    }
    let doc = s.doc()?;
    let mut seen = std::collections::BTreeSet::new();
    let mut electrical = 0;
    for h in handles {
        if !seen.insert(*h) || doc.space_of(*h) != Some(Space::Model) {
            return Err(bad("electrical.calibration.prepare", "Selezione non valida."));
        }
        let e = doc.entity(*h).ok_or_else(|| bad("electrical.calibration.prepare", "Oggetto non più disponibile."))?;
        if locked && doc.layer(&e.common.layer).is_some_and(|l| l.locked) {
            return Err(bad(
                "electrical.calibration.prepare",
                format!("Il layer «{}» è bloccato. Sbloccalo prima di scalare la planimetria.", e.common.layer),
            ));
        }
        if e.common.xdata.iter().any(|t| t.code == 1001 && matches!(t.value.as_str(), "CC_COMPOSITORE" | "CC_SIMBOLO" | "CC_AMBIENTE")) {
            electrical += 1;
        }
    }
    Ok(electrical)
}

fn prepare(s: &mut Session, p: &Value) -> Result<Value> {
    if s.running.is_some() {
        return Err(bad("electrical.calibration.prepare", "Termina il comando corrente."));
    }
    let handles = if p.get("handles").is_some() {
        super::super::targets(s, p)?
    } else {
        match p["scope"].as_str().unwrap_or("all") {
            "all" => s.doc()?.model.handles().to_vec(),
            "selected" => s.selection(),
            _ => return Err(bad("electrical.calibration.prepare", "Ambito non valido.")),
        }
    };
    let check = p["check"].as_bool().unwrap_or(false);
    let electrical = check_targets(s, &handles, !check)?;
    let doc = s.state()?;
    let (uid, revision) = (doc.uid, doc.revision);
    let model_unit = match doc.doc.header.i64("INSUNITS", 0) {
        4 => Some("mm"),
        5 => Some("cm"),
        6 => Some("m"),
        _ => None,
    };
    s.electrical_measure = Some(Pick { points: vec![], uid, revision });
    if !check {
        s.set_selection(handles.clone());
    }
    Ok(
        json!({"uid":uid,"revision":revision,"handles":handles.iter().map(|h|h.hex()).collect::<Vec<_>>(),"count":handles.len(),"electrical":electrical,"model_unit":model_unit}),
    )
}

fn unit(value: &Value) -> Result<f64> {
    match value.as_str() {
        Some("mm") => Ok(1.0),
        Some("cm") => Ok(10.0),
        Some("m") => Ok(1000.0),
        _ => Err(bad("electrical.calibration.apply", "Scegli mm, cm oppure metri.")),
    }
}

fn apply(s: &mut Session, p: &Value) -> Result<Value> {
    if s.running.is_some() || p["uid"].as_u64() != Some(s.state()?.uid) || p["revision"].as_u64() != Some(s.state()?.revision) {
        return Err(bad("electrical.calibration.apply", "Il disegno è cambiato. Ripeti la misura."));
    }
    if !p["handles"].is_array() {
        return Err(bad("electrical.calibration.apply", "Manca la selezione preparata. Ripeti la misura."));
    }
    let handles = super::super::targets(s, p)?;
    let electrical = check_targets(s, &handles, true)?;
    let point = |key: &str| -> Result<Vec2> {
        p[key]
            .as_array()
            .filter(|v| v.len() == 2)
            .and_then(|v| Some(Vec2::new(v.first()?.as_f64()?, v.get(1)?.as_f64()?)))
            .filter(|q| q.is_finite() && q.x.abs() <= 1e12 && q.y.abs() <= 1e12)
            .ok_or_else(|| bad("electrical.calibration.apply", "Punti di riferimento non validi."))
    };
    let a = point("p1")?;
    let b = point("p2")?;
    let distance = a.dist(b);
    let length = p["length"]
        .as_f64()
        .filter(|n| n.is_finite() && *n > 0.0 && *n <= 1e12)
        .ok_or_else(|| bad("electrical.calibration.apply", "Indica una misura reale positiva."))?;
    let target = length * unit(&p["unit"])? / unit(&p["model_unit"])?;
    let factor = target / distance;
    if distance <= 1e-9 || !factor.is_finite() || !(1e-9..=1e9).contains(&factor) {
        return Err(bad("electrical.calibration.apply", "Distanza nulla o rapporto di scala fuori limite."));
    }
    if p["backup_dir"].as_str().is_some_and(|v| v.len() > 32768) {
        return Err(bad("electrical.calibration.apply", "Percorso della copia di sicurezza troppo lungo."));
    }
    let backup = backup(s.doc()?, p["backup_dir"].as_str())?;
    let transformed = super::super::modify::transform_entities(s, &handles, &Mat3::scale_about(a, factor), false)?;
    let code = match p["model_unit"].as_str() {
        Some("cm") => 5,
        Some("m") => 6,
        _ => 4,
    };
    s.doc_mut()?.header.set_i64("INSUNITS", code);
    Ok(json!({"factor":factor,"distance":distance,"target":target,"count":transformed.len(),"electrical":electrical,"backup":backup}))
}

struct Measure;
impl Interactive for Measure {
    fn name(&self) -> &'static str {
        "electrical.calibration.pick"
    }
    fn prompt(&self, s: &Session) -> Prompt {
        let first = s.electrical_measure.as_ref().and_then(|d| d.points.first()).copied();
        Prompt::new(if first.is_some() { "Indica il secondo estremo della misura" } else { "Indica il primo estremo della misura" }, Accept::POINT)
            .base_opt(first)
    }
    fn input(&mut self, s: &mut Session, input: Input) -> Result<Step> {
        let d = current(s)?;
        if let Input::Point(point) = input {
            if !point.is_finite() || point.x.abs() > 1e12 || point.y.abs() > 1e12 || d.points.first().is_some_and(|a| a.dist(point) <= 1e-9) {
                return Err(bad("electrical.calibration.pick", "Scegli due punti distinti e validi."));
            }
            if let Some(d) = &mut s.electrical_measure {
                if d.points.len() < 2 {
                    d.points.push(point);
                }
                if d.points.len() == 2 {
                    return Ok(Step::Done);
                }
            }
        }
        Ok(Step::Continue)
    }
    fn preview(&self, s: &Session, cursor: Vec2) -> Vec<EntityKind> {
        current(s).ok().and_then(|d| d.points.first().copied()).filter(|_| cursor.is_finite()).map(|a| vec![line(a, cursor)]).unwrap_or_default()
    }
}
