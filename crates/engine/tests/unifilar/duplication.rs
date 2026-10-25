//! Real wire-engine fixtures: isolation, rooted scope, identities, atomic arrays and persistence.
use super::*;
use cadcraft_engine::project::references::data;
fn clear(f: &mut Fixture, keep: &[usize]) {
    for (i, h) in f.devices.iter().enumerate() {
        if !keep.contains(&i) {
            f.s.doc_mut().unwrap().remove_entity(Handle::parse_hex(h).unwrap());
        }
    }
}
fn child(f: &mut Fixture) -> String {
    let path = f.root.join("catalogo.json");
    let mut catalog: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let mut sym = catalog["symbols"][0].clone();
    sym["id"] = json!("command");
    sym["category"] = json!("Comando");
    sym["data_position"] = json!("right");
    catalog["symbols"].as_array_mut().unwrap().push(sym);
    std::fs::write(path, catalog.to_string()).unwrap();
    f.s.execute("electrical.unifilar.insert", &json!({"model":f.model,"id":"command","at":[27.5,235.],"fields":{"sigla":"KM1"},"backup_dir":f.root}))
        .unwrap()["handle"]
        .as_str()
        .unwrap()
        .into()
}
fn duplicate(f: &mut Fixture, mode: &str, column: usize, copies: usize) -> Value {
    f.s.execute(
        "electrical.unifilar.duplicate",
        &json!({"model":f.model,"handle":f.devices[0],"mode":mode,"column":column,"copies":copies,"backup_dir":f.root}),
    )
    .unwrap()
}
fn ids(s: &Session) -> Vec<String> {
    s.doc()
        .unwrap()
        .model
        .iter()
        .filter(|e| data(e, "CC_UNIFILARE_DEVICE").is_some() || data(e, "CC_UNIFILARE_LINK").is_some())
        .map(|e| {
            let id = data(e, "CC_OBJECT_ID").unwrap()["id"].as_str().unwrap().to_owned();
            assert!(uuid::Uuid::parse_str(&id).is_ok());
            id
        })
        .collect()
}
fn added(s: &Session, v: &Value) -> Vec<Handle> {
    v["handles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| {
            let h = Handle::parse_hex(h.as_str().unwrap()).unwrap();
            assert!(s.doc().unwrap().entity(h).is_some());
            h
        })
        .collect()
}
#[test]
fn duplicate_column_array_cuts_external_branches_and_removes_copied_junction_dots() {
    let mut f = fixture();
    clear(&mut f, &[0, 1]);
    let command = child(&mut f);
    let main = f.link(json!({"version":3,"kind":"wire","from":f.port(0,"out"),"to":{"handle":command,"port":"in"},"y":255.}));
    f.link(json!({"version":3,"kind":"wire","tap":{"wire":main["handle"],"part":"route","fraction":0.3},"to":f.port(1,"in"),"y":255.}));
    let before = f.s.doc().unwrap().clone();
    let original_ids = ids(&f.s);
    let result = duplicate(&mut f, "column", 3, 2);
    let new = added(&f.s, &result);
    assert_eq!(new.len(), 6, "two devices and one internal vertical wire per copy");
    let all = ids(&f.s);
    assert_eq!(all.len(), all.iter().collect::<std::collections::BTreeSet<_>>().len());
    assert!(original_ids.iter().all(|id| all.contains(id)));
    assert_eq!(result["groups"].as_array().unwrap().len(), 2);
    assert_ne!(result["groups"][0], result["groups"][1]);
    for h in &new {
        if data(f.s.doc().unwrap().entity(*h).unwrap(), "CC_UNIFILARE_LINK").is_some() {
            let wire = f.s.execute("electrical.unifilar.link.read", &json!({"handle":h.hex()})).unwrap();
            assert!(wire["link"]["tap"].is_null() && wire["link"]["end_tap"].is_null());
            for p in ["from", "to"] {
                assert!(new.contains(&Handle::parse_hex(wire["link"][p]["handle"].as_str().unwrap()).unwrap()));
            }
            let path = f.path(&json!(h.hex()));
            assert!(path.windows(2).all(|p| p[0][0] == p[1][0]));
        }
    }
    let saved = f.s.doc().unwrap().clone();
    f.s.undo().unwrap();
    assert_eq!(f.s.doc().unwrap().model, before.model);
    f.s.redo().unwrap();
    assert_eq!(f.s.doc().unwrap().model, saved.model);
}
#[test]
fn duplicate_circuit_starts_at_root_output_copies_other_columns_and_detaches_upstream() {
    let mut f = fixture();
    clear(&mut f, &[0, 1]);
    f.s.execute("electrical.project.new", &json!({"name":"Test"})).unwrap();
    f.s.execute("electrical.project.add", &json!({})).unwrap();
    let upstream = f.s.execute("electrical.project.reference.insert", &json!({"at":[27.5,282.5],"kind":"input"})).unwrap();
    f.link(json!({"version":3,"kind":"wire","from":{"handle":upstream["handle"],"port":"in"},"to":f.port(0,"in"),"y":280.}));
    f.link(json!({"version":3,"kind":"wire","from":f.port(0,"out"),"to":f.port(1,"in"),"y":258.}));
    let result = duplicate(&mut f, "circuit", 3, 1);
    let handles = added(&f.s, &result);
    assert_eq!(handles.len(), 3);
    let devices: Vec<_> = handles.iter().filter_map(|h| data(f.s.doc().unwrap().entity(*h).unwrap(), "CC_UNIFILARE_DEVICE")).collect();
    assert_eq!(
        devices.iter().map(|m| m["column"].as_u64().unwrap()).collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([3, 4])
    );
    let all = ids(&f.s);
    assert_eq!(all.len(), all.iter().collect::<std::collections::BTreeSet<_>>().len());
    let file = f.root.join("duplicate.dxf");
    f.s.execute("saveas", &json!({"path":file})).unwrap();
    let read = cadcraft_io::read(&std::fs::read(file).unwrap(), "drawing.dxf").unwrap();
    for h in handles {
        assert_eq!(data(read.entity(h).unwrap(), "CC_OBJECT_ID"), data(f.s.doc().unwrap().entity(h).unwrap(), "CC_OBJECT_ID"));
    }
}

