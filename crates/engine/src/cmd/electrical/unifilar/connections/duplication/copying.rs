//! Copy-on-write transaction. No identifiers or geometry are published before validation.
use super::super::sections::graph::projection;
use super::*;
use cadcraft_doc::{Block, Entity};

fn fresh_block(d: &mut Drawing, source: &str, depth: usize, budget: &mut usize) -> Result<String> {
    if depth >= 32 {
        return Err(fail("Blocchi annidati o ciclici troppo complessi."));
    }
    let original = d.block(source).cloned().ok_or_else(|| fail("Blocco originale assente."))?;
    *budget = budget.checked_sub(original.entities.len()).ok_or_else(|| fail("La copia richiede troppa geometria."))?;
    if original.entities.len() > 20000 {
        return Err(fail("Simbolo troppo complesso."));
    }
    let name = format!("CC_DUP_{}", id());
    let mut block = Block::new(&name);
    block.base = original.base;
    for e in original.entities.iter() {
        let mut next = e.as_ref().clone();
        next.handle = d.new_handle();
        if let EntityKind::Insert(i) = &mut next.kind {
            i.block = fresh_block(d, &i.block, depth + 1, budget)?;
        }
        block.entities.push(next);
    }
    d.blocks.insert(name.clone(), Arc::new(block));
    Ok(name)
}
fn rename_sigla(
    d: &mut Drawing,
    name: &str,
    original: &cadcraft_doc::Insert,
    meta: &Value,
    layout: &schema::Layout,
    old: &str,
    new: &str,
) -> Result<()> {
    let col = meta["column"].as_u64().and_then(|n| layout.columns.iter().find(|c| Some(c.index) == usize::try_from(n).ok()));
    let cell = col.and_then(|c| c.cells.get("sigla"));
    let table = meta["data_position"].as_str().unwrap_or("table") == "table";
    let block = d.block(name).ok_or_else(|| fail("Blocco assente."))?;
    let mut edits = Vec::new();
    let mut local = false;
    for e in block.entities.iter() {
        let EntityKind::Text(t) = &e.kind else { continue };
        if e.common.layer != "UNI_TESTI" || t.value != old {
            continue;
        }
        let at = t.insert.xy() + original.insert.xy();
        let in_cell = cell.is_some_and(|c| at.x >= c.bounds[0] && at.x <= c.bounds[2] && at.y >= c.bounds[1] && at.y <= c.bounds[3]);
        if table && !in_cell && (at.y < layout.drawing_area[1] || local) {
            continue;
        }
        if !in_cell {
            if local {
                return Err(fail("La sigla coincide con altri testi: disattiva la rinumerazione automatica e modifica la sigla dopo la copia."));
            }
            local = true;
        }
        let mut next = e.as_ref().clone();
        let EntityKind::Text(text) = &mut next.kind else { continue };
        let before = cadcraft_fonts::line_width(old, text.height, text.width_factor);
        let after = cadcraft_fonts::line_width(new, text.height, text.width_factor);
        if in_cell {
            text.insert.x += (before - after) / 2.;
        } else if table && text.insert.x + before < 0. {
            text.insert.x += before - after;
        }
        if let Some(col) = col
            && (text.insert.x + original.insert.x < col.bounds[0] || text.insert.x + original.insert.x + after > col.bounds[2])
        {
            return Err(fail(
                "La nuova sigla non entra nella colonna. Disattiva la rinumerazione automatica e scegli una sigla più breve dopo la copia.",
            ));
        }
        text.value = new.into();
        edits.push(next);
    }
    if edits.is_empty() {
        return Err(fail("Testo della sigla non riconosciuto: disattiva la rinumerazione automatica per copiare questo simbolo."));
    }
    if let Some(block) = d.blocks.get_mut(name) {
        let block = Arc::make_mut(block);
        for e in edits {
            block.entities.replace(e);
        }
    }
    Ok(())
}

