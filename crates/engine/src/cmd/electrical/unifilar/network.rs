//! Electrical topology is independent of wire geometry and legacy UI connection kinds.
//! A junction references a stable owner and an arc-length fraction; dependencies are bounded.
use super::super::*;
use super::connections::{Link, Port};
use cadcraft_doc::Handle;
use std::collections::{BTreeMap, BTreeSet};
fn err(message: &str) -> crate::EngineError {
    bad("electrical.unifilar.link", message)
}
pub(super) fn parent(l: &Link) -> Result<Option<Handle>> {
    let text = l.tap.as_ref().map(|t| t.wire.as_str()).or(l.bus.as_deref());
    text.map(|h| Handle::parse_hex(h).ok_or_else(|| err("Riferimento alla giunzione non valido."))).transpose()
}
pub(super) fn source<'a>(l: &'a Link, links: &'a BTreeMap<Handle, Link>) -> Result<&'a Port> {
    let mut current = l;
    let mut seen = BTreeSet::new();
    for _ in 0..=256 {
        if let Some(h) = parent(current)? {
            if !seen.insert(h) {
                return Err(err("Le giunzioni formano una dipendenza circolare."));
            }
            current = links.get(&h).ok_or_else(|| err("Il collegamento della giunzione non esiste più."))?;
            if current.kind == "pe" {
                return Err(err("Una derivazione di alimentazione non può partire dal PE."));
            }
        } else {
            return current.from.as_ref().ok_or_else(|| err("Origine del collegamento mancante."));
        }
    }
    Err(err("Troppi livelli di derivazione."))
}
pub(super) fn net(l: &Link, links: &BTreeMap<Handle, Link>) -> Result<(String, String)> {
    if l.kind == "pe" {
        return Ok(("PE".into(), "pe".into()));
    }
    let p = source(l, links)?;
    Ok((p.handle.clone(), p.port.clone()))
}
pub(super) fn order(links: &BTreeMap<Handle, Link>) -> Result<Vec<Handle>> {
    if links.len() > 256 {
        return Err(err("Troppi collegamenti."));
    }
    let mut ready = BTreeSet::new();
    let mut result = Vec::new();
    for _ in 0..=links.len() {
        for (h, l) in links {
            if !ready.contains(h) && parent(l)?.is_none_or(|p| ready.contains(&p)) {
                ready.insert(*h);
                result.push(*h);
            }
        }
        if ready.len() == links.len() {
            return Ok(result);
        }
    }
    Err(err("Giunzione mancante o dipendenza circolare."))
}
pub(super) fn validate_graph(links: &BTreeMap<Handle, Link>) -> Result<()> {
    let _ = order(links)?;
    let mut inputs = BTreeSet::new();
    let mut pe = BTreeSet::new();
    let mut edges: BTreeMap<Handle, Vec<Handle>> = BTreeMap::new();
    for l in links.values() {
        let has_source = l.from.as_ref().is_some_and(|p| p.port == if l.kind == "pe" { "pe" } else { "out" });
        let has_target = l.to.as_ref().is_some_and(|p| p.port == "in");
        let shape = match l.kind.as_str() {
            "series" => has_source && has_target && l.tap.is_none() && l.bus.is_none() && l.span.is_none(),
            "branch" => l.from.is_none() && has_target && (l.tap.is_some() != l.bus.is_some()) && l.span.is_none(),
            "bus" => has_source && l.to.is_none() && l.tap.is_none() && l.bus.is_none() && l.span.is_some_and(|s| s[1] - s[0] >= 2.),
            "pe" => has_source && l.to.is_none() && l.tap.is_none() && l.bus.is_none() && l.span.is_none(),
            _ => false,
        };
        if !shape {
            return Err(err("Terminali o giunzioni incompatibili con il collegamento."));
        }
        if let Some(bus) = &l.bus
            && !links.get(&Handle::parse_hex(bus).ok_or_else(|| err("Barra non valida."))?).is_some_and(|l| l.kind == "bus")
        {
            return Err(err("Barra comune non disponibile."));
        }
        if l.kind == "pe" {
            if !pe.insert(l.from.as_ref().map(|p| (&p.handle, &p.port))) {
                return Err(err("Il punto PE è già collegato."));
            }
            continue;
        }
        let a = source(l, links)?;
        if let Some(to) = &l.to {
            let a = Handle::parse_hex(&a.handle).ok_or_else(|| err("Origine non valida."))?;
            let b = Handle::parse_hex(&to.handle).ok_or_else(|| err("Arrivo non valido."))?;
            if !inputs.insert((b, &to.port)) {
                return Err(err("L'ingresso è già collegato. Modifica il collegamento esistente."));
            }
            if a == b {
                return Err(err("Un apparecchio non può alimentare se stesso."));
            }
            edges.entry(a).or_default().push(b);
        } else if l.kind != "bus" {
            return Err(err("Arrivo del collegamento mancante."));
        }
    }
    for start in edges.keys() {
        let mut seen = BTreeSet::new();
        let mut pending = edges.get(start).cloned().unwrap_or_default();
        while let Some(h) = pending.pop() {
            if h == *start {
                return Err(err("Il collegamento creerebbe un circuito chiuso di alimentazione."));
            }
            if seen.insert(h)
                && let Some(next) = edges.get(&h)
            {
                pending.extend_from_slice(next);
            }
        }
    }
    Ok(())
}
