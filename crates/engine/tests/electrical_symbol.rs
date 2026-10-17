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
    let metadata = |d: &Drawing| -> Value {
        let e = d.model.iter().last().unwrap();
        assert_eq!(e.common.layer, "EL_SIMBOLI");
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
    std::fs::remove_dir_all(root).unwrap();
}