fn sigla(old: &str, used: &mut BTreeSet<String>) -> Result<String> {
    if old.trim().is_empty() {
        return Ok(String::new());
    }
    let split = old.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let (prefix, start) = if split < old.len() {
        (old.get(..split).unwrap_or(old).to_owned(), old.get(split..).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0))
    } else {
        (format!("{old}-"), 1)
    };
    for offset in 1..=10000u64 {
        let n = start.checked_add(offset).ok_or_else(|| fail("Numerazione sigla esaurita."))?;
        let new = format!("{prefix}{n}");
        if new.len() > 160 {
            return Err(fail("La nuova sigla è troppo lunga."));
        }
        if used.insert(new.to_lowercase()) {
            return Ok(new);
        }
    }
    Err(fail("Impossibile trovare una sigla libera."))
}
fn remap_link(
    l: &mut Link,
    devices: &BTreeMap<Handle, Handle>,
    wires: &BTreeMap<Handle, Handle>,
    nodes: &mut BTreeMap<String, String>,
    dx: f64,
) -> Result<()> {
    for p in [&mut l.from, &mut l.to].into_iter().flatten() {
        p.handle = devices.get(&handle(&p.handle)?).ok_or_else(|| fail("Un'estremità esce dall'ambito selezionato."))?.hex();
    }
    for t in [&mut l.tap, &mut l.end_tap].into_iter().flatten() {
        t.wire = wires.get(&handle(&t.wire)?).ok_or_else(|| fail("Una derivazione dipende da un filo esterno."))?.hex();
    }
    if let Some(bus) = &mut l.bus {
        *bus = wires.get(&handle(bus)?).ok_or_else(|| fail("Barra esterna al circuito."))?.hex();
    }
    for n in [&mut l.start_node, &mut l.end_node].into_iter().flatten() {
        n.id = nodes.entry(n.id.clone()).or_insert_with(id).clone();
        n.at[0] += dx;
    }
    for x in [&mut l.vertical_x, &mut l.pe_x, &mut l.pe_start].into_iter().flatten() {
        *x += dx;
    }
    if let Some(span) = &mut l.span {
        span[0] += dx;
        span[1] += dx;
    }
    for p in &mut l.waypoints {
        p[0] += dx;
    }
    Ok(())
}
fn stamp(e: &mut Entity, group: &str) {
    references::duplicate_entity(e);
    placement::tags(&mut e.common, "CC_COPY_GROUP", &json!({"id":group}));
    if references::data(e, "CC_CIRCUIT_ROOT").is_some() {
        placement::tags(&mut e.common, "CC_CIRCUIT_ROOT", &json!({"id":id()}));
    }
}
pub(super) fn transform(d: &Draft, target: usize) -> Result<(Drawing, Vec<Handle>, Vec<String>)> {
    let mut draw = (*d.baseline).clone();
    for layer in ["UNI_APPARECCHI", "UNI_TESTI", "UNI_COLLEGAMENTI", "UNI_PE", "UNI_RIMANDI"] {
        if draw.layer(layer).is_some_and(|l| l.locked || l.frozen || !l.on) {
            return Err(fail(format!("Attiva e sblocca {layer}.")));
        }
    }
    let origin = d.layout.columns.iter().position(|c| c.index == target).ok_or_else(|| fail("Colonna di destinazione assente."))?;
    let mut destinations = Vec::new();
    let mut occupied = BTreeSet::new();
    for copy in 0..d.copies {
        let position = i64::try_from(origin)
            .ok()
            .and_then(|n| i64::try_from(copy).ok().and_then(|i| i.checked_mul(d.step)).and_then(|step| n.checked_add(step)))
            .and_then(|n| usize::try_from(n).ok());
        let target = position.and_then(|i| d.layout.columns.get(i)).ok_or_else(|| fail("La serie esce dalla tavola."))?;
        let mapping = super::super::super::circuits::scope::destinations(&d.layout, &d.plan.columns, d.source, target.index)?;
        for (dest, _) in mapping.values() {
            if !occupied.insert(*dest) {
                return Err(fail("Le copie si sovrappongono."));
            }
        }
        destinations.push(mapping);
    }
    for e in draw.model.iter() {
        if let Some(m) = placement::data(e, DEVICE)?
            && m["column"].as_u64().and_then(|n| usize::try_from(n).ok()).is_some_and(|c| occupied.contains(&c))
        {
            return Err(fail("Una colonna di destinazione contiene già un apparecchio o dati. Nessuna copia è stata inserita."));
        }
    }
    let estimate = d.plan.devices.len().checked_add(d.plan.wires.len()).and_then(|n| n.checked_mul(d.copies)).ok_or_else(|| fail("Troppe copie."))?;
    if estimate > 1024 || draw.entity_count().saturating_add(estimate) > 10000 {
        return Err(fail("Duplicazione troppo grande."));
    }
    let source_owners: BTreeSet<_> = d.plan.wires.iter().flat_map(|w| w.owners.iter().copied()).chain(d.plan.devices.iter().copied()).collect();
    for h in &source_owners {
        draw.modify_entity(*h, references::ensure_object_id)?;
    }
    if d.plan.mode == "circuit" && draw.entity(d.plan.root).is_some_and(|e| references::data(e, "CC_CIRCUIT_ROOT").is_none()) {
        draw.modify_entity(d.plan.root, |e| placement::tags(&mut e.common, "CC_CIRCUIT_ROOT", &json!({"id":id()})))?;
    }
    let mut used = BTreeSet::new();
    for e in draw.model.iter() {
        if let Some(m) = placement::data(e, DEVICE)?
            && let Some(label) = m["fields"]["sigla"].as_str()
        {
            used.insert(label.trim().to_lowercase());
        }
    }
    let mut geometry_budget = 100000usize;
    let mut added = Vec::new();
    let mut groups = Vec::new();
    for mapping in destinations {
        let dx = mapping.get(&d.source).map(|(_, dx)| *dx).ok_or_else(|| fail("Colonna iniziale assente."))?;
        let group = id();
        groups.push(group.clone());
        let mut devices = BTreeMap::new();
        for old in &d.plan.devices {
            let source = draw.entity(*old).cloned().ok_or_else(|| fail("Apparecchio assente."))?;
            if draw.layer(&source.common.layer).is_some_and(|l| l.locked || l.frozen || !l.on) {
                return Err(fail("Un apparecchio selezionato è bloccato o nascosto."));
            }
            let mut meta = placement::data(&source, DEVICE)?.ok_or_else(|| fail("Dati assenti."))?;
            let previous = meta["fields"]["sigla"].as_str().unwrap_or("").trim().to_owned();
            let next = if d.renumber { sigla(&previous, &mut used)? } else { previous.clone() };
            if !previous.is_empty() {
                meta["fields"]["sigla"] = json!(next);
            }
            let EntityKind::Insert(original) = &source.kind else {
                return Err(fail("Simbolo non valido."));
            };
            if original.rotation != 0. || original.scale != Vec3::new(1., 1., 1.) {
                return Err(fail("Annulla le trasformazioni generiche del simbolo."));
            }
            let name = fresh_block(&mut draw, &original.block, 0, &mut geometry_budget)?;
            if d.renumber && !previous.is_empty() {
                rename_sigla(&mut draw, &name, original, &meta, &d.layout, &previous, &next)?;
            }
            if references::is_reference(&source)
                && let Some(block) = draw.blocks.get_mut(&name)
            {
                let block = Arc::make_mut(block);
                let texts: Vec<_> = block
                    .entities
                    .iter()
                    .filter_map(|e| {
                        if let EntityKind::Text(t) = &e.kind {
                            let mut e = e.as_ref().clone();
                            let mut t = t.clone();
                            t.value = "DA COMPLETARE".into();
                            e.kind = EntityKind::Text(t);
                            Some(e)
                        } else {
                            None
                        }
                    })
                    .collect();
                for e in texts {
                    block.entities.replace(e);
                }
            }
            let mut insert = original.clone();
            insert.block = name;
            insert.insert.x += dx;
            if insert.insert.x < d.layout.drawing_area[0] || insert.insert.x > d.layout.drawing_area[2] {
                return Err(fail("Un simbolo del circuito esce dalla tavola."));
            }
            meta["at"] = json!([insert.insert.x, insert.insert.y]);
            if let Some(col) = meta["column"].as_u64().and_then(|c| usize::try_from(c).ok()) {
                meta["column"] = json!(mapping.get(&col).ok_or_else(|| fail("Dati di tabella fuori dall'ambito."))?.0);
            }
            let mut entity = source.as_ref().clone();
            entity.handle = draw.new_handle();
            entity.kind = EntityKind::Insert(insert);
            placement::tags(&mut entity.common, DEVICE, &meta);
            stamp(&mut entity, &group);
            let h = entity.handle;
            draw.model.push(entity);
            devices.insert(*old, h);
            added.push(h);
        }
        let mut reserved = Vec::new();
        let mut wires = BTreeMap::new();
        for wire in &d.plan.wires {
            let h = draw.new_handle();
            let name = format!("CC_DUP_WIRE_{}", id());
            draw.blocks.insert(name.clone(), Arc::new(Block::new(&name)));
            let template = wire.owners.first().and_then(|h| draw.entity(*h)).ok_or_else(|| fail("Filo originale assente."))?;
            let mut entity = template.as_ref().clone();
            entity.handle = h;
            let EntityKind::Insert(i) = &mut entity.kind else {
                return Err(fail("Filo non valido."));
            };
            i.block = name;
            let mut data = placement::data(&entity, APP)?.ok_or_else(|| fail("Dati filo assenti."))?;
            data["owner"] = json!(h.hex());
            placement::tags(&mut entity.common, APP, &data);
            stamp(&mut entity, &group);
            draw.model.push(entity);
            reserved.push(h);
            added.push(h);
            if let Some(old) = wire.original {
                wires.insert(old, h);
            }
        }
        let mut nodes = BTreeMap::new();
        let mut outputs = Vec::new();
        for (wire, h) in d.plan.wires.iter().zip(reserved) {
            let mut link = wire.link.clone();
            remap_link(&mut link, &devices, &wires, &mut nodes, dx)?;
            let path: Vec<_> = wire.path.iter().map(|p| *p + Vec2::new(dx, 0.)).collect();
            let geometry = if let Some(old) = wire.original {
                let source = d.baseline.entity(old).ok_or_else(|| fail("Filo assente."))?;
                let EntityKind::Insert(i) = &source.kind else {
                    return Err(fail("Filo non valido."));
                };
                d.baseline
                    .block(&i.block)
                    .ok_or_else(|| fail("Geometria assente."))?
                    .entities
                    .iter()
                    .map(|e| {
                        let mut k = e.kind.clone();
                        k.transform(&Mat3::translate(Vec2::new(dx, 0.)));
                        k
                    })
                    .collect()
            } else {
                path.windows(2).map(|s| line(s[0], s[1])).collect()
            };
            store(&mut draw, Some(h), &link, geometry, path.clone())?;
            outputs.push((h, wire, path));
        }
        for (old, h) in &devices {
            let mut meta =
                placement::data(draw.entity(*h).ok_or_else(|| fail("Marcatore assente."))?, DEVICE)?.ok_or_else(|| fail("Dati assenti."))?;
            if meta["kind"] != "line_marker" {
                continue;
            }
            let original =
                placement::data(d.baseline.entity(*old).ok_or_else(|| fail("Marcatore assente."))?, DEVICE)?.ok_or_else(|| fail("Dati assenti."))?;
            let owner = original["line"].as_str().and_then(Handle::parse_hex).ok_or_else(|| fail("Filo del marcatore assente."))?;
            if let Some(wire) = wires.get(&owner) {
                meta["line"] = json!(wire.hex());
                draw.modify_entity(*h, |e| placement::tags(&mut e.common, DEVICE, &meta))?;
                continue;
            }
            let at: [f64; 2] = serde_json::from_value(meta["at"].clone()).map_err(|_| fail("Posizione non valida."))?;
            let (wire, fraction) = outputs
                .iter()
                .filter(|(_, w, _)| w.owners.contains(&owner))
                .find_map(|(h, _, path)| {
                    projection(path, Vec2::new(at[0], at[1]))
                        .filter(|(_, dist)| *dist < 1e-6)
                        .map(|(along, _)| (*h, along / path.windows(2).map(|s| s[0].dist(s[1])).sum::<f64>()))
                })
                .ok_or_else(|| fail("Il marcatore non appartiene ai fili copiati."))?;
            meta["line"] = json!(wire.hex());
            meta["attachment"]["part"] = json!("route");
            meta["attachment"]["fraction"] = json!(fraction);
            draw.modify_entity(*h, |e| placement::tags(&mut e.common, DEVICE, &meta))?;
        }
        if let Some(root) = devices.get(&d.plan.root)
            && d.plan.mode == "circuit"
        {
            draw.modify_entity(*root, |e| placement::tags(&mut e.common, "CC_CIRCUIT_ROOT", &json!({"id":group})))?;
        }
    }
    refresh(&mut draw, &d.layout)?;
    Ok((draw, added, groups))
}