#[test]
fn moving_rooted_circuit_uses_copy_scope_but_keeps_upstream_and_existing_identities() {
    let mut f = fixture();
    clear(&mut f, &[0, 1]);
    f.s.execute("electrical.project.new", &json!({"name":"Move"})).unwrap();
    f.s.execute("electrical.project.add", &json!({})).unwrap();
    let reference = f.s.execute("electrical.project.reference.insert", &json!({"at":[27.5,282.5],"kind":"input"})).unwrap();
    let feed = f.link(json!({"version":3,"kind":"wire","from":{"handle":reference["handle"],"port":"in"},"to":f.port(0,"in"),"y":280.}));
    let wire = f.link(json!({"version":3,"kind":"wire","from":f.port(0,"out"),"to":f.port(1,"in"),"y":258.}));
    let old_path = f.path(&wire["handle"]);
    let before = f.s.doc().unwrap().clone();
    let devices: Vec<_> = before.model.iter().map(|e| e.handle).collect();
    let result = f
        .s
        .execute("electrical.unifilar.circuit.move", &json!({"model":f.model,"handle":f.devices[0],"mode":"circuit","column":3,"backup_dir":f.root}))
        .unwrap();
    assert_eq!(result["columns"], json!([1, 2]));
    for (i, col) in [(0, 3), (1, 4)] {
        let m = f.s.execute("electrical.unifilar.read", &json!({"handle":f.devices[i]})).unwrap();
        assert_eq!(m["column"], col);
    }
    assert_eq!(f.path(&wire["handle"]), old_path.iter().map(|p| [p[0] + 70., p[1]]).collect::<Vec<_>>());
    assert_eq!(f.path(&feed["handle"]).first(), Some(&[27.5, 282.5]));
    assert_eq!(f.path(&feed["handle"]).last(), Some(&[97.5, 275.]));
    assert_eq!(devices, f.s.doc().unwrap().model.iter().map(|e| e.handle).collect::<Vec<_>>());
    let after = f.s.doc().unwrap().clone();
    f.s.undo().unwrap();
    assert_eq!(f.s.doc().unwrap().model, before.model);
    f.s.redo().unwrap();
    assert_eq!(f.s.doc().unwrap().model, after.model);
}

#[test]
fn moving_circuit_uses_root_column_not_leftmost_and_leaves_unconnected_apparatus() {
    let mut f = fixture();
    clear(&mut f, &[1]);
    let child =
        f.s.execute("electrical.unifilar.insert", &json!({"model":f.model,"id":"dev","at":[27.5,210.],"fields":{},"backup_dir":f.root})).unwrap();
    let foreign =
        f.s.execute("electrical.unifilar.insert", &json!({"model":f.model,"id":"dev","at":[27.5,185.],"fields":{},"backup_dir":f.root})).unwrap();
    let wire = f.link(json!({"version":3,"kind":"wire","from":f.port(1,"out"),"to":{"handle":child["handle"],"port":"in"},"y":225.}));
    let old_path = f.path(&wire["handle"]);
    let unrelated = f.s.doc().unwrap().entity(Handle::parse_hex(foreign["handle"].as_str().unwrap()).unwrap()).unwrap().clone();
    f.s.execute("electrical.unifilar.circuit.move", &json!({"model":f.model,"handle":f.devices[1],"mode":"circuit","column":4,"backup_dir":f.root}))
        .unwrap();
    assert_eq!(f.s.execute("electrical.unifilar.read", &json!({"handle":f.devices[1]})).unwrap()["column"], 4);
    assert_eq!(f.s.execute("electrical.unifilar.read", &json!({"handle":child["handle"]})).unwrap()["column"], 3);
    assert_eq!(f.s.doc().unwrap().entity(unrelated.handle).unwrap(), &unrelated);
    assert_eq!(f.path(&wire["handle"]), old_path.iter().map(|p| [p[0] + 70., p[1]]).collect::<Vec<_>>());
}

