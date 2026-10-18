//! Original synthetic rooms: geometry, guided cancellation, editing and assignments.
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::doc::{Block, Common, Drawing, EntityKind, Handle, Insert, RawTag, Space};
use cadcraft_engine::geom::{Vec2, Vec3};
use cadcraft_engine::{Input, Session};
use serde_json::{Value, json};
use std::sync::Arc;

fn setup() -> std::path::PathBuf {
    set_io(IoHooks {
        read: |b, n| cadcraft_io::read(b, n).map_err(|e| e.to_string()),
        write: |d, n| cadcraft_io::write(d, n).map_err(|e| e.to_string()),
        plot: None,
    });
    let path = std::env::temp_dir()
        .join(format!("cadcraft-rooms-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&path).unwrap();
    path
}
fn component(s: &mut Session, x: f64, y: f64) -> Handle {
    let d = s.doc_mut().unwrap();
    d.blocks.entry("SyntheticSymbol".into()).or_insert_with(|| Arc::new(Block::new("SyntheticSymbol")));
    let mut common = Common::default();
    common.xdata.push(RawTag { code: 1001, value: "CC_SIMBOLO".into() });
    d.add(
        &Space::Model,
        common,
        EntityKind::Insert(Insert {
            block: "SyntheticSymbol".into(),
            insert: Vec3::new(x, y, 0.0),
            scale: Vec3::new(1.0, 1.0, 1.0),
            rotation: 0.0,
            attribs: vec![],
            cols: 1,
            rows: 1,
            col_spacing: 0.0,
            row_spacing: 0.0,
        }),
    )
    .unwrap()
}
fn status(s: &mut Session, h: Handle) -> Value {
    s.execute("electrical.room.scan", &json!({})).unwrap()["components"].as_array().unwrap().iter().find(|v| v["handle"] == h.hex()).unwrap().clone()
}

#[test]
fn rooms_roundtrip_assignments_disambiguate_and_edits_preserve_identity_and_copies() {
    let root = setup();
    let mut s = Session::new();
    let first = s
        .execute(
            "electrical.room.create",
            &json!({"name":"Cucina","floor":"PT","points":[[0,0],[10,0],[10,10],[0,10]],"label_height":0.5,"backup_dir":root}),
        )
        .unwrap();
    let room = Handle::parse_hex(first["handle"].as_str().unwrap()).unwrap();
    assert!(std::path::Path::new(first["backup"].as_str().unwrap()).is_file());
    assert!(!s.doc().unwrap().layer("EL_AMBIENTI").unwrap().plot);
    let original = s.doc().unwrap().entity(room).unwrap().clone();
    let copy = s.doc_mut().unwrap().add(&Space::Model, original.common.clone(), original.kind.clone()).unwrap();
    // Remove the copy until later, otherwise its overlap would be correctly reported.
    s.doc_mut().unwrap().remove_entity(copy);
    let inside = component(&mut s, 5.0, 5.0);
    let boundary = component(&mut s, 10.0, 5.0);
    let outside = component(&mut s, 30.0, 5.0);
    assert_eq!(status(&mut s, inside)["status"], "automatic");
    assert_eq!(status(&mut s, boundary)["status"], "boundary");
    assert_eq!(status(&mut s, outside)["status"], "outside");
    s.execute("electrical.room.create", &json!({"name":"Soggiorno","points":[[2,2],[8,2],[8,8],[2,8]],"label_height":0.5,"backup_dir":root}))
        .unwrap();
    assert_eq!(status(&mut s, inside)["status"], "overlap");
    s.execute("electrical.room.assign", &json!({"handles":[inside.hex()],"room":room.hex(),"backup_dir":root})).unwrap();
    assert_eq!(status(&mut s, inside)["status"], "manual");
    assert_eq!(status(&mut s, inside)["room"], room.hex());
    let bytes = cadcraft_io::write(s.doc().unwrap(), "rooms.dxf").unwrap();
    *s.doc_mut().unwrap() = cadcraft_io::read(&bytes, "rooms.dxf").unwrap();
    assert_eq!(status(&mut s, inside)["status"], "manual");
    let original = s.doc().unwrap().entity(room).unwrap().clone();
    let copy = s.doc_mut().unwrap().add(&Space::Model, original.common.clone(), original.kind.clone()).unwrap();
    s.execute("electrical.room.prepare", &json!({"handle":room.hex(),"backup_dir":root})).unwrap();
    s.start("electrical.room.place").unwrap();
    s.execute("electrical.room.options", &json!({"name":"Cucina nuova","action":"redraw"})).unwrap();
    for p in [Vec2::new(0.0, 0.0), Vec2::new(12.0, 0.0), Vec2::new(12.0, 12.0), Vec2::new(0.0, 12.0)] {
        s.input(Input::Point(p)).unwrap();
    }
    s.input(Input::Enter).unwrap();
    s.input(Input::Enter).unwrap();
    assert!(s.running.is_none());
    assert_eq!(s.doc().unwrap().entity(copy).unwrap().kind, original.kind);
    let list = s.execute("electrical.room.list", &json!({})).unwrap();
    assert_eq!(list["rooms"].as_array().unwrap().iter().find(|v| v["handle"] == room.hex()).unwrap()["name"], "Cucina nuova");
    assert_eq!(status(&mut s, inside)["room"], room.hex());
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.doc().unwrap().entity(room).unwrap().kind, original.kind);
    s.execute("electrical.room.delete", &json!({"handle":room.hex(),"backup_dir":root})).unwrap();
    assert_eq!(status(&mut s, inside)["status"], "missing_room");
    s.execute("electrical.room.assign", &json!({"handles":[inside.hex()],"room":null,"backup_dir":root})).unwrap();
    assert_eq!(status(&mut s, inside)["status"], "overlap");
    s.doc_mut().unwrap().layer_mut("EL_AMBIENTI").unwrap().locked = true;
    let before = s.state().unwrap().doc.clone();
    assert!(s.execute("electrical.room.create", &json!({"name":"Locked","points":[[0,0],[1,0],[1,1]],"backup_dir":root})).is_err());
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_boundaries_and_backup_failures_are_atomic_and_concave_labels_are_inside() {
    let root = setup();
    let mut s = Session::new();
    for points in [
        json!([[0, 0], [1, 1]]),
        json!([[0, 0], [10, 10], [0, 10], [10, 0]]),
        json!([[0, 0], [1, 0], [1, 0], [0, 1]]),
        json!([[0, 0], [1, 0], [2, 0]]),
    ] {
        let before = s.state().unwrap().doc.clone();
        assert!(s.execute("electrical.room.create", &json!({"name":"Bad","points":points,"backup_dir":root})).is_err());
        assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    }
    let before = s.state().unwrap().doc.clone();
    s.execute("electrical.room.prepare", &json!({"backup_dir":root})).unwrap();
    s.start("electrical.room.place").unwrap();
    s.input(Input::Point(Vec2::ZERO)).unwrap();
    s.cancel();
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    let file = root.join("not-directory");
    std::fs::write(&file, b"x").unwrap();
    let points = json!([[0, 0], [10, 0], [10, 2], [2, 2], [2, 10], [0, 10]]);
    assert!(s.execute("electrical.room.create", &json!({"name":"L","points":points,"label_height":0.5,"backup_dir":file})).is_err());
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    let result = s.execute("electrical.room.create", &json!({"name":"Locale a L","points":points,"label_height":0.5,"backup_dir":root})).unwrap();
    let e = s.doc().unwrap().entity(Handle::parse_hex(result["handle"].as_str().unwrap()).unwrap()).unwrap();
    let EntityKind::Insert(ins) = &e.kind else { panic!() };
    let block = s.doc().unwrap().block(&ins.block).unwrap();
    let p = block.entities.iter().find_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.insert) } else { None }).unwrap();
    let h = component(&mut s, p.x, p.y);
    assert_eq!(status(&mut s, h)["status"], "automatic");
    let bytes = cadcraft_io::write(s.doc().unwrap(), "rooms.dxf").unwrap();
    let drawing: Drawing = cadcraft_io::read(&bytes, "rooms.dxf").unwrap();
    assert!(!drawing.layer("EL_AMBIENTI").unwrap().plot);
    std::fs::remove_dir_all(root).unwrap();
}
