//! Electrical topology is independent of wire geometry and legacy UI connection kinds.
//! A junction references a stable owner and an arc-length fraction; dependencies are bounded.
use super::super::*;
use super::connections::{Link, Port};
use cadcraft_doc::Handle;
use std::collections::{BTreeMap, BTreeSet};
fn err(message: &str) -> crate::EngineError {
    bad("electrical.unifilar.link", message)
}
pub(super) fn parents(l: &Link) -> Result<Vec<Handle>> {
    let mut out = Vec::new();
    for text in [l.tap.as_ref().map(|t| t.wire.as_str()), l.bus.as_deref(), l.end_tap.as_ref().map(|t| t.wire.as_str())].into_iter().flatten() {
        let h = Handle::parse_hex(text).ok_or_else(|| err("Riferimento alla giunzione non valido."))?;
        if !out.contains(&h) {
            out.push(h);
        }
    }
    Ok(out)
}
fn parent(l: &Link) -> Result<Option<Handle>> {
    Ok(parents(l)?.first().copied())
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
    // An undirected graph: port labels carry no power-flow semantics.
    type Key = (String, String);
    fn tokens(w: &Link) -> Result<Vec<Key>> {
        let mut t: Vec<_> = [w.from.as_ref(), w.to.as_ref()].into_iter().flatten().map(|p| (p.handle.clone(), p.port.clone())).collect();
        t.extend([w.start_node.as_ref(), w.end_node.as_ref()].into_iter().flatten().map(|n| ("@node".into(), n.id.clone())));
        for h in parents(w)? {
            t.push(("@wire".into(), h.hex()));
        }
        if w.kind == "pe" || w.pe_start.is_some() || w.pe_x.is_some() {
            t.push(("PE".into(), "pe".into()));
        }
        Ok(t)
    }
    let mut edges: BTreeMap<Key, Vec<Key>> = BTreeMap::new();
    for (h, w) in links {
        let root = ("@wire".into(), h.hex());
        for t in tokens(w)? {
            edges.entry(root.clone()).or_default().push(t.clone());
            edges.entry(t).or_default().push(root.clone());
        }
    }
    let mut pending = tokens(l)?;
    let mut seen = BTreeSet::new();
    while let Some(key) = pending.pop() {
        if seen.insert(key.clone())
            && let Some(next) = edges.get(&key)
        {
            pending.extend(next.iter().cloned());
        }
        if seen.len() > 2048 {
            return Err(err("Rete troppo complessa."));
        }
    }
    seen.into_iter().find(|k| k.0 != "@wire").ok_or_else(|| err("Il filo non ha terminali."))
}
pub(super) fn order(links: &BTreeMap<Handle, Link>) -> Result<Vec<Handle>> {
    if links.len() > 256 {
        return Err(err("Troppi collegamenti."));
    }
    let mut ready = BTreeSet::new();
    let mut result = Vec::new();
    for _ in 0..=links.len() {
        for (h, l) in links {
            if !ready.contains(h) && parents(l)?.iter().all(|p| ready.contains(p)) {
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
    let mut anchors: BTreeMap<&str, [f64; 2]> = BTreeMap::new();
    for l in links.values() {
        for n in [l.start_node.as_ref(), l.end_node.as_ref()].into_iter().flatten() {
            if let Some(old) = anchors.insert(&n.id, n.at)
                && (old[0] - n.at[0]).abs() + (old[1] - n.at[1]).abs() > 1e-6
            {
                return Err(err("Posizioni discordanti della giunzione."));
            }
        }
    }
    let mut inputs = BTreeSet::new();
    let mut pe = BTreeSet::new();
    let mut edges: BTreeMap<Handle, Vec<Handle>> = BTreeMap::new();
    for l in links.values() {
        if l.kind == "wire" {
            let start = usize::from(l.from.is_some())
                + usize::from(l.tap.is_some())
                + usize::from(l.pe_start.is_some())
                + usize::from(l.start_node.is_some());
            let end =
                usize::from(l.to.is_some()) + usize::from(l.end_tap.is_some()) + usize::from(l.pe_x.is_some()) + usize::from(l.end_node.is_some());
            if start != 1 || end != 1 || l.bus.is_some() || l.span.is_some() {
                return Err(err("Terminali del filo non validi."));
            }
            continue;
        }
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