#[test]
fn moving_circuit_moves_its_downstream_bar_and_branches_together() {
    let mut f = fixture();
    clear(&mut f, &[0, 1]);
    let bus = f.link(json!({"version":1,"kind":"bus","from":f.port(0,"out"),"span":[45,75],"y":262.5}));
    let branch = f.link(json!({"version":1,"kind":"branch","bus":bus["handle"],"to":f.port(1,"in"),"y":262.5}));
    let old = f.path(&branch["handle"]);
    f.s.execute("electrical.unifilar.circuit.move", &json!({"model":f.model,"handle":f.devices[0],"mode":"circuit","column":3,"backup_dir":f.root}))
        .unwrap();
    let read = f.s.execute("electrical.unifilar.link.read", &json!({"handle":bus["handle"]})).unwrap();
    assert_eq!(read["link"]["span"], json!([115., 145.]));
    assert_eq!(f.path(&branch["handle"]), old.iter().map(|p| [p[0] + 70., p[1]]).collect::<Vec<_>>());
}
#[test]
fn arrays_are_atomic_on_occupied_out_of_bounds_and_hostile_step_and_cancel() {
    use cadcraft_engine::{Input, geom::Vec2};
    let mut f = fixture();
    clear(&mut f, &[0, 3]);
    let before = f.s.doc().unwrap().clone();
    for (column, copies, step) in [(2, 3, 1), (3, 3, 1), (2, 2, i64::MIN), (2, 2, 0)] {
        assert!(
            f.s.execute(
                "electrical.unifilar.duplicate",
                &json!({"model":f.model,"handle":f.devices[0],"mode":"column","column":column,"copies":copies,"step":step,"backup_dir":f.root})
            )
            .is_err()
        );
        assert_eq!(f.s.doc().unwrap().model, before.model);
    }
    let h = Handle::parse_hex(&f.devices[0]).unwrap();
    f.s.set_selection(vec![h]);
    f.s.execute("electrical.unifilar.duplicate.prepare", &json!({"model":f.model,"mode":"column"})).unwrap();
    f.s.start("electrical.unifilar.duplicate.place").unwrap();
    assert!(!f.s.preview(Vec2::new(62.5, 220.)).is_empty());
    f.s.input(Input::Cancel).unwrap();
    assert!(f.s.unifilar_duplicate.is_none());
    assert_eq!(f.s.selection(), vec![h]);
    assert_eq!(f.s.doc().unwrap().model, before.model);
}
#[test]
fn visible_sigla_is_renumbered_without_changing_identical_glyph_or_description_text() {
    use cadcraft_engine::doc::{Common, Text};
    let mut f = fixture();
    clear(&mut f, &[0]);
    let h = Handle::parse_hex(&f.devices[0]).unwrap();
    let EntityKind::Insert(i) = &f.s.doc().unwrap().entity(h).unwrap().kind else { panic!() };
    let name = i.block.clone();
    let mut block = f.s.doc().unwrap().block(&name).unwrap().as_ref().clone();
    for (layer, y) in [("0", -5.), ("UNI_TESTI", -140.)] {
        let mut e = Entity::new(
            f.s.doc_mut().unwrap().new_handle(),
            EntityKind::Text(Text {
                value: "Q0".into(),
                insert: Vec3::new(2., y, 0.),
                height: 1.,
                width_factor: 1.,
                rotation: 0.,
                oblique: 0.,
                style: "Standard".into(),
                halign: Default::default(),
                valign: Default::default(),
                align_pt: None,
            }),
        );
        e.common = Common { layer: layer.into(), ..Default::default() };
        block.entities.push(e);
    }
    f.s.doc_mut().unwrap().blocks.insert(name, Arc::new(block));
    let result = duplicate(&mut f, "column", 2, 1);
    let new = added(&f.s, &result)[0];
    let m = data(f.s.doc().unwrap().entity(new).unwrap(), "CC_UNIFILARE_DEVICE").unwrap();
    assert_ne!(m["fields"]["sigla"], "Q0");
    let EntityKind::Insert(i) = &f.s.doc().unwrap().entity(new).unwrap().kind else { panic!() };
    let texts: Vec<_> =
        f.s.doc()
            .unwrap()
            .block(&i.block)
            .unwrap()
            .entities
            .iter()
            .filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some((e.common.layer.clone(), t.value.clone())) } else { None })
            .collect();
    assert!(texts.contains(&("0".into(), "Q0".into())));
    assert!(texts.contains(&("UNI_TESTI".into(), "Q0".into())));
}

