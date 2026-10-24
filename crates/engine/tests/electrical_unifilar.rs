//! Synthetic templates only: failed creation must preserve every existing drawing.
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::{
    Session,
    doc::{Attrib, Block, Drawing, Entity, EntityKind, RawTag, Text},
    geom::Vec3,
};
use serde_json::json;
use std::sync::Arc;
#[test]
fn new_unifilar_is_independent_atomic_and_preserves_cartiglio_on_roundtrip() {
    set_io(IoHooks {
        read: |b, n| cadcraft_io::read(b, n).map_err(|e| e.to_string()),
        write: |d, n| cadcraft_io::write(d, n).map_err(|e| e.to_string()),
        plot: None,
    });
    let root = std::env::temp_dir()
        .join(format!("cadcraft-unifilar-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&root).unwrap();
    let mut d = Drawing::new_metric();
    let mut block = Block::new("TITLE");
    let mut fields = Vec::new();
    for i in 0..10 {
        let id = if i == 0 { "quadro".into() } else { format!("field{i}") };
        let tag = id.to_uppercase();
        let pos = [10.0, 10.0 + i as f64 * 5.0];
        let a = Attrib {
            tag: tag.clone(),
            text: Text {
                insert: Vec3::new(pos[0], pos[1], 0.0),
                align_pt: None,
                height: 2.0,
                value: String::new(),
                rotation: 0.0,
                width_factor: 1.0,
                oblique: 0.0,
                style: "Standard".into(),
                halign: Default::default(),
                valign: Default::default(),
            },
            invisible: false,
            constant: false,
            prompt: id.clone(),
            props: Default::default(),
            multiline: None,
            modes: Default::default(),
        };
        let h = d.new_handle();
        block.entities.push(Entity::new(h, EntityKind::AttDef(a)));
        fields.push(json!({"id":id,"tag":tag,"position":pos,"width_mm":40,"text_height_mm":2}));
    }
    d.blocks.insert("TITLE".into(), Arc::new(block));
    let mut setup = Session::empty();
    setup.open_drawing(d, "template", None);
    let r = setup.execute("insert", &json!({"name":"TITLE","at":[0,0]})).unwrap();
    let h = cadcraft_engine::doc::Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    setup
        .doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            e.common.xdata = vec![RawTag { code: 1001, value: "CC_UNIFILARE_MODELO".into() }, RawTag { code: 1000, value: "synthetic".into() }]
        })
        .unwrap();
    let bytes = cadcraft_io::write(setup.doc().unwrap(), "template.dxf").unwrap();
    let file = root.join("template.dxf");
    std::fs::write(&file, &bytes).unwrap();
    let model = root.join("modello.json");
    let config =
        json!({"format":"cadcraft-unifilar-template","version":1,"id":"synthetic","units":"mm","template":"template.dxf","title_fields":fields});
    std::fs::write(&model, config.to_string()).unwrap();
    let mut s = Session::new();
    s.execute("line", &json!({"points":[[1,2],[3,4]]})).unwrap();
    let before = s.state().unwrap().doc.clone();
    let uid = s.state().unwrap().uid;
    let count = s.docs.len();
    for badfields in [json!({"quadro":"A".repeat(100)}), json!({"unknown":"x"}), json!({"quadro":"two\nlines"}), json!({"quadro":false})] {
        assert!(s.execute("electrical.unifilar.new", &json!({"model":model,"fields":badfields})).is_err());
        assert_eq!(s.docs.len(), count);
        assert_eq!(s.state().unwrap().uid, uid);
        assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    }
    let r = s.execute("electrical.unifilar.new", &json!({"model":model,"fields":{"quadro":"QG Test","field1":"Cliente"}})).unwrap();
    assert_eq!(r["title"], "QG Test");
    assert_eq!(s.docs.len(), count + 1);
    assert!(s.state().unwrap().path.is_none());
    assert!(s.state().unwrap().is_dirty());
    assert!(Arc::ptr_eq(&s.docs[0].doc, &before));
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    let data = cadcraft_io::write(s.doc().unwrap(), "new.dxf").unwrap();
    let reopened = cadcraft_io::read(&data, "new.dxf").unwrap();
    let e = reopened.model.iter().find_map(|e| if let EntityKind::Insert(i) = &e.kind { Some(i) } else { None }).unwrap();
    assert_eq!(e.attribs.iter().find(|a| a.tag == "QUADRO").unwrap().text.value, "QG Test");
    // A sheet manager can start after every drawing tab was closed.
    let mut empty = Session::empty();
    empty.execute("electrical.unifilar.new", &json!({"model":model,"fields":{"quadro":"Primo foglio"}})).unwrap();
    assert_eq!(empty.docs.len(), 1);
    let active = s.active;
    let count = s.docs.len();
    std::fs::write(&model, config.to_string().replace("template.dxf", "../template.dxf")).unwrap();
    assert!(s.execute("electrical.unifilar.new", &json!({"model":model})).is_err());
    assert_eq!(s.docs.len(), count);
    assert_eq!(s.active, active);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn unifilar_placement_owns_column_data_is_atomic_and_markers_do_not_occupy_columns() {
    use cadcraft_engine::{
        Input,
        doc::{Common, Insert, Line, Space},
        geom::Vec2,
    };
    set_io(IoHooks {
        read: |b, n| cadcraft_io::read(b, n).map_err(|e| e.to_string()),
        write: |d, n| cadcraft_io::write(d, n).map_err(|e| e.to_string()),
        plot: None,
    });
    let root = std::env::temp_dir()
        .join(format!("unifilar-place-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir(&root).unwrap();
    let mut library = Drawing::new_metric();
    for (name, height) in [("DEVICE", 20.0), ("MARK", 5.0)] {
        let mut block = Block::new(name);
        let h = library.new_handle();
        block.entities.push(Entity::new(h, EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(0.0, -height, 0.0) })));
        let h = library.new_handle();
        block.entities.push(Entity::new(h, EntityKind::Line(Line { a: Vec3::new(-2.0, -height / 2.0, 0.0), b: Vec3::new(2.0, -height / 2.0, 0.0) })));
        library.blocks.insert(name.into(), Arc::new(block));
    }
    std::fs::write(root.join("symbols.dxf"), cadcraft_io::write(&library, "symbols.dxf").unwrap()).unwrap();
    let mut cols = Vec::new();
    for i in 0..2 {
        let x = 10.0 + i as f64 * 35.0;
        cols.push(json!({"index":i+1,"bounds":[x,20,x+35.0,220],"axis_x":x+17.5,"symbol_y_range":[100,200],"cells":{
        "sigla":{"bounds":[x,38,x+35.0,43],"text_center":[x+17.5,40.5],"padding_mm":1.5,"max_lines":1},
        "descrizione":{"bounds":[x,20,x+35.0,38],"text_center":[x+17.5,29],"padding_mm":1.5,"max_lines":3}}}));
    }
    let config = json!({"format":"cadcraft-unifilar-template","version":1,"id":"synthetic","units":"mm","catalog":"catalog.json","columns":cols,"table":{"rows":[{"id":"sigla","label":"Sigla"},{"id":"descrizione","label":"Descrizione"}]},"drawing_area":[0,80,100,220],"pe":{"y":90}});
    let model = root.join("model.json");
    std::fs::write(&model, config.to_string()).unwrap();
    let catalog = json!({"format":"cadcraft-unifilar-symbols","version":1,"library":"symbols.dxf","symbols":[
        {"id":"device","name":"Synthetic device","block":"DEVICE","category":"Protection","kind":"device","fields":["sigla","descrizione"],"ports":[]},
        {"id":"marker","name":"Synthetic marker","block":"MARK","category":"Lines","kind":"line_marker","fields":[],"ports":[]}]});
    std::fs::write(root.join("catalog.json"), catalog.to_string()).unwrap();
    let mut sheet = Drawing::new_metric();
    sheet.blocks.insert("TITLE".into(), Arc::new(Block::new("TITLE")));
    let common = Common {
        xdata: vec![RawTag { code: 1001, value: "CC_UNIFILARE_MODELO".into() }, RawTag { code: 1000, value: "synthetic".into() }],
        ..Default::default()
    };
    sheet
        .add(
            &Space::Model,
            common,
            EntityKind::Insert(Insert {
                block: "TITLE".into(),
                insert: Vec3::ZERO,
                scale: Vec3::new(1.0, 1.0, 1.0),
                rotation: 0.0,
                attribs: vec![],
                cols: 1,
                rows: 1,
                col_spacing: 0.0,
                row_spacing: 0.0,
            }),
        )
        .unwrap();
    let mut s = Session::empty();
    s.open_drawing(sheet, "Synthetic", None);
    let mut p = json!({"model":model,"id":"device","fields":{"sigla":"Q1","descrizione":"First circuit"},"at":[15,180],"backup_dir":root});
    let first = s.execute("electrical.unifilar.insert", &p).unwrap();
    let h = cadcraft_engine::doc::Handle::parse_hex(first["handle"].as_str().unwrap()).unwrap();
    let meta = s.execute("electrical.unifilar.read", &json!({"handle":h.hex()})).unwrap();
    assert_eq!(meta["column"], 1);
    assert_eq!(meta["at"], json!([27.5, 180.0]));
    assert!(std::path::Path::new(first["backup"].as_str().unwrap()).exists());
    let table_world = |s: &Session, h| {
        let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!() };
        s.doc()
            .unwrap()
            .block(&i.block)
            .unwrap()
            .entities
            .iter()
            .filter_map(
                |e| if let EntityKind::Text(t) = &e.kind { (t.value == "First circuit").then_some(t.insert.xy() + i.insert.xy()) } else { None },
            )
            .collect::<Vec<_>>()
    };
    let initial_table = table_world(&s, h);
    assert_eq!(initial_table.len(), 1);
    let before = s.state().unwrap().doc.clone();
    assert!(s.execute("electrical.unifilar.insert", &p).is_err());
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    p["at"] = json!([60, 150]);
    p["fields"] = json!({"sigla":"Q2"});
    let second = s.execute("electrical.unifilar.insert", &p).unwrap();
    assert_eq!(second["column"], 2);
    let copy = s.execute("copy", &json!({"handles":[h.hex()],"delta":[0,0]})).unwrap();
    let _ = copy;
    s.execute("u", &json!({})).unwrap();
    let mut edit = s.execute("electrical.unifilar.read", &json!({"handle":h.hex()})).unwrap();
    edit["original"] = edit.clone();
    s.touch();
    edit["model"] = json!(model);
    edit["backup_dir"] = json!(root);
    edit["at"] = json!([20, 130]);
    s.execute("electrical.unifilar.update", &edit).unwrap();
    assert_eq!(initial_table, table_world(&s, h));
    let moved = s.execute("electrical.unifilar.read", &json!({"handle":h.hex()})).unwrap();
    assert_eq!(moved["at"], json!([27.5, 130.0]));
    let mut occupied = moved.clone();
    occupied["model"] = json!(model);
    occupied["backup_dir"] = json!(root);
    occupied["at"] = json!([60, 180]);
    let before = s.state().unwrap().doc.clone();
    assert!(s.execute("electrical.unifilar.update", &occupied).is_err());
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    s.execute("u", &json!({})).unwrap();
    assert_eq!(s.execute("electrical.unifilar.read", &json!({"handle":h.hex()})).unwrap()["at"], json!([27.5, 180.0]));
    let before = s.state().unwrap().doc.clone();
    p["fields"] = json!({"sigla":"A".repeat(200)});
    assert!(s.execute("electrical.unifilar.prepare", &p).is_err());
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    p["fields"] = json!({"descrizione":"uno\ndue\ntre\nquattro"});
    assert!(s.execute("electrical.unifilar.prepare", &p).is_err());
    s.execute("line", &json!({"points":[[90,120],[90,190]]})).unwrap();
    let marker = json!({"model":model,"id":"marker","at":[90.5,170],"fields":{},"backup_dir":root});
    let result = s.execute("electrical.unifilar.insert", &marker).unwrap();
    assert!(result["column"].is_null());
    let m = s.execute("electrical.unifilar.read", &json!({"handle":result["handle"]})).unwrap();
    assert_eq!(m["at"], json!([90.0, 170.0]));
    assert_eq!(m["kind"], "line_marker");
    let bad = root.join("not-directory");
    std::fs::write(&bad, b"x").unwrap();
    let mut fail = marker.clone();
    fail["backup_dir"] = json!(bad);
    let before = s.state().unwrap().doc.clone();
    assert!(s.execute("electrical.unifilar.insert", &fail).is_err());
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    let saved = cadcraft_io::write(s.doc().unwrap(), "roundtrip.dxf").unwrap();
    let reopened = cadcraft_io::read(&saved, "roundtrip.dxf").unwrap();
    let mut re = Session::empty();
    re.open_drawing(reopened, "roundtrip", None);
    assert_eq!(re.execute("electrical.unifilar.read", &json!({"handle":h.hex()})).unwrap()["fields"]["descrizione"], "First circuit");
    p["fields"] = json!({});
    s.execute("electrical.unifilar.prepare", &p).unwrap();
    s.touch(); // Zoom/pan redraws may change the revision without changing drawing contents.
    s.start("electrical.unifilar.place").unwrap();
    let before = s.state().unwrap().doc.clone();
    s.cancel();
    assert!(Arc::ptr_eq(&before, &s.state().unwrap().doc));
    s.execute("electrical.unifilar.prepare", &p).unwrap();
    s.execute("new", &json!({"metric":true})).unwrap();
    assert!(s.start("electrical.unifilar.place").is_err());
    let _ = Input::Enter;
    let _ = Vec2::ZERO;
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn opening_unifilar_aligns_half_columns_without_changing_files_or_plan_settings() {
    use cadcraft_engine::geom::Vec2;
    let mut setup = Session::empty();
    let mut d = Drawing::new_metric();
    d.blocks.insert("TITLE".into(), Arc::new(Block::new("TITLE")));
    setup.open_drawing(d, "template", None);
    let r = setup.execute("insert", &json!({"name":"TITLE","at":[0,0]})).unwrap();
    let h = cadcraft_engine::doc::Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    setup
        .doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            e.common.xdata = vec![RawTag { code: 1001, value: "CC_UNIFILARE_MODELO".into() }, RawTag { code: 1000, value: "a3-10col-v1".into() }];
        })
        .unwrap();
    let bytes = cadcraft_io::write(setup.doc().unwrap(), "sheet.dxf").unwrap();
    let sheet = cadcraft_io::read(&bytes, "sheet.dxf").unwrap();
    let mut s = Session::empty();
    s.settings.gridmode = false;
    let plan = s.open_drawing(Drawing::new_metric(), "Plan", None);
    let plan_settings = s.drafting_settings();
    let index = s.open_drawing(sheet, "Unifilar", None);
    let original = s.state().unwrap().doc.clone();
    assert!(!s.state().unwrap().is_dirty());
    let grid = s.state().unwrap().unifilar_grid.unwrap();
    assert_eq!(grid.origin, Vec2::new(55.0, 0.0));
    assert_eq!(s.drafting_settings().gridunit.x, 17.75);
    assert!(s.drafting_settings().gridmode);
    assert!(s.drafting_settings().snapmode);
    assert_eq!(s.drafting_settings().snapunit.y, 12.5);
    s.settings.osmode = 0;
    s.settings.otrack = false;
    s.settings.polarmode = false;
    let q =
        cadcraft_engine::snap::tracking::CursorQuery { base: None, deferred: false, corner: false, aperture: 0.1, tol: 0.1, now: 0.0, shift: false };
    let raw = Vec2::new(133.567, 237.1);
    assert_eq!(s.snap_cursor(raw, &q).point, Vec2::new(raw.x, 237.5));
    s.viewport_px = (4000.0, 3000.0);
    assert_eq!(s.snap_cursor(raw, &q).point, Vec2::new(raw.x, 237.5), "zoom must not change row heights");
    assert_eq!(s.snap_cursor(Vec2::new(raw.x, -11.8), &q).point, Vec2::new(raw.x, -12.5));
    s.execute("snap", &json!({})).unwrap();
    assert!(!s.drafting_settings().snapmode);
    assert_eq!(s.snap_cursor(raw, &q).point, raw);
    s.execute("snap", &json!({})).unwrap();
    assert!(s.drafting_settings().snapmode);
    for i in 0..10 {
        let left = 55.0 + i as f64 * 35.5;
        assert_eq!(grid.origin.x + (i * 2) as f64 * grid.spacing.x, left);
        assert_eq!(grid.origin.x + (i * 2 + 1) as f64 * grid.spacing.x, left + 17.75);
    }
    s.execute("grid", &json!({})).unwrap();
    assert!(!s.drafting_settings().gridmode);
    s.execute("grid", &json!({})).unwrap();
    assert!(s.drafting_settings().gridmode);
    assert!(Arc::ptr_eq(&original, &s.state().unwrap().doc));
    s.active = plan;
    assert_eq!(s.drafting_settings().gridunit, plan_settings.gridunit);
    assert_eq!(s.drafting_settings().gridmode, plan_settings.gridmode);
    assert_eq!(s.drafting_settings().snapmode, plan_settings.snapmode);
    assert_eq!(s.drafting_settings().snapunit, plan_settings.snapunit);
    s.active = index;
    assert_eq!(s.drafting_settings().gridunit.x, 17.75);
    let reopened = cadcraft_io::read(&bytes, "sheet.dxf").unwrap();
    s.open_drawing(reopened, "Reopened", None);
    assert_eq!(s.drafting_settings().gridunit.x, 17.75);
    assert!(!s.state().unwrap().is_dirty());
    s.execute("line", &json!({"points":[[160,233],[160,240]]})).unwrap();
    s.settings.osmode = cadcraft_engine::snap::mode::END;
    assert_eq!(s.snap_cursor(Vec2::new(160.01, 233.02), &q).point, Vec2::new(160.0, 233.0), "ports/endpoints take precedence over rows");
}
