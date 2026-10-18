//! Synthetic, original catalog only. Exercises the same commands as the native composer.
use std::sync::Arc;

use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::{Block, Drawing, Entity, EntityKind, Handle, Line};
use cadcraft_engine::geom::{Vec2, Vec3};
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};

fn metadata(s: &Session) -> Value {
    let e = s.doc().unwrap().model.iter().last().unwrap();
    let text: String = e.common.xdata.iter().filter(|t| t.code == 1000).map(|t| t.value.as_str()).collect();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn editing_replaces_only_one_reference_preserves_geometry_supports_seven_and_is_atomic() {
    set_io(IoHooks {
        read: |b, n| cadcraft_io::read(b, n).map_err(|e| e.to_string()),
        write: |d, n| cadcraft_io::write(d, n).map_err(|e| e.to_string()),
        plot: None,
    });
    let root = std::env::temp_dir().join(format!(
        "cadcraft-edit-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let mut library = Drawing::new_metric();
    let mut b = Block::new("Switch");
    b.entities.push(Entity::new(Handle(100), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(3.0, 4.0, 0.0) })));
    library.blocks.insert("Switch".into(), Arc::new(b));
    let path = root.join("symbols.dxf");
    std::fs::write(&path, cadcraft_io::write(&library, "symbols.dxf").unwrap()).unwrap();
    let mut s = Session::new();
    let request = json!({"library":path,"backup_dir":root,"composition":{"capacity":3,"items":[{"name":"Switch","block":"Switch","modules":1}],"factor":1.0,"installation_height_cm":100},"at":[20,30],"rotation":90});
    let inserted = s.execute("electrical.insert", &request).unwrap();
    let handle = inserted["handle"].as_u64().map(Handle).or_else(|| inserted["handle"].as_str().and_then(Handle::parse_hex)).unwrap();
    // Simulate an older box: no saved layer configuration, reference on EL_SCATOLE.
    let mut legacy = metadata(&s);
    legacy.as_object_mut().unwrap().remove("layers");
    s.doc_mut().unwrap().ensure_layer("EL_SCATOLE");
    s.doc_mut()
        .unwrap()
        .modify_entity(handle, |e| {
            e.common.layer = "EL_SCATOLE".into();
            e.common.xdata.retain(|t| t.code == 1001);
            for chunk in legacy.to_string().as_bytes().chunks(200) {
                e.common.xdata.push(cadcraft_engine::doc::RawTag { code: 1000, value: String::from_utf8(chunk.to_vec()).unwrap() });
            }
        })
        .unwrap();
    // A second reference deliberately shares the original definition, just like COPY.
    s.doc_mut()
        .unwrap()
        .modify_entity(handle, |e| {
            e.common.xdata.push(cadcraft_engine::doc::RawTag { code: 1001, value: "CC_AMBIENTE_REF".into() });
            e.common.xdata.push(cadcraft_engine::doc::RawTag { code: 1000, value: "{\"room\":\"F00\"}".into() });
        })
        .unwrap();
    let original = s.doc().unwrap().entity(handle).unwrap().clone();
    let copy = s.doc_mut().unwrap().add(&cadcraft_engine::doc::Space::Model, original.common.clone(), original.kind.clone()).unwrap();
    let original_block = inserted["block"].as_str().unwrap();
    let lines = |s: &Session, name: &str| {
        s.doc()
            .unwrap()
            .block(name)
            .unwrap()
            .entities
            .iter()
            .filter_map(|e| match &e.kind {
                EntityKind::Line(l) => Some((l.a, l.b)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let old_lines = lines(&s, original_block);
    let mut edit = s.execute("electrical.read", &json!({"handle":handle})).unwrap();
    edit["backup_dir"] = json!(root);
    edit["composition"]["installation_height_cm"] = json!(120);
    edit["side"] = json!(3);
    let before = s.state().unwrap().doc.clone();
    s.execute("electrical.edit.prepare", &edit).unwrap();
    s.start("electrical.place").unwrap();
    assert_eq!(s.electrical.as_ref().unwrap().phase, 1);
    s.cancel();
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    let result = s.execute("electrical.update", &edit).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 2);
    assert_eq!(lines(&s, result["block"].as_str().unwrap()), old_lines);
    assert_eq!(s.doc().unwrap().entity(copy).unwrap().kind, original.kind);
    assert_eq!(s.doc().unwrap().entity(copy).unwrap().common.layer, "EL_SCATOLE");
    let block = s.doc().unwrap().block(result["block"].as_str().unwrap()).unwrap();
    assert!(block.entities.iter().any(|e| matches!(&e.kind,EntityKind::Text(t) if t.value=="H120" && t.rotation==0.0)));
    assert!(block.entities.iter().filter(|e| matches!(&e.kind, EntityKind::Text(_))).all(|e| e.common.layer == "EL_ALTEZZE"));
    assert_eq!(s.doc().unwrap().entity(handle).unwrap().common.layer, "EL_COMPONENTI");
    assert!(s.doc().unwrap().entity(handle).unwrap().common.xdata.iter().any(|t| t.code == 1000 && t.value == "{\"room\":\"F00\"}"));
    // A stale dialog may not overwrite later drawing changes.
    assert!(s.execute("electrical.update", &edit).is_err());
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().entity(handle).unwrap().kind, original.kind);
    let mut edit = s.execute("electrical.read", &json!({"handle":handle})).unwrap();
    edit["backup_dir"] = json!(root);
    edit["library"] = json!(path);
    edit["composition"]["capacity"] = json!(7);
    edit["composition"]["items"] = json!(vec![json!({"name":"Switch","block":"Switch","modules":1}); 7]);
    let result = s.execute("electrical.update", &edit).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 2);
    assert_eq!(s.execute("electrical.read", &json!({"handle":handle})).unwrap()["composition"]["capacity"], 7);
    let saved = cadcraft_io::read(&std::fs::read(result["backup"].as_str().unwrap()).unwrap(), "backup.dxf").unwrap();
    assert_eq!(saved.model.len(), 2);
    let mut edit = s.execute("electrical.read", &json!({"handle":handle})).unwrap();
    let before = s.state().unwrap().doc.clone();
    edit["library"] = json!(path);
    edit["composition"]["capacity"] = json!(6);
    assert!(s.execute("electrical.update", &edit).is_err());
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    edit["composition"]["capacity"] = json!(7);
    edit["composition"]["installation_height_cm"] = Value::Null;
    let file = root.join("not-directory");
    std::fs::write(&file, b"x").unwrap();
    edit["backup_dir"] = json!(file);
    assert!(s.execute("electrical.update", &edit).is_err());
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    edit["backup_dir"] = json!(root);
    s.execute("electrical.update", &edit).unwrap();
    let r = s.execute("electrical.read", &json!({"handle":handle})).unwrap();
    assert!(r["composition"]["installation_height_cm"].is_null());
    s.doc_mut()
        .unwrap()
        .modify_entity(handle, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                i.insert = Vec3::new(120.0, 230.0, 0.0);
                i.scale = Vec3::new(2.0, 2.0, 1.0);
                i.rotation = std::f64::consts::FRAC_PI_2;
            }
        })
        .unwrap();
    let mut transformed = s.execute("electrical.read", &json!({"handle":handle})).unwrap();
    assert_eq!(transformed["at"], json!([120.0, 230.0]));
    assert_eq!(transformed["rotation"], 180.0);
    assert_eq!(transformed["composition"]["factor"], 2.0);
    transformed["composition"]["installation_height_cm"] = json!(80);
    transformed["backup_dir"] = json!(root);
    let updated = s.execute("electrical.update", &transformed).unwrap();
    let block = s.doc().unwrap().block(updated["block"].as_str().unwrap()).unwrap();
    assert!(block.entities.iter().any(|e| matches!(&e.kind,EntityKind::Text(t) if t.value=="H80" && t.rotation==0.0)));
    // Nonuniform scaling is rejected rather than silently changing the symbol proportions.
    s.doc_mut()
        .unwrap()
        .modify_entity(handle, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                i.scale.x = 2.0;
            }
        })
        .unwrap();
    assert!(s.execute("electrical.read", &json!({"handle":handle})).is_err());
    // Different documents and unsupported object types never become edit targets.
    s.new_drawing(true);
    assert!(s.execute("electrical.update", &r).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn provisional_placement_backup_atomic_insert_metadata_roundtrip_and_undo() {
    set_io(IoHooks {
        read: |b, n| cadcraft_io::read(b, n).map_err(|e| e.to_string()),
        write: |d, n| cadcraft_io::write(d, n).map_err(|e| e.to_string()),
        plot: None,
    });
    let root = std::env::temp_dir().join(format!(
        "cadcraft-electrical-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let library = root.join("synthetic.dxf");
    let mut catalog = Drawing::new_metric();
    let mut block = Block::new("Switch");
    block.entities.push(Entity::new(Handle(100), EntityKind::Line(Line { a: Vec3::new(10.0, 20.0, 0.0), b: Vec3::new(15.0, 24.0, 0.0) })));
    catalog.blocks.insert("Switch".into(), Arc::new(block));
    std::fs::write(&library, cadcraft_io::write(&catalog, "library.dxf").unwrap()).unwrap();
    let request = json!({"library":library,"backup_dir":root.join("backups"),"composition":{"capacity":3,"items":[{"name":"Interruttore è","block":"Switch","modules":1}],"factor":1.0,"installation_height_cm":100},"at":[20,30]});
    let mut s = Session::new();
    s.execute("line", &json!({"points":[[0,0],[10,0]]})).unwrap();
    let before = s.state().unwrap().doc.clone();
    s.execute("electrical.prepare", &request).unwrap();
    s.start("electrical.place").unwrap();
    s.input(Input::Point(Vec2::new(20.0, 30.0))).unwrap();
    s.execute("electrical.options", &json!({"rotation":90,"side":3})).unwrap();
    s.input(Input::Enter).unwrap();
    assert_eq!(s.electrical.as_ref().unwrap().phase, 2);
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    s.cancel();
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    assert!(s.start("electrical.place").is_err());
    assert!(!root.join("backups").exists());
    for rotation in [0, 90, 180, 270] {
        for side in 0..4 {
            let mut params = request.clone();
            params["rotation"] = json!(rotation);
            params["side"] = json!(side);
            let result = s.execute("electrical.insert", &params).unwrap();
            assert_eq!(s.doc().unwrap().model.len(), 2);
            let saved = cadcraft_io::read(&std::fs::read(result["backup"].as_str().unwrap()).unwrap(), "backup.dxf").unwrap();
            assert_eq!(saved.model.len(), 1);
            let assembly = s.doc().unwrap().block(result["block"].as_str().unwrap()).unwrap();
            let height = assembly
                .entities
                .iter()
                .find_map(|e| match &e.kind {
                    EntityKind::Text(t) => Some(t),
                    _ => None,
                })
                .unwrap();
            assert_eq!(height.rotation, 0.0);
            assert_eq!(height.value, "H100");
            let box_bounds = cadcraft_engine::geom::Bounds2::from_points(assembly.entities.iter().flat_map(|e| match &e.kind {
                EntityKind::Line(l) => vec![l.a.xy(), l.b.xy()],
                _ => vec![],
            }));
            let h = height.justify_point();
            match side {
                0 => assert!(h.y > box_bounds.max.y),
                1 => assert!(h.y < box_bounds.min.y),
                2 => assert!(h.x > box_bounds.max.x),
                _ => assert!(h.x < box_bounds.min.x),
            }
            let meta = metadata(&s);
            assert_eq!(meta["rotation"].as_f64(), Some(f64::from(rotation)));
            let roundtrip = cadcraft_io::read(&cadcraft_io::write(s.doc().unwrap(), "out.dxf").unwrap(), "out.dxf").unwrap();
            let mut reread = Session::new();
            *reread.doc_mut().unwrap() = roundtrip;
            assert_eq!(metadata(&reread), meta);
            s.execute("undo", &json!({})).unwrap();
            assert_eq!(s.doc().unwrap().model.len(), 1);
            assert!(s.doc().unwrap().block(result["block"].as_str().unwrap()).is_none());
        }
    }
    // A backup failure must not leave a block, layer or entity in the client drawing.
    let mut bad = request.clone();
    bad["backup_dir"] = json!(library);
    let before = s.state().unwrap().doc.clone();
    assert!(s.execute("electrical.insert", &bad).is_err());
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    s.execute("electrical.prepare", &bad).unwrap();
    s.start("electrical.place").unwrap();
    s.input(Input::Point(Vec2::new(20.0, 30.0))).unwrap();
    s.input(Input::Enter).unwrap();
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_some());
    assert!(s.electrical.as_ref().unwrap().error.is_some());
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    s.cancel();
    bad = request.clone();
    bad["composition"]["items"][0]["modules"] = json!(4);
    assert!(s.execute("electrical.prepare", &bad).is_err());
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    // No height: rotation confirmation commits directly, with one undo step.
    let mut no_height = request.clone();
    no_height["composition"]["installation_height_cm"] = Value::Null;
    s.execute("electrical.prepare", &no_height).unwrap();
    s.start("electrical.place").unwrap();
    s.input(Input::Point(Vec2::new(1.0, 2.0))).unwrap();
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none());
    assert_eq!(s.doc().unwrap().model.len(), 2);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 1);
    let mut meters = no_height.clone();
    meters["composition"]["factor"] = json!(0.001);
    let result = s.execute("electrical.insert", &meters).unwrap();
    let assembly = s.doc().unwrap().block(result["block"].as_str().unwrap()).unwrap();
    let bounds = cadcraft_engine::geom::Bounds2::from_points(assembly.entities.iter().flat_map(|e| match &e.kind {
        EntityKind::Line(l) => vec![l.a.xy(), l.b.xy()],
        _ => vec![],
    }));
    assert!((bounds.width() - 0.009).abs() < 1e-10);
    assert!((bounds.height() - 0.005).abs() < 1e-10);
    s.execute("undo", &json!({})).unwrap();
    s.execute("electrical.prepare", &request).unwrap();
    s.start("electrical.place").unwrap();
    s.new_drawing(true);
    assert!(s.running.is_none());
    assert!(s.start("electrical.place").is_err());
    assert_eq!(s.doc().unwrap().model.len(), 0);
    // Test artifacts are confined to this uniquely named directory.
    std::fs::remove_dir_all(root).unwrap();
}
