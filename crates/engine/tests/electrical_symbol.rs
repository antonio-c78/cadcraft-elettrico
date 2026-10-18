//! Original synthetic geometry: standalone placement must never create a box frame.
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::{Block, Drawing, Entity, EntityKind, Handle, Line};
use cadcraft_engine::geom::{Vec2, Vec3};
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};
use std::sync::Arc;

#[test]
fn symbol_placement_is_provisional_guided_backed_up_atomic_and_roundtrips() {
    set_io(IoHooks {
        read: |b, n| cadcraft_io::read(b, n).map_err(|e| e.to_string()),
        write: |d, n| cadcraft_io::write(d, n).map_err(|e| e.to_string()),
        plot: None,
    });
    let root = std::env::temp_dir()
        .join(format!("cadcraft-symbol-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("library.dxf");
    let mut library = Drawing::new_metric();
    let mut block = Block::new("Synthetic");
    block.entities.push(Entity::new(Handle(100), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(3.0, 4.0, 0.0) })));
    library.blocks.insert("Synthetic".into(), Arc::new(block));
    std::fs::write(&path, cadcraft_io::write(&library, "library.dxf").unwrap()).unwrap();
    let params = json!({"library":path,"backup_dir":root,"symbol":{"id":"original-symbol","name":"Synthetic symbol","block":"Synthetic","category":["Synthetic"],"factor":10.0,"width_mm":8.0,"height_mm":8.0,"anchor":[0.5,0.5]}});
    let mut s = Session::new();
    let before = s.state().unwrap().doc.clone();
    s.execute("electrical.symbol.prepare", &params).unwrap();
    s.start("electrical.symbol.place").unwrap();
    s.input(Input::Point(Vec2::new(20.0, 30.0))).unwrap();
    assert_eq!(s.electrical.as_ref().unwrap().phase, 1);
    s.execute("electrical.options", &json!({"rotation":90})).unwrap();
    s.input(Input::Enter).unwrap();
    assert_eq!(s.electrical.as_ref().unwrap().phase, 3);
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    assert!(s.execute("electrical.options", &json!({"height_cm":-1})).is_err());
    s.execute("electrical.options", &json!({"height_cm":100,"side":2})).unwrap();
    s.input(Input::Enter).unwrap();
    assert_eq!(s.electrical.as_ref().unwrap().phase, 2);
    s.cancel();
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    let mut inserted = params.clone();
    inserted["at"] = json!([20, 30]);
    inserted["rotation"] = json!(90);
    inserted["height_cm"] = json!(100);
    inserted["side"] = json!(2);
    let result = s.execute("electrical.symbol.insert", &inserted).unwrap();
    assert!(std::path::Path::new(result["backup"].as_str().unwrap()).is_file());
    let block = s.doc().unwrap().block(result["block"].as_str().unwrap()).unwrap();
    assert_eq!(block.entities.len(), 2); // One original line plus label; no enclosing box.
    let text = block.entities.iter().find_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t) } else { None }).unwrap();
    assert_eq!(text.value, "H100");
    assert_eq!(text.rotation, 0.0);
    assert!(text.insert.x > 40.0);
    for entity in block.entities.iter() {
        assert_eq!(entity.common.color, cadcraft_color::Color::ByLayer);
        assert_eq!(entity.common.layer, if matches!(&entity.kind, EntityKind::Text(_)) { "EL_ALTEZZE" } else { "0" });
    }
    for layer in ["EL_COMPONENTI", "EL_ALTEZZE", "EL_DESCRIZIONI", "EL_LEGENDA"] {
        assert!(s.doc().unwrap().layer(layer).is_some());
    }
    let mut hidden = s.doc().unwrap().clone();
    let visible = cadcraft_render::build(&hidden, &cadcraft_engine::doc::Space::Model, &Default::default());
    hidden.layer_mut("EL_ALTEZZE").unwrap().on = false;
    let no_height = cadcraft_render::build(&hidden, &cadcraft_engine::doc::Space::Model, &Default::default());
    assert!(!no_height.prims.is_empty());
    assert!(no_height.prims.len() < visible.prims.len());
    hidden.layer_mut("EL_COMPONENTI").unwrap().on = false;
    assert!(cadcraft_render::build(&hidden, &cadcraft_engine::doc::Space::Model, &Default::default()).prims.is_empty());
    let metadata = |d: &Drawing| -> Value {
        let e = d.model.iter().last().unwrap();
        assert_eq!(e.common.layer, "EL_COMPONENTI");
        assert!(e.common.xdata.iter().any(|t| t.code == 1001 && t.value == "CC_SIMBOLO"));
        let text: String = e.common.xdata.iter().filter(|t| t.code == 1000).map(|t| t.value.as_str()).collect();
        serde_json::from_str(&text).unwrap()
    };
    assert_eq!(metadata(s.doc().unwrap())["symbol"]["id"], "original-symbol");
    let bytes = cadcraft_io::write(s.doc().unwrap(), "drawing.dxf").unwrap();
    assert_eq!(metadata(&cadcraft_io::read(&bytes, "drawing.dxf").unwrap())["installation_height_cm"], 100.0);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().model.len(), 0);
    // Blank height skips label placement after height confirmation.
    s.execute("electrical.symbol.prepare", &params).unwrap();
    s.start("electrical.symbol.place").unwrap();
    s.input(Input::Point(Vec2::ZERO)).unwrap();
    s.input(Input::Enter).unwrap();
    assert_eq!(s.electrical.as_ref().unwrap().phase, 3);
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none());
    let b = s.doc().unwrap().blocks.values().find(|b| b.name.starts_with("CC_SIMBOLO")).unwrap();
    assert_eq!(b.entities.len(), 1);
    let before = s.state().unwrap().doc.clone();
    let mut bad = inserted.clone();
    bad["backup_dir"] = json!(path); // A file cannot be a backup directory.
    assert!(s.execute("electrical.symbol.insert", &bad).is_err());
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    let mut custom = inserted.clone();
    custom["layers"] = json!({"components":"TEST_COMPONENTS","heights":"TEST_HEIGHTS","descriptions":"TEST_DESCRIPTIONS","legend":"TEST_LEGEND"});
    s.execute("electrical.symbol.insert", &custom).unwrap();
    assert_eq!(s.doc().unwrap().model.iter().last().unwrap().common.layer, "TEST_COMPONENTS");
    s.doc_mut().unwrap().layer_mut("TEST_HEIGHTS").unwrap().locked = true;
    let before = s.state().unwrap().doc.clone();
    assert!(s.execute("electrical.symbol.insert", &custom).is_err());
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    custom["layers"]["heights"] = json!("TEST_COMPONENTS");
    assert!(s.execute("electrical.symbol.prepare", &custom).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn symbol_edits_preserve_reference_copies_assignment_and_are_atomic() {
    use cadcraft_engine::doc::{RawTag, Space};
    set_io(IoHooks {
        read: |b, n| cadcraft_io::read(b, n).map_err(|e| e.to_string()),
        write: |d, n| cadcraft_io::write(d, n).map_err(|e| e.to_string()),
        plot: None,
    });
    let root = std::env::temp_dir()
        .join(format!("cadcraft-symbol-edit-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&root).unwrap();
    let library_path = root.join("library.dxf");
    let mut library = Drawing::new_metric();
    for (name, end) in [("First", Vec3::new(3.0, 4.0, 0.0)), ("Second", Vec3::new(4.0, 1.0, 0.0))] {
        let mut block = Block::new(name);
        block.entities.push(Entity::new(Handle(100), EntityKind::Line(Line { a: Vec3::ZERO, b: end })));
        library.blocks.insert(name.into(), Arc::new(block));
    }
    std::fs::write(&library_path, cadcraft_io::write(&library, "library.dxf").unwrap()).unwrap();
    let mut s = Session::new();
    let request = json!({"library":library_path,"backup_dir":root,"at":[25,35],"rotation":90,"height_cm":100,"side":2,
        "symbol":{"id":"first","name":"First","block":"First","category":[],"factor":10.0,"width_mm":8,"height_mm":8,"anchor":[0.5,0.5]}});
    s.execute("electrical.symbol.insert", &request).unwrap();
    let handle = *s.doc().unwrap().model.handles().first().unwrap();
    s.doc_mut()
        .unwrap()
        .modify_entity(handle, |e| {
            e.common.xdata.push(RawTag { code: 1001, value: "CC_AMBIENTE_REF".into() });
            e.common.xdata.push(RawTag { code: 1000, value: "{\"room\":\"F00\"}".into() });
        })
        .unwrap();
    let original = s.doc().unwrap().entity(handle).unwrap().clone();
    let copy = s.doc_mut().unwrap().add(&Space::Model, original.common.clone(), original.kind.clone()).unwrap();
    let mut p = s.execute("electrical.symbol.read", &json!({"handle":handle.hex()})).unwrap();
    assert_eq!(p["at"], json!([25.0, 35.0]));
    p["backup_dir"] = json!(root);
    p["height_cm"] = json!(125);
    p["side"] = json!(3);
    let result = s.execute("electrical.symbol.update", &p).unwrap();
    assert!(std::path::Path::new(result["backup"].as_str().unwrap()).is_file());
    let updated = s.execute("electrical.symbol.read", &json!({"handle":handle.hex()})).unwrap();
    assert_eq!(updated["installation_height_cm"], 125.0);
    assert_eq!(updated["side"], 3);
    assert_eq!(updated["rotation"], 90.0);
    assert_eq!(s.doc().unwrap().entity(copy).unwrap().kind, original.kind);
    assert_eq!(s.doc().unwrap().entity(handle).unwrap().common.xdata.iter().filter(|t| t.code == 1001 && t.value == "CC_SIMBOLO").count(), 1);
    assert!(s.doc().unwrap().entity(handle).unwrap().common.xdata.iter().any(|t| t.value == "CC_AMBIENTE_REF"));
    let before = s.state().unwrap().doc.clone();
    assert!(s.execute("electrical.symbol.update", &p).is_err()); // Stale revision.
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.execute("electrical.symbol.read", &json!({"handle":handle.hex()})).unwrap()["installation_height_cm"], 100.0);
    let mut replacement = s.execute("electrical.symbol.read", &json!({"handle":handle.hex()})).unwrap();
    replacement["symbol"]["id"] = json!("second");
    replacement["symbol"]["name"] = json!("Second");
    replacement["symbol"]["block"] = json!("Second");
    replacement["library"] = json!(library_path);
    replacement["backup_dir"] = json!(root);
    s.execute("electrical.symbol.update", &replacement).unwrap();
    let updated = s.execute("electrical.symbol.read", &json!({"handle":handle.hex()})).unwrap();
    assert_eq!(updated["symbol"]["id"], "second");
    assert_eq!(updated["at"], json!([25.0, 35.0]));
    assert_eq!(updated["rotation"], 90.0);
    assert_eq!(s.doc().unwrap().entity(copy).unwrap().kind, original.kind);
    let roundtrip = cadcraft_io::read(&cadcraft_io::write(s.doc().unwrap(), "drawing.dxf").unwrap(), "drawing.dxf").unwrap();
    *s.doc_mut().unwrap() = roundtrip;
    assert_eq!(s.execute("electrical.symbol.read", &json!({"handle":handle.hex()})).unwrap()["symbol"]["id"], "second");
    let mut p = s.execute("electrical.symbol.read", &json!({"handle":handle.hex()})).unwrap();
    p["backup_dir"] = json!(library_path);
    p["height_cm"] = Value::Null;
    let before = s.state().unwrap().doc.clone();
    assert!(s.execute("electrical.symbol.update", &p).is_err());
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    p["backup_dir"] = json!(root);
    s.execute("electrical.symbol.update", &p).unwrap();
    assert!(s.execute("electrical.symbol.read", &json!({"handle":handle.hex()})).unwrap()["installation_height_cm"].is_null());
    // Native CAD transforms must be incorporated once, with height text rebuilt horizontally.
    s.doc_mut()
        .unwrap()
        .modify_entity(handle, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                i.scale = Vec3::new(1.5, 1.5, 1.0);
                i.rotation = std::f64::consts::FRAC_PI_4;
            }
        })
        .unwrap();
    let mut p = s.execute("electrical.symbol.read", &json!({"handle":handle.hex()})).unwrap();
    assert_eq!(p["symbol"]["factor"], 15.0);
    assert_eq!(p["rotation"], 135.0);
    p["height_cm"] = json!(110);
    p["backup_dir"] = json!(root);
    let result = s.execute("electrical.symbol.update", &p).unwrap();
    let reread = s.execute("electrical.symbol.read", &json!({"handle":handle.hex()})).unwrap();
    assert_eq!(reread["symbol"]["factor"], 15.0);
    assert_eq!(reread["rotation"], 135.0);
    let block = s.doc().unwrap().block(result["block"].as_str().unwrap()).unwrap();
    assert!(block.entities.iter().any(|e| matches!(&e.kind,EntityKind::Text(t) if t.value=="H110"&&t.rotation==0.0)));
    std::fs::remove_dir_all(root).unwrap();
}