#[test]
fn rooted_fork_copies_junctions_with_fresh_node_identities() {
    let mut f = fixture();
    clear(&mut f, &[0, 1]);
    let command = child(&mut f);
    let junction = json!({"id":"branch-source", "at":[27.5,250.]});
    f.link(json!({"version":3,"kind":"wire","from":f.port(0,"out"),"end_node":junction,"y":255.}));
    f.link(json!({"version":3,"kind":"wire","start_node":junction,"to":{"handle":command,"port":"in"},"y":255.}));
    f.link(json!({"version":3,"kind":"wire","start_node":junction,"to":f.port(1,"in"),"y":250.}));
    let nodes = |s: &Session| -> std::collections::BTreeSet<String> {
        s.doc()
            .unwrap()
            .model
            .iter()
            .filter_map(|e| data(e, "CC_UNIFILARE_LINK"))
            .flat_map(|m| [m["link"]["start_node"]["id"].as_str().map(str::to_owned), m["link"]["end_node"]["id"].as_str().map(str::to_owned)])
            .flatten()
            .collect()
    };
    let original = nodes(&f.s);
    assert!(!original.is_empty());
    let before = dot_count(&f);
    let result = duplicate(&mut f, "circuit", 3, 1);
    let after = nodes(&f.s);
    assert!(after.len() > original.len());
    assert!(original.is_subset(&after));
    assert_eq!(dot_count(&f), before * 2);
    let new = added(&f.s, &result);
    for h in new {
        if let Some(m) = data(f.s.doc().unwrap().entity(h).unwrap(), "CC_UNIFILARE_LINK") {
            for key in ["start_node", "end_node"] {
                if let Some(id) = m["link"][key]["id"].as_str() {
                    assert!(!original.contains(id));
                }
            }
        }
    }
}

#[test]
fn return_to_upstream_is_rejected_without_guessing_the_circuit_boundary() {
    let mut f = fixture();
    clear(&mut f, &[0]);
    let command = child(&mut f);
    f.link(json!({"version":3,"kind":"wire","from":f.port(0,"out"),"to":{"handle":command,"port":"in"},"y":250.}));
    f.link(json!({"version":3,"kind":"wire","from":{"handle":command,"port":"out"},"to":f.port(0,"in"),"y":282.,"waypoints":[[5.,223.],[5.,282.],[27.5,282.]]}));
    let before = f.s.doc().unwrap().clone();
    let err =
        f.s.execute("electrical.unifilar.duplicate", &json!({"model":f.model,"handle":f.devices[0],"mode":"circuit","column":3,"backup_dir":f.root}))
            .unwrap_err();
    assert!(err.to_string().contains("monte"));
    assert_eq!(f.s.doc().unwrap().model, before.model);
}

#[test]
fn leftward_column_series_uses_distinct_identifiers() {
    let mut f = fixture();
    clear(&mut f, &[0]);
    let result =
        f.s.execute(
            "electrical.unifilar.duplicate",
            &json!({"model":f.model,"handle":f.devices[0],"mode":"column","column":4,"copies":2,"step":-1,"backup_dir":f.root}),
        )
        .unwrap();
    let columns: std::collections::BTreeSet<_> = added(&f.s, &result)
        .iter()
        .filter_map(|h| data(f.s.doc().unwrap().entity(*h).unwrap(), "CC_UNIFILARE_DEVICE"))
        .map(|m| m["column"].as_u64().unwrap())
        .collect();
    assert_eq!(columns, std::collections::BTreeSet::from([3, 4]));
    let all = ids(&f.s);
    assert_eq!(all.len(), all.iter().collect::<std::collections::BTreeSet<_>>().len());
}

// Count all native links, including independently named duplicate-owned blocks.
fn dot_count(f: &Fixture) -> usize {
    f.s.doc()
        .unwrap()
        .model
        .iter()
        .filter(|e| data(e, "CC_UNIFILARE_LINK").is_some())
        .filter_map(|e| if let EntityKind::Insert(i) = &e.kind { f.s.doc().unwrap().block(&i.block) } else { None })
        .map(|b| b.entities.iter().filter(|e| matches!(e.kind, EntityKind::Solid(_))).count() / 8)
        .sum()
}
