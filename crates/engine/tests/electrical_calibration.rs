//! Original synthetic geometry only: calibration is uniform, scoped, backed up and undoable.
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::{
    Input, Session,
    doc::{EntityKind, Layer},
    geom::Vec2,
};
use serde_json::{Value, json};
use std::sync::Arc;

#[test]
fn known_distance_calibration_is_scoped_atomic_and_uses_explicit_units() {
    set_io(IoHooks {
        read: |b, n| cadcraft_io::read(b, n).map_err(|e| e.to_string()),
        write: |d, n| cadcraft_io::write(d, n).map_err(|e| e.to_string()),
        plot: None,
    });
    let root = std::env::temp_dir()
        .join(format!("cadcraft-calibration-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&root).unwrap();
    let mut s = Session::new();
    s.doc_mut().unwrap().header.set_i64("INSUNITS", 4);
    s.execute("line", &json!({"points":[[10,20],[14,20]]})).unwrap();
    let line = *s.doc().unwrap().model.handles().first().unwrap();
    s.execute("circle", &json!({"center":[20,20],"radius":2})).unwrap();
    let circle = *s.doc().unwrap().model.handles().last().unwrap();
    s.execute("electrical.room.create", &json!({"points":[[0,0],[4,0],[4,4],[0,4]],"name":"Synthetic","label_height":1,"backup_dir":root})).unwrap();
    let original_circle = s.doc().unwrap().entity(circle).unwrap().kind.clone();
    s.set_selection(vec![line]);
    let mut plan = s.execute("electrical.calibration.prepare", &json!({"scope":"selected"})).unwrap();
    assert_eq!(plan["count"], 1);
    assert_eq!(plan["model_unit"], "mm");
    let before = s.state().unwrap().doc.clone();
    s.start("electrical.calibration.pick").unwrap();
    s.input(Input::Point(Vec2::new(10.0, 20.0))).unwrap();
    s.input(Input::Point(Vec2::new(10.0, 20.0))).unwrap();
    assert_eq!(s.electrical_measure.as_ref().unwrap().points.len(), 1);
    s.cancel();
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    s.execute("electrical.calibration.prepare", &json!({"scope":"selected"})).unwrap();
    s.start("electrical.calibration.pick").unwrap();
    s.input(Input::Point(Vec2::new(10.0, 20.0))).unwrap();
    s.input(Input::Point(Vec2::new(14.0, 20.0))).unwrap();
    assert!(s.running.is_none());
    assert_eq!(s.electrical_measure.as_ref().unwrap().points.len(), 2);
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    assert!(s.start("electrical.calibration.pick").is_err());
    plan["p1"] = json!([10, 20]);
    plan["p2"] = json!([14, 20]);
    plan["length"] = json!(80);
    plan["unit"] = json!("cm");
    plan["model_unit"] = json!("mm");
    plan["backup_dir"] = json!(root);
    let result = s.execute("electrical.calibration.apply", &plan).unwrap();
    assert_eq!(result["factor"], 200.0);
    let EntityKind::Line(l) = &s.doc().unwrap().entity(line).unwrap().kind else { panic!("line") };
    assert_eq!(l.a.xy(), Vec2::new(10.0, 20.0));
    assert_eq!(l.b.xy(), Vec2::new(810.0, 20.0));
    assert_eq!(s.doc().unwrap().entity(circle).unwrap().kind, original_circle);
    let backup = std::fs::read(result["backup"].as_str().unwrap()).unwrap();
    let backup = cadcraft_io::read(&backup, "backup.dxf").unwrap();
    let EntityKind::Line(l) = &backup.entity(line).unwrap().kind else { panic!("line") };
    assert_eq!(l.b.x, 14.0);
    let after = s.state().unwrap().doc.clone();
    assert!(s.execute("electrical.calibration.apply", &plan).is_err());
    assert!(Arc::ptr_eq(&after, &s.state().unwrap().doc));
    s.execute("undo", &json!({})).unwrap();
    let EntityKind::Line(l) = &s.doc().unwrap().entity(line).unwrap().kind else { panic!("line") };
    assert_eq!(l.b.x, 14.0);
    // A real length of 0.8 m can be stored in a model expressed in meters; Undo restores header too.
    s.set_selection(vec![line]);
    let fresh = s.execute("electrical.calibration.prepare", &json!({"scope":"selected"})).unwrap();
    plan["revision"] = fresh["revision"].clone();
    plan["length"] = json!(0.8);
    plan["unit"] = json!("m");
    plan["model_unit"] = json!("m");
    s.execute("electrical.calibration.apply", &plan).unwrap();
    assert_eq!(s.doc().unwrap().header.i64("INSUNITS", 0), 6);
    let EntityKind::Line(l) = &s.doc().unwrap().entity(line).unwrap().kind else { panic!("line") };
    assert!((l.b.x - 10.8).abs() < 1e-9);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().header.i64("INSUNITS", 0), 4);
    s.set_selection(vec![line]);
    let fresh = s.execute("electrical.calibration.prepare", &json!({"scope":"selected"})).unwrap();
    plan["revision"] = fresh["revision"].clone();
    let blocked = root.join("not-a-directory");
    std::fs::write(&blocked, b"original").unwrap();
    plan["backup_dir"] = json!(blocked);
    let before = s.state().unwrap().doc.clone();
    assert!(s.execute("electrical.calibration.apply", &plan).is_err());
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    plan["backup_dir"] = json!(root);
    for (key, value) in [("length", json!(0)), ("unit", json!("inches")), ("p2", json!([10, 20])), ("handles", Value::Null)] {
        let mut invalid = plan.clone();
        invalid[key] = value;
        assert!(s.execute("electrical.calibration.apply", &invalid).is_err());
        assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    }
    // Never silently skip a locked object; hidden but unlocked objects are included in whole-plan scaling.
    let mut layer = Layer::new("Synthetic hidden");
    layer.locked = true;
    layer.on = false;
    s.doc_mut().unwrap().layers.push(layer);
    s.doc_mut().unwrap().modify_entity(circle, |e| e.common.layer = "Synthetic hidden".into()).unwrap();
    assert!(s.execute("electrical.calibration.prepare", &json!({"scope":"all"})).is_err());
    assert!(s.execute("electrical.calibration.prepare", &json!({"scope":"all","check":true})).is_ok());
    s.doc_mut().unwrap().layer_mut("Synthetic hidden").unwrap().locked = false;
    let mut all = s.execute("electrical.calibration.prepare", &json!({"scope":"all"})).unwrap();
    assert_eq!(all["count"], 3);
    assert_eq!(all["electrical"], 1);
    for key in ["p1", "p2", "backup_dir"] {
        all[key] = plan[key].clone();
    }
    all["p2"] = json!([13, 24]); // A diagonal reference is measured by its true Euclidean length.
    all["length"] = json!(10);
    all["unit"] = json!("mm");
    all["model_unit"] = json!("mm");
    s.execute("electrical.calibration.apply", &all).unwrap();
    let EntityKind::Circle(c) = &s.doc().unwrap().entity(circle).unwrap().kind else { panic!("circle") };
    assert_eq!(c.center.x, 30.0);
    assert_eq!(c.radius, 4.0);
    let rooms = s.execute("electrical.room.list", &json!({})).unwrap();
    assert_eq!(rooms["rooms"][0]["area_model_units"], 64.0);
    s.execute("undo", &json!({})).unwrap();
    let before = s.state().unwrap().doc.clone();
    s.execute("electrical.calibration.prepare", &json!({"scope":"all","check":true})).unwrap();
    s.start("electrical.calibration.pick").unwrap();
    s.input(Input::Point(Vec2::ZERO)).unwrap();
    s.input(Input::Point(Vec2::new(4.0, 0.0))).unwrap();
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    s.new_drawing(true);
    assert!(s.execute("electrical.calibration.apply", &all).is_err());
    assert!(s.start("electrical.calibration.pick").is_err());
    std::fs::remove_dir_all(root).unwrap();
}
