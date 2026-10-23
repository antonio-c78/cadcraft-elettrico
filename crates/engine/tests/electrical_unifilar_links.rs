//! Synthetic symbol geometry only; no external library artwork is embedded in tests.
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::{
    Session,
    doc::{Block, Drawing, Entity, EntityKind, Handle, Line, RawTag},
    geom::Vec3,
};
use serde_json::{Value, json};
use std::sync::Arc;
struct Fixture {
    s: Session,
    root: std::path::PathBuf,
    model: String,
    devices: Vec<String>,
}
fn block(d: &mut Drawing, name: &str) -> Block {
    let mut b = Block::new(name);
    for (a, c) in [
        ([0., 0.], [0., -2.]),
        ([-2., -2.], [2., -2.]),
        ([-2., -2.], [-2., -10.]),
        ([2., -2.], [2., -10.]),
        ([-2., -10.], [2., -10.]),
        ([0., -10.], [0., -12.]),
    ] {
        b.entities.push(Entity::new(d.new_handle(), EntityKind::Line(Line { a: Vec3::new(a[0], a[1], 0.), b: Vec3::new(c[0], c[1], 0.) })));
    }
    b
}
static FIXTURE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
fn fixture() -> Fixture {
    set_io(IoHooks {
        read: |b, n| cadcraft_io::read(b, n).map_err(|e| e.to_string()),
        write: |d, n| cadcraft_io::write(d, n).map_err(|e| e.to_string()),
        plot: None,
    });
    let root = std::env::temp_dir().join(format!(
        "cadcraft-link-test-{}-{}-{}",
        std::process::id(),
        FIXTURE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let mut library = Drawing::new_metric();
    let b = block(&mut library, "DEVICE");
    library.blocks.insert("DEVICE".into(), Arc::new(b));
    let mut marker = Block::new("MARK");
    for (a, b) in [([0., -2.], [0., 2.]), ([-1., -0.5], [1., 0.5])] {
        marker
            .entities
            .push(Entity::new(library.new_handle(), EntityKind::Line(Line { a: Vec3::new(a[0], a[1], 0.), b: Vec3::new(b[0], b[1], 0.) })));
    }
    library.blocks.insert("MARK".into(), Arc::new(marker));
    std::fs::write(root.join("library.dxf"), cadcraft_io::write(&library, "library.dxf").unwrap()).unwrap();
    let columns:Vec<_>=(0..4).map(|i|{let x=10.+35.*i as f64;json!({"index":i+1,"bounds":[x,30,x+35.,287],"axis_x":x+17.5,"symbol_y_range":[171,282],"cells":{"sigla":{"bounds":[x,146,x+35.,153],"text_center":[x+17.5,149.5],"padding_mm":1.5,"max_lines":1}}})}).collect();
    let model = root.join("modello.json").to_string_lossy().into_owned();
    std::fs::write(&model,json!({"format":"cadcraft-unifilar-template","version":1,"id":"test","units":"mm","catalog":"catalogo.json","columns":columns,"table":{"rows":[{"id":"sigla","label":"Sigla"}]},"drawing_area":[0,153,180,287],"pe":{"y":161,"x_range":[5,170],"layer":"UNI_PE"}}).to_string()).unwrap();
    let input = json!({"id":"in","role":"power","position":[0,0,0]});
    let output = json!({"id":"out","role":"power","position":[0,-12,0]});
    let pe = json!({"id":"pe","role":"pe","position":[0,-12,0]});
    std::fs::write(
        root.join("catalogo.json"),
        json!({"format":"cadcraft-unifilar-symbols","version":1,"library":"library.dxf","symbols":[
            {"id":"dev","name":"Device","block":"DEVICE","category":"Test","kind":"device","fields":["sigla"],"ports":[input,output]},
            {"id":"spd","name":"SPD","block":"DEVICE","category":"Test","kind":"device","fields":["sigla"],"ports":[input,pe]},
            {"id":"mark","name":"Marker","block":"MARK","category":"Test","kind":"line_marker","fields":[],"ports":[]}
        ]})
        .to_string(),
    )
    .unwrap();
    let mut d = Drawing::new_metric();
    d.blocks.insert("TITLE".into(), Arc::new(Block::new("TITLE")));
    let mut s = Session::empty();
    s.open_drawing(d, "test", None);
    let r = s.execute("insert", &json!({"name":"TITLE","at":[0,0]})).unwrap();
    let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
    s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            e.common.xdata = vec![RawTag { code: 1001, value: "CC_UNIFILARE_MODELO".into() }, RawTag { code: 1000, value: "test".into() }];
        })
        .unwrap();
    let mut devices = vec![];
    for (i, (x, y)) in [(27.5, 275.), (62.5, 250.), (97.5, 250.), (132.5, 237.5)].into_iter().enumerate() {
        let r = s
            .execute(
                "electrical.unifilar.insert",
                &json!({"model":model,"id":if i==3{"spd"}else{"dev"},"at":[x,y],"fields":{"sigla":format!("Q{i}")},"backup_dir":root}),
            )
            .unwrap();
        devices.push(r["handle"].as_str().unwrap().into());
    }
    Fixture { s, root, model, devices }
}
impl Fixture {
    fn link(&mut self, l: Value) -> Value {
        self.s.execute("electrical.unifilar.link.insert", &json!({"model":self.model,"link":l,"backup_dir":self.root})).unwrap()
    }
    fn port(&self, i: usize, id: &str) -> Value {
        json!({"handle":self.devices[i],"port":id})
    }
    fn path(&mut self, h: &Value) -> Vec<[f64; 2]> {
        let handle = Handle::parse_hex(h.as_str().unwrap()).unwrap();
        let e = self.s.doc().unwrap().entity(handle).unwrap();
        let mut active = false;
        let mut text = String::new();
        for t in &e.common.xdata {
            if t.code == 1001 {
                active = t.value == "CC_UNIFILARE_LINK";
            } else if active && t.code == 1000 {
                text.push_str(&t.value);
            }
        }
        serde_json::from_value(serde_json::from_str::<Value>(&text).unwrap()["path"].clone()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if self.root.starts_with(std::env::temp_dir()) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}
#[test]
fn symbol_axis_is_always_the_column_center_even_with_legacy_offsets() {
    let mut f = fixture();
    let mut model: Value = serde_json::from_slice(&std::fs::read(&f.model).unwrap()).unwrap();
    model["table"]["rows"].as_array_mut().unwrap().push(json!({"id":"tipo","label":"Tipo"}));
    for col in model["columns"].as_array_mut().unwrap() {
        let x = col["bounds"][0].as_f64().unwrap();
        col["cells"]["tipo"] = json!({"bounds":[x,140.,x+35.,146.],"text_center":[x+17.5,143.],"padding_mm":1.5,"max_lines":1});
    }
    std::fs::write(&f.model, model.to_string()).unwrap();
    let h = Handle::parse_hex(&f.devices[0]).unwrap();
    let mut old = f.s.execute("electrical.unifilar.read", &json!({"handle":h.hex()})).unwrap();
    old.as_object_mut().unwrap().retain(|k, _| !["handle", "uid", "revision"].contains(&k.as_str()));
    old["at"] = json!([22.5, 275.]);
    f.s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            if let EntityKind::Insert(i) = &mut e.kind {
                i.insert.x = 22.5;
            }
            e.common.xdata = vec![RawTag { code: 1001, value: "CC_UNIFILARE_DEVICE".into() }, RawTag { code: 1000, value: old.to_string() }];
        })
        .unwrap();
    let cat_path = f.root.join("catalogo.json");
    let mut cat: Value = serde_json::from_slice(&std::fs::read(&cat_path).unwrap()).unwrap();
    let mut command = cat["symbols"][0].clone();
    command["id"] = json!("command");
    command["name"] = json!("Contattore");
    command["fields"] = json!(["sigla", "tipo"]);
    command["defaults"] = json!({"tipo":"Contattore"});
    command["data_position"] = json!("right");
    command["axis_offset_mm"] = json!(-5.);
    cat["symbols"].as_array_mut().unwrap().push(command);
    std::fs::write(&cat_path, cat.to_string()).unwrap();
    let added = f
        .s
        .execute("electrical.unifilar.insert", &json!({"model":f.model,"id":"command","at":[20.,235.],"fields":{"sigla":"KM1"},"backup_dir":f.root}))
        .unwrap();
    let read = f.s.execute("electrical.unifilar.read", &json!({"handle":added["handle"]})).unwrap();
    assert_eq!(read["at"], json!([27.5, 235.]));
    assert_eq!(read["fields"]["tipo"], "Contattore");
    assert_eq!(f.s.execute("electrical.unifilar.read", &json!({"handle":h.hex()})).unwrap()["at"], json!([22.5, 275.]));
    let link = f.link(json!({"version":1,"kind":"series","from":f.port(0,"out"),"to":f.port(1,"in"),"y":262.5}));
    let mut edit = f.s.execute("electrical.unifilar.read", &json!({"handle":h.hex()})).unwrap();
    edit["model"] = json!(f.model);
    edit["backup_dir"] = json!(f.root);
    f.s.execute("electrical.unifilar.update", &edit).unwrap();
    assert_eq!(f.s.execute("electrical.unifilar.read", &json!({"handle":h.hex()})).unwrap()["at"], json!([27.5, 275.]));
    assert_eq!(f.path(&link["handle"]).first().copied(), Some([27.5, 263.]));
    f.s.execute("u", &json!({})).unwrap();
    assert_eq!(f.s.execute("electrical.unifilar.read", &json!({"handle":h.hex()})).unwrap()["at"], json!([22.5, 275.]));
    assert_eq!(f.path(&link["handle"]).first().copied(), Some([22.5, 263.]));
}
#[test]
fn series_parallel_pe_roundtrip_follow_edits_and_undo_atomically() {
    let mut f = fixture();
    let series = json!({"version":1,"kind":"series","from":f.port(0,"out"),"to":f.port(1,"in"),"y":262.5});
    let a = f.link(series.clone());
    let path = f.path(&a["handle"]);
    assert_eq!(path.first().copied(), Some([27.5, 263.]));
    assert_eq!(path.last().copied(), Some([62.5, 250.]));
    assert!(path.windows(2).all(|p| p[0][0] == p[1][0] || p[0][1] == p[1][1]));
    let before = f.s.state().unwrap().doc.clone();
    assert!(f.s.execute("electrical.unifilar.link.insert", &json!({"model":f.model,"link":series,"backup_dir":f.root})).is_err());
    assert!(Arc::ptr_eq(&before, &f.s.state().unwrap().doc));
    f.s.execute("electrical.unifilar.link.delete", &json!({"handle":a["handle"],"backup_dir":f.root})).unwrap();
    let bus = f.link(json!({"version":1,"kind":"bus","from":f.port(0,"out"),"span":[45,145],"y":262.5}));
    let b = f.link(json!({"version":1,"kind":"branch","bus":bus["handle"],"to":f.port(1,"in"),"y":262.5}));
    let c = f.link(json!({"version":1,"kind":"branch","bus":bus["handle"],"to":f.port(2,"in"),"y":262.5}));
    let pe = f.link(json!({"version":1,"kind":"pe","from":f.port(3,"pe"),"y":161}));
    assert_eq!(f.path(&pe["handle"]).last().copied(), Some([132.5, 161.]));
    let marker =
        f.s.execute("electrical.unifilar.insert", &json!({"model":f.model,"id":"mark","at":[97.5,256],"fields":{},"backup_dir":f.root})).unwrap();
    let before = f.s.state().unwrap().doc.clone();
    let cycle = json!({"version":1,"kind":"series","from":f.port(1,"out"),"to":f.port(0,"in"),"y":275});
    assert!(f.s.execute("electrical.unifilar.link.insert", &json!({"model":f.model,"link":cycle,"backup_dir":f.root})).is_err());
    assert!(Arc::ptr_eq(&before, &f.s.state().unwrap().doc));
    let before = f.s.state().unwrap().doc.clone();
    let invalid = json!({"version":1,"kind":"pe","from":f.port(1,"out"),"y":161});
    assert!(f.s.execute("electrical.unifilar.link.insert", &json!({"model":f.model,"link":invalid,"backup_dir":f.root})).is_err());
    assert!(Arc::ptr_eq(&before, &f.s.state().unwrap().doc));
    let mut edit = f.s.execute("electrical.unifilar.read", &json!({"handle":f.devices[2]})).unwrap();
    edit["model"] = json!(f.model);
    edit["backup_dir"] = json!(f.root);
    edit["at"] = json!([97.5, 225]);
    f.s.execute("electrical.unifilar.update", &edit).unwrap();
    assert_eq!(f.path(&c["handle"]).last().copied(), Some([97.5, 225.]));
    let m = f.s.execute("electrical.unifilar.read", &json!({"handle":marker["handle"]})).unwrap();
    assert!((m["at"][1].as_f64().unwrap() - 243.0).abs() < 1e-6);
    f.s.execute("u", &json!({})).unwrap();
    assert_eq!(f.path(&c["handle"]).last().copied(), Some([97.5, 250.]));
    let bytes = cadcraft_io::write(f.s.doc().unwrap(), "roundtrip.dxf").unwrap();
    let mut reopened = Session::empty();
    reopened.open_drawing(cadcraft_io::read(&bytes, "roundtrip.dxf").unwrap(), "reopened", None);
    assert_eq!(reopened.execute("electrical.unifilar.link.read", &json!({"handle":b["handle"]})).unwrap()["link"]["bus"], bus["handle"]);
    let before = f.s.state().unwrap().doc.clone();
    assert!(f.s.execute("electrical.unifilar.link.delete", &json!({"handle":bus["handle"]})).is_err());
    assert!(Arc::ptr_eq(&before, &f.s.state().unwrap().doc));
    let r = f.s.execute("electrical.unifilar.link.delete", &json!({"handle":bus["handle"],"cascade":true,"backup_dir":f.root})).unwrap();
    assert_eq!(r["deleted"], 4);
    f.s.execute("u", &json!({})).unwrap();
    assert!(f.s.execute("electrical.unifilar.link.read", &json!({"handle":bus["handle"]})).is_ok());
    // Removing an output would orphan the bus: the whole device edit is rejected.
    let mut edit = f.s.execute("electrical.unifilar.read", &json!({"handle":f.devices[0]})).unwrap();
    edit["model"] = json!(f.model);
    edit["backup_dir"] = json!(f.root);
    edit["id"] = json!("spd");
    let before = f.s.state().unwrap().doc.clone();
    assert!(f.s.execute("electrical.unifilar.update", &edit).is_err());
    assert!(Arc::ptr_eq(&before, &f.s.state().unwrap().doc));
}

#[test]
fn guided_bus_requires_confirmation_and_cancel_is_clean() {
    use cadcraft_engine::{Input, geom::Vec2};
    let mut f = fixture();
    f.s.set_selection(vec![]);
    let before = f.s.state().unwrap().doc.clone();
    f.s.execute("electrical.unifilar.link.prepare", &json!({"model":f.model,"kind":"bus","backup_dir":f.root})).unwrap();
    f.s.start("electrical.unifilar.link.place").unwrap();
    f.s.input(Input::Point(Vec2::new(27.5, 263.))).unwrap();
    f.s.input(Input::Point(Vec2::new(45., 262.5))).unwrap();
    assert_eq!(f.s.unifilar_link.as_ref().unwrap().phase, 4);
    f.s.input(Input::Point(Vec2::new(145., 200.))).unwrap();
    assert_eq!(f.s.unifilar_link.as_ref().unwrap().phase, 3);
    assert!(Arc::ptr_eq(&before, &f.s.state().unwrap().doc));
    f.s.input(Input::Enter).unwrap();
    assert!(f.s.running.is_none());
    let h = f.s.doc().unwrap().model.iter().find(|e| e.common.xdata.iter().any(|t| t.code == 1001 && t.value == "CC_UNIFILARE_LINK")).unwrap().handle;
    let mut edit = f.s.execute("electrical.unifilar.link.read", &json!({"handle":h.hex()})).unwrap();
    edit["model"] = json!(f.model);
    edit["backup_dir"] = json!(f.root);
    edit["link"]["y"] = json!(275);
    f.s.execute("electrical.unifilar.link.insert", &edit).unwrap();
    assert_eq!(f.s.execute("electrical.unifilar.link.read", &json!({"handle":h.hex()})).unwrap()["link"]["y"].as_f64(), Some(275.0));
    let before = f.s.state().unwrap().doc.clone();
    f.s.execute("electrical.unifilar.link.prepare", &json!({"model":f.model,"kind":"branch","backup_dir":f.root})).unwrap();
    f.s.start("electrical.unifilar.link.place").unwrap();
    f.s.input(Input::Point(Vec2::new(100., 275.))).unwrap();
    f.s.input(Input::Point(Vec2::new(62.5, 250.))).unwrap();
    f.s.input(Input::Point(Vec2::new(0., 275.))).unwrap();
    f.s.cancel();
    assert!(Arc::ptr_eq(&before, &f.s.state().unwrap().doc));
    f.s.execute("u", &json!({})).unwrap();
    assert_eq!(f.s.execute("electrical.unifilar.link.read", &json!({"handle":h.hex()})).unwrap()["link"]["y"], 262.5);
    let bad = f.root.join("not-directory");
    std::fs::write(&bad, b"x").unwrap();
    let before = f.s.state().unwrap().doc.clone();
    let link = json!({"version":1,"kind":"branch","bus":h.hex(),"to":f.port(2,"in"),"y":262.5});
    assert!(f.s.execute("electrical.unifilar.link.insert", &json!({"model":f.model,"link":link,"backup_dir":bad})).is_err());
    assert!(Arc::ptr_eq(&before, &f.s.state().unwrap().doc));
}

#[test]
fn series_vertical_pass_defaults_to_midpoint_and_can_be_overridden() {
    use cadcraft_engine::{Input, geom::Vec2};
    let mut f = fixture();
    let before = f.s.state().unwrap().doc.clone();
    f.s.execute("electrical.unifilar.link.prepare", &json!({"model":f.model,"kind":"series","backup_dir":f.root})).unwrap();
    f.s.start("electrical.unifilar.link.place").unwrap();
    f.s.input(Input::Point(Vec2::new(62.5, 238.))).unwrap();
    f.s.input(Input::Point(Vec2::new(97.5, 250.))).unwrap();
    f.s.input(Input::Point(Vec2::new(10., 200.))).unwrap();
    assert_eq!(f.s.unifilar_link.as_ref().unwrap().phase, 5);
    assert_eq!(f.s.unifilar_link.as_ref().unwrap().proposed_vertical_x(), Some(80.));
    assert!(Arc::ptr_eq(&before, &f.s.state().unwrap().doc));
    // A fourth click cannot force a vertical line through the destination body.
    let _ = f.s.input(Input::Point(Vec2::new(97.5, 200.)));
    assert_eq!(f.s.unifilar_link.as_ref().unwrap().phase, 5);
    assert!(f.s.unifilar_link.as_ref().unwrap().error.is_some());
    f.s.input(Input::Enter).unwrap();
    assert_eq!(f.s.unifilar_link.as_ref().unwrap().phase, 3);
    assert!(Arc::ptr_eq(&before, &f.s.state().unwrap().doc));
    f.s.input(Input::Enter).unwrap();
    let h = f.s.doc().unwrap().model.iter().find(|e| e.common.xdata.iter().any(|t| t.code == 1001 && t.value == "CC_UNIFILARE_LINK")).unwrap().handle;
    let path = f.path(&json!(h.hex()));
    assert!(path.windows(2).any(|p| p[0] == [80., 200.] && p[1] == [80., 252.]));
    f.s.execute("electrical.unifilar.link.prepare", &json!({"model":f.model,"handle":h.hex(),"backup_dir":f.root})).unwrap();
    f.s.start("electrical.unifilar.link.place").unwrap();
    f.s.input(Input::Keyword("Verticale".into())).unwrap();
    f.s.input(Input::Point(Vec2::new(75., 100.))).unwrap();
    assert_eq!(f.s.unifilar_link.as_ref().unwrap().phase, 3);
    f.s.input(Input::Enter).unwrap();
    assert!(f.path(&json!(h.hex())).windows(2).any(|p| p[0] == [75., 200.] && p[1] == [75., 252.]));
    let bytes = cadcraft_io::write(f.s.doc().unwrap(), "roundtrip.dxf").unwrap();
    let mut reopened = Session::empty();
    reopened.open_drawing(cadcraft_io::read(&bytes, "roundtrip.dxf").unwrap(), "reopened", None);
    assert_eq!(reopened.execute("electrical.unifilar.link.read", &json!({"handle":h.hex()})).unwrap()["link"]["vertical_x"], 75.);
    f.s.execute("u", &json!({})).unwrap();
    assert!(f.s.execute("electrical.unifilar.link.read", &json!({"handle":h.hex()})).unwrap()["link"]["vertical_x"].is_null());
}

#[test]
fn local_device_data_shares_a_column_and_line_data_owns_only_its_cells() {
    let mut f = fixture();
    f.s.doc_mut().unwrap().ensure_layer("UNI_GUIDE");
    let guide =
        f.s.doc_mut()
            .unwrap()
            .add(
                &cadcraft_engine::doc::Space::Model,
                cadcraft_engine::doc::Common { layer: "UNI_GUIDE".into(), ..Default::default() },
                EntityKind::Line(Line { a: Vec3::new(32., 171., 0.), b: Vec3::new(32., 282., 0.) }),
            )
            .unwrap();
    assert!(f.s.doc().unwrap().entity(guide).is_some());
    let cat_path = f.root.join("catalogo.json");
    let mut cat: Value = serde_json::from_slice(&std::fs::read(&cat_path).unwrap()).unwrap();
    let mut command = cat["symbols"][0].clone();
    command["id"] = json!("command");
    command["category"] = json!("Comando");
    command["data_position"] = json!("right");
    let mut terminal = command.clone();
    terminal["id"] = json!("terminal");
    terminal["category"] = json!("Utilizzatori");
    terminal["data_position"] = json!("below");
    terminal["ports"].as_array_mut().unwrap().retain(|p| p["id"] != "out");
    let mut marker = cat["symbols"][2].clone();
    marker["fields"] = json!(["cavo", "formazione", "lunghezza"]);
    cat["symbols"].as_array_mut().unwrap().extend([command, terminal]);
    cat["symbols"][2] = marker;
    std::fs::write(&cat_path, cat.to_string()).unwrap();
    let mut model: Value = serde_json::from_slice(&std::fs::read(&f.model).unwrap()).unwrap();
    for (i, key) in ["cavo", "formazione", "lunghezza"].iter().enumerate() {
        model["table"]["rows"].as_array_mut().unwrap().push(json!({"id":key,"label":key}));
        for col in model["columns"].as_array_mut().unwrap() {
            let x = col["bounds"][0].as_f64().unwrap();
            let y = 140. - i as f64 * 6.;
            col["cells"][key] = json!({"bounds":[x,y-6.,x+35.,y],"text_center":[x+17.5,y-3.],"padding_mm":1.5,"max_lines":1});
        }
    }
    std::fs::write(&f.model, model.to_string()).unwrap();
    let mut added = Vec::new();
    for (id, y, sigla) in [("command", 245., "KM1"), ("terminal", 215., "M1"), ("terminal", 185., "M2")] {
        let r =
            f.s.execute("electrical.unifilar.insert", &json!({"model":f.model,"id":id,"at":[27.5,y],"fields":{"sigla":sigla},"backup_dir":f.root}))
                .unwrap();
        let h = Handle::parse_hex(r["handle"].as_str().unwrap()).unwrap();
        let EntityKind::Insert(i) = &f.s.doc().unwrap().entity(h).unwrap().kind else { panic!() };
        let text: Vec<_> =
            f.s.doc()
                .unwrap()
                .block(&i.block)
                .unwrap()
                .entities
                .iter()
                .filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.insert.y + i.insert.y) } else { None })
                .collect();
        assert!(!text.is_empty() && text.iter().all(|y| *y > 164.));
        added.push(r["handle"].clone());
    }
    let before = f.s.state().unwrap().doc.clone();
    assert!(
        f.s.execute(
            "electrical.unifilar.insert",
            &json!({"model":f.model,"id":"terminal","at":[27.5,185.],"fields":{"sigla":"duplicate"},"backup_dir":f.root})
        )
        .is_err()
    );
    assert!(Arc::ptr_eq(&before, &f.s.state().unwrap().doc));
    f.s.execute("line", &json!({"points":[[27.5,210.],[27.5,225.]]})).unwrap();
    let p = json!({"model":f.model,"id":"mark","at":[27.5,220.],"fields":{"cavo":"FG16","formazione":"3G2.5","lunghezza":"20"},"backup_dir":f.root});
    let r = f.s.execute("electrical.unifilar.insert", &p).unwrap();
    assert_eq!(r["column"], 1);
    let before = f.s.state().unwrap().doc.clone();
    assert!(f.s.execute("electrical.unifilar.insert", &p).is_err());
    assert!(Arc::ptr_eq(&before, &f.s.state().unwrap().doc));
    let bytes = cadcraft_io::write(f.s.doc().unwrap(), "roundtrip.dxf").unwrap();
    let mut reopened = Session::empty();
    reopened.open_drawing(cadcraft_io::read(&bytes, "roundtrip.dxf").unwrap(), "test", None);
    let m = reopened.execute("electrical.unifilar.read", &json!({"handle":added[0]})).unwrap();
    assert_eq!(m["data_position"], "right");
    assert_eq!(m["fields"]["sigla"], "KM1");
    // An attached marker follows the bar while cable data stays in its table column.
    let bus = f.link(json!({"version":1,"kind":"bus","from":f.port(0,"out"),"span":[45,145],"y":275}));
    let marker = f
        .s
        .execute("electrical.unifilar.insert", &json!({"model":f.model,"id":"mark","at":[62.5,275.],"fields":{"cavo":"FG16"},"backup_dir":f.root}))
        .unwrap();
    let world_texts = |s: &Session, h: &Value| {
        let h = Handle::parse_hex(h.as_str().unwrap()).unwrap();
        let EntityKind::Insert(i) = &s.doc().unwrap().entity(h).unwrap().kind else { panic!() };
        s.doc()
            .unwrap()
            .block(&i.block)
            .unwrap()
            .entities
            .iter()
            .filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some((t.value.clone(), t.insert.xy() + i.insert.xy(), t.rotation)) } else { None })
            .collect::<Vec<_>>()
    };
    let texts = world_texts(&f.s, &marker["handle"]);
    let mut edit = f.s.execute("electrical.unifilar.link.read", &json!({"handle":bus["handle"]})).unwrap();
    edit["model"] = json!(f.model);
    edit["backup_dir"] = json!(f.root);
    edit["link"]["y"] = json!(262.5);
    f.s.execute("electrical.unifilar.link.insert", &edit).unwrap();
    assert_eq!(world_texts(&f.s, &marker["handle"]), texts);
    assert_eq!(f.s.execute("electrical.unifilar.read", &json!({"handle":marker["handle"]})).unwrap()["at"][1], 262.5);
    // Explicitly editing an older field policy archives obsolete values instead of losing them.
    cat["symbols"][0]["fields"] = json!(["sigla", "cavo"]);
    std::fs::write(&cat_path, cat.to_string()).unwrap();
    let old = f
        .s
        .execute("electrical.unifilar.insert", &json!({"model":f.model,"id":"dev","at":[62.5,195.],"fields":{"cavo":"legacy"},"backup_dir":f.root}));
    // Column two already has cable data from the marker: select an unclaimed row on column four instead.
    assert!(old.is_err());
    let old = f
        .s
        .execute("electrical.unifilar.insert", &json!({"model":f.model,"id":"dev","at":[132.5,195.],"fields":{"cavo":"legacy"},"backup_dir":f.root}))
        .unwrap();
    cat["symbols"][0]["fields"] = json!(["sigla"]);
    std::fs::write(&cat_path, cat.to_string()).unwrap();
    let mut edit = f.s.execute("electrical.unifilar.read", &json!({"handle":old["handle"]})).unwrap();
    edit["model"] = json!(f.model);
    edit["backup_dir"] = json!(f.root);
    f.s.execute("electrical.unifilar.update", &edit).unwrap();
    let meta = f.s.execute("electrical.unifilar.read", &json!({"handle":old["handle"]})).unwrap();
    assert!(meta["fields"].get("cavo").is_none());
    assert_eq!(meta["legacy_fields"]["cavo"], "legacy");
}

#[test]
fn compact_command_fields_validate_render_and_roundtrip_without_table_rows() {
    let mut f = fixture();
    let path = f.root.join("catalogo.json");
    let mut catalog: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    catalog["symbols"].as_array_mut().unwrap().push(json!({
        "id":"command-compact","name":"Contattore","block":"DEVICE","category":"Comando","kind":"device",
        "data_position":"right","fields":["sigla","tipo","classe","bobina","bobina_alimentazione","poli","in_apparecchio"],
        "ports":[{"id":"in","role":"power","position":[0,0,0]},{"id":"out","role":"power","position":[0,-12,0]}]
    }));
    std::fs::write(&path, catalog.to_string()).unwrap();
    let mut p = json!({"model":f.model,"id":"command-compact","at":[27.5,230.],
        "fields":{"sigla":"KM1","tipo":"CT","classe":"AC-3","bobina":"24","bobina_alimentazione":"DC","poli":"3","in_apparecchio":"16"},"backup_dir":f.root});
    let before = f.s.state().unwrap().doc.clone();
    for (key, value) in [("classe", "invalid"), ("bobina_alimentazione", "both"), ("bobina_alimentazione", ""), ("poli", "0"), ("poli", "3P")] {
        let saved = p["fields"][key].clone();
        p["fields"][key] = json!(value);
        assert!(f.s.execute("electrical.unifilar.insert", &p).is_err());
        assert!(Arc::ptr_eq(&before, &f.s.state().unwrap().doc));
        p["fields"][key] = saved;
    }
    let inserted = f.s.execute("electrical.unifilar.insert", &p).unwrap();
    let h = Handle::parse_hex(inserted["handle"].as_str().unwrap()).unwrap();
    let EntityKind::Insert(i) = &f.s.doc().unwrap().entity(h).unwrap().kind else { panic!() };
    assert_eq!(i.insert.x, 27.5);
    let b = f.s.doc().unwrap().block(&i.block).unwrap();
    let texts: Vec<_> = b.entities.iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.value.as_str()) } else { None }).collect();
    for expected in ["KM1", "CT", "AC-3", "24 VDC", "3P", "In [A] 16"] {
        assert!(texts.contains(&expected), "{texts:?}");
    }
    assert_eq!(b.entities.iter().filter(|e| e.common.layer == "UNI_TESTI" && matches!(e.kind, EntityKind::Line(_))).count(), 6);
    assert_eq!(texts.len(), 6);
    let out = json!({"handle":inserted["handle"],"port":"in"});
    f.link(json!({"version":1,"kind":"series","from":f.port(0,"out"),"to":out,"y":250}));
    let bytes = cadcraft_io::write(f.s.doc().unwrap(), "roundtrip.dxf").unwrap();
    let mut reopened = Session::empty();
    reopened.open_drawing(cadcraft_io::read(&bytes, "roundtrip.dxf").unwrap(), "test", None);
    let read = reopened.execute("electrical.unifilar.read", &json!({"handle":inserted["handle"]})).unwrap();
    assert_eq!(read["fields"], p["fields"]);
    let mut edit = f.s.execute("electrical.unifilar.read", &json!({"handle":inserted["handle"]})).unwrap();
    edit["model"] = json!(f.model);
    edit["backup_dir"] = json!(f.root);
    edit["fields"]["bobina_alimentazione"] = json!("AC");
    edit["fields"]["bobina"] = json!("230");
    f.s.execute("electrical.unifilar.update", &edit).unwrap();
    f.s.execute("undo", &json!({})).unwrap();
    assert_eq!(f.s.execute("electrical.unifilar.read", &json!({"handle":inserted["handle"]})).unwrap()["fields"]["bobina"], "24");
}

fn extra_device(f: &mut Fixture, x: f64, y: f64) -> Value {
    f.s.execute("electrical.unifilar.insert", &json!({"model":f.model,"id":"dev","at":[x,y],"fields":{},"backup_dir":f.root})).unwrap()["handle"]
        .clone()
}
#[test]
fn vertical_connections_skip_route_questions_and_taps_follow_nested_networks() {
    use cadcraft_engine::{Input, geom::Vec2};
    let mut f = fixture();
    let command = extra_device(&mut f, 27.5, 225.);
    let lamp1 = extra_device(&mut f, 27.5, 185.);
    let lamp2 = extra_device(&mut f, 62.5, 185.);
    let lamp3 = extra_device(&mut f, 97.5, 185.);
    f.s.set_selection(vec![]);
    f.s.execute("electrical.unifilar.link.prepare", &json!({"model":f.model,"kind":"series","backup_dir":f.root})).unwrap();
    f.s.start("electrical.unifilar.link.place").unwrap();
    f.s.input(Input::Point(Vec2::new(27.5, 263.))).unwrap();
    f.s.input(Input::Point(Vec2::new(27.5, 225.))).unwrap();
    assert_eq!(f.s.unifilar_link.as_ref().unwrap().phase, 3);
    assert!(f.s.unifilar_link.as_ref().unwrap().simple_vertical());
    f.s.input(Input::Enter).unwrap();
    let trunk = f
        .link(json!({"version":2,"kind":"series","from":{"handle":command,"port":"out"},"to":{"handle":lamp1,"port":"in"},"automatic":true,"y":205}));
    assert_eq!(f.path(&trunk["handle"]), vec![[27.5, 213.], [27.5, 185.]]);
    // The same primary Connect command accepts a wire origin and needs only the target click.
    f.s.set_selection(vec![]);
    f.s.execute("electrical.unifilar.link.prepare", &json!({"model":f.model,"kind":"series","backup_dir":f.root})).unwrap();
    f.s.start("electrical.unifilar.link.place").unwrap();
    f.s.input(Input::Point(Vec2::new(27.5, 205.))).unwrap();
    assert_eq!(f.s.unifilar_link.as_ref().unwrap().link.kind, "branch");
    f.s.input(Input::Point(Vec2::new(62.5, 185.))).unwrap();
    assert_eq!(f.s.unifilar_link.as_ref().unwrap().phase, 3);
    let tap = f.s.unifilar_link.as_ref().unwrap().link.tap.clone().unwrap();
    assert_eq!(tap.wire, trunk["handle"].as_str().unwrap());
    f.s.input(Input::Enter).unwrap();
    let graph = f.s.execute("electrical.unifilar.network.read", &json!({"model":f.model})).unwrap();
    let branch = graph["wires"].as_array().unwrap().iter().find(|w| !w["junction"].is_null()).unwrap()["handle"].clone();
    assert_eq!(f.path(&branch), vec![[27.5, 205.], [62.5, 205.], [62.5, 185.]]);
    let nested=f.link(json!({"version":2,"kind":"branch","tap":{"wire":branch,"part":"route","fraction":17.5/55.},"to":{"handle":lamp3,"port":"in"},"automatic":true,"y":205}));
    assert_eq!(f.s.execute("electrical.unifilar.link.read", &json!({"handle":trunk["handle"]})).unwrap()["dependents"], 2);
    let before_erase = f.s.state().unwrap().doc.clone();
    assert!(f.s.execute("erase", &json!({"handles":[trunk["handle"]]})).is_err());
    assert!(Arc::ptr_eq(&before_erase, &f.s.state().unwrap().doc));

    let bytes = cadcraft_io::write(f.s.doc().unwrap(), "network.dxf").unwrap();
    let mut reopened = Session::empty();
    reopened.open_drawing(cadcraft_io::read(&bytes, "network.dxf").unwrap(), "network", None);
    let graph = reopened.execute("electrical.unifilar.network.read", &json!({"model":f.model})).unwrap();
    assert_eq!(graph["junctions"].as_array().unwrap().len(), 2);
    assert_eq!(graph["wires"].as_array().unwrap().iter().filter(|w| w["destination"]["handle"] == lamp2).count(), 1);
    // Device relocation recalculates parents before their junction children.
    let old_path = f.path(&branch);
    let mut edit = f.s.execute("electrical.unifilar.read", &json!({"handle":command})).unwrap();
    edit["model"] = json!(f.model);
    edit["backup_dir"] = json!(f.root);
    edit["at"] = json!([97.5, 225.]);
    f.s.execute("electrical.unifilar.update", &edit).unwrap();
    assert_ne!(f.path(&branch).first(), old_path.first());
    assert_eq!(f.path(&branch).last(), Some(&[62.5, 185.]));
    f.s.execute("u", &json!({})).unwrap();
    assert_eq!(f.path(&branch), old_path);
    // Parent deletion requires explicit cascade and preserves unrelated conductors.
    let before = f.s.state().unwrap().doc.clone();
    assert!(f.s.execute("electrical.unifilar.link.delete", &json!({"handle":trunk["handle"]})).is_err());
    assert!(Arc::ptr_eq(&before, &f.s.state().unwrap().doc));
    assert_eq!(
        f.s.execute("electrical.unifilar.link.delete", &json!({"handle":trunk["handle"],"cascade":true,"backup_dir":f.root})).unwrap()["deleted"],
        3
    );
    assert!(f.s.execute("electrical.unifilar.link.read", &json!({"handle":nested["handle"]})).is_err());
    f.s.execute("u", &json!({})).unwrap();
    assert!(f.s.execute("electrical.unifilar.link.read", &json!({"handle":nested["handle"]})).is_ok());
}
#[test]
fn fixed_waypoints_and_hostile_junctions_are_atomic() {
    let mut f = fixture();
    let lamp = extra_device(&mut f, 62.5, 185.);
    let command = extra_device(&mut f, 27.5, 225.);
    let link=f.link(json!({"version":2,"kind":"series","from":{"handle":command,"port":"out"},"to":{"handle":lamp,"port":"in"},"waypoints":[[45.,205.],[45.,195.]],"y":205}));
    let path = f.path(&link["handle"]);
    assert!(path.windows(2).any(|p| p[0][0] == 45. && p[1][0] == 45. && p[0][1].max(p[1][1]) >= 205. && p[0][1].min(p[1][1]) <= 195.));
    let before = f.s.state().unwrap().doc.clone();
    for tap in [
        json!({"wire":link["handle"],"part":"route","fraction":2.}),
        json!({"wire":"FFFF","part":"route","fraction":0.5}),
        json!({"wire":link["handle"],"part":"invalid","fraction":0.5}),
    ] {
        let result = f.s.execute(
            "electrical.unifilar.link.insert",
            &json!({"model":f.model,"link":{"version":2,"kind":"branch","tap":tap,"to":f.port(0,"in"),"y":260},"backup_dir":f.root}),
        );
        assert!(result.is_err());
        assert!(Arc::ptr_eq(&before, &f.s.state().unwrap().doc));
    }
    let mut edit = f.s.execute("electrical.unifilar.link.read", &json!({"handle":link["handle"]})).unwrap();
    edit["model"] = json!(f.model);
    edit["backup_dir"] = json!(f.root);
    edit["link"]["kind"] = json!("branch");
    edit["link"].as_object_mut().unwrap().remove("from");
    edit["link"]["tap"] = json!({"wire":link["handle"],"part":"route","fraction":0.5});
    assert!(f.s.execute("electrical.unifilar.link.insert", &edit).is_err());
    assert!(Arc::ptr_eq(&before, &f.s.state().unwrap().doc));
}

#[test]
fn moving_one_segment_preserves_endpoints_and_fixed_anchors() {
    use cadcraft_engine::{Input, geom::Vec2};
    let mut f = fixture();
    let command = extra_device(&mut f, 27.5, 225.);
    let lamp = extra_device(&mut f, 62.5, 185.);
    let link=f.link(json!({"version":2,"kind":"series","from":{"handle":command,"port":"out"},"to":{"handle":lamp,"port":"in"},"waypoints":[[45.,205.],[45.,195.]],"y":205}));
    let original = f.path(&link["handle"]);
    let mut edit = f.s.execute("electrical.unifilar.link.read", &json!({"handle":link["handle"]})).unwrap();
    edit["model"] = json!(f.model);
    edit["backup_dir"] = json!(f.root);
    f.s.execute("electrical.unifilar.link.prepare", &edit).unwrap();
    f.s.start("electrical.unifilar.link.place").unwrap();
    f.s.input(Input::Keyword("Tratto".into())).unwrap();
    f.s.input(Input::Point(Vec2::new(45., 200.))).unwrap();
    assert_eq!(f.s.unifilar_link.as_ref().unwrap().phase, 8);
    f.s.input(Input::Point(Vec2::new(50., 200.))).unwrap();
    assert_eq!(f.s.unifilar_link.as_ref().unwrap().phase, 3);
    f.s.input(Input::Enter).unwrap();
    let path = f.path(&link["handle"]);
    assert_eq!(path.first(), original.first());
    assert_eq!(path.last(), original.last());
    assert!(path.windows(2).any(|p| p[0][0] == 50. && p[1][0] == 50. && p[0][1].max(p[1][1]) >= 205. && p[0][1].min(p[1][1]) <= 195.), "{path:?}");
    f.s.execute("u", &json!({})).unwrap();
    assert_eq!(f.path(&link["handle"]), original);
    // Multiple loads may also share a source terminal directly, without requiring a bus.
    let lamp2 = extra_device(&mut f, 97.5, 185.);
    let other = f
        .link(json!({"version":2,"kind":"series","from":{"handle":command,"port":"out"},"to":{"handle":lamp2,"port":"in"},"automatic":true,"y":200}));
    assert!(!f.path(&other["handle"]).is_empty());
}

#[test]
fn geometric_crossings_do_not_create_electrical_junctions() {
    let mut f = fixture();
    let first = f.link(json!({"version":1,"kind":"series","from":f.port(0,"out"),"to":f.port(2,"in"),"y":260}));
    let second = f.link(json!({"version":1,"kind":"series","from":f.port(1,"out"),"to":f.port(0,"in"),"y":255}));
    let a = f.path(&first["handle"]);
    let b = f.path(&second["handle"]);
    let mut intersections = 0;
    for p in a.windows(2) {
        for q in b.windows(2) {
            let (v, h) = if (p[0][0] - p[1][0]).abs() < 1e-7 { (p, q) } else { (q, p) };
            if (v[0][0] - v[1][0]).abs() < 1e-7
                && (h[0][1] - h[1][1]).abs() < 1e-7
                && v[0][0] > h[0][0].min(h[1][0])
                && v[0][0] < h[0][0].max(h[1][0])
                && h[0][1] > v[0][1].min(v[1][1])
                && h[0][1] < v[0][1].max(v[1][1])
            {
                intersections += 1;
            }
        }
    }
    assert!(intersections > 0, "{a:?} {b:?}");
    let graph = f.s.execute("electrical.unifilar.network.read", &json!({"model":f.model})).unwrap();
    assert!(graph["junctions"].as_array().unwrap().is_empty());
    for h in [&first["handle"], &second["handle"]] {
        let e = f.s.doc().unwrap().entity(Handle::parse_hex(h.as_str().unwrap()).unwrap()).unwrap();
        let EntityKind::Insert(i) = &e.kind else { panic!() };
        assert!(f.s.doc().unwrap().block(&i.block).unwrap().entities.iter().all(|e| !matches!(e.kind, EntityKind::Solid(_))));
    }
}

#[test]
fn a_same_axis_tap_leaves_the_trunk_sideways_without_retracing_it() {
    let mut f = fixture();
    let command = extra_device(&mut f, 27.5, 225.);
    let lamp = extra_device(&mut f, 27.5, 185.);
    let trunk = f.link(json!({"version":2,"kind":"series","from":f.port(0,"out"),"to":{"handle":command,"port":"in"},"automatic":true,"y":245}));
    let branch=f.link(json!({"version":2,"kind":"branch","tap":{"wire":trunk["handle"],"part":"route","fraction":18./38.},"to":{"handle":lamp,"port":"in"},"automatic":true,"y":245}));
    let path = f.path(&branch["handle"]);
    assert_eq!(path.first(), Some(&[27.5, 245.]));
    assert_eq!(path.last(), Some(&[27.5, 185.]));
    for p in path.windows(2) {
        if p[0][0] == 27.5 && p[1][0] == 27.5 {
            assert!(p[0][1].max(p[1][1]).min(263.) - p[0][1].min(p[1][1]).max(225.) <= 1e-7, "{path:?}");
        }
    }
}

fn wire(f: &Fixture, a: Value, b: Value, points: Value) -> Value {
    let _ = f;
    json!({"version":3,"kind":"wire","from":a,"to":b,"y":200.,"waypoints":points,"route_mode":"diagonal"})
}
fn dot_count(f: &Fixture) -> usize {
    f.s.doc()
        .unwrap()
        .model
        .iter()
        .filter_map(|e| match &e.kind {
            EntityKind::Insert(i) if i.block.starts_with("CC_UNIFILARE_LINK_") => Some(i),
            _ => None,
        })
        .map(|i| f.s.doc().unwrap().block(&i.block).unwrap().entities.iter().filter(|e| matches!(e.kind, EntityKind::Solid(_))).count() / 8)
        .sum()
}
#[test]
fn undirected_wires_accept_all_port_pairs_and_shared_terminal() {
    for (a, b) in [("out", "in"), ("in", "out"), ("out", "out"), ("in", "in")] {
        let mut f = fixture();
        let ay = if a == "in" { 278. } else { 260. };
        let by = if b == "in" { 253. } else { 235. };
        let l = wire(&f, f.port(0, a), f.port(1, b), json!([[27.5, ay], [45., ay], [45., by], [62.5, by]]));
        let added = f.link(l);
        assert_eq!(f.path(&added["handle"]).first().unwrap()[0], 27.5);
        assert_eq!(dot_count(&f), 0);
        let l = wire(&f, f.port(1, b), f.port(2, "in"), json!([[62.5, by], [80., by], [80., 253.], [97.5, 253.]]));
        let second = f.link(l);
        assert_eq!(dot_count(&f), 1, "Two wires on one terminal need one junction dot");
        f.s.execute("electrical.unifilar.link.delete", &json!({"handle":second["handle"],"backup_dir":f.root})).unwrap();
        assert_eq!(dot_count(&f), 0, "A lone terminal wire has no dot");
        f.s.execute("u", &json!({})).unwrap();
        assert_eq!(dot_count(&f), 1, "Undo restores the shared terminal dot");
        f.s.execute("electrical.unifilar.network.read", &json!({"model":f.model})).unwrap();
    }
}
#[test]
fn trace_vertical_two_clicks_commits_once_and_undo_restores() {
    use cadcraft_engine::{Input, geom::Vec2};
    let mut f = fixture();
    f.s.execute("electrical.unifilar.insert", &json!({"model":f.model,"id":"dev","at":[62.5,210.],"fields":{},"backup_dir":f.root})).unwrap();
    let before = f.s.state().unwrap().doc.clone();
    f.s.execute("electrical.unifilar.link.prepare", &json!({"model":f.model,"trace":true,"backup_dir":f.root})).unwrap();
    f.s.start("electrical.unifilar.link.place").unwrap();
    f.s.input(Input::Point(Vec2::new(62.5, 238.))).unwrap();
    assert_eq!(f.s.unifilar_link.as_ref().unwrap().phase, 9);
    assert!(Arc::ptr_eq(&before, &f.s.state().unwrap().doc));
    f.s.input(Input::Point(Vec2::new(62.5, 210.))).unwrap();
    assert!(f.s.running.is_none());
    assert_eq!(dot_count(&f), 0);
    let g = f.s.execute("electrical.unifilar.network.read", &json!({"model":f.model})).unwrap();
    let path = f.path(&g["wires"][0]["handle"]);
    assert_eq!(path, vec![[62.5, 238.], [62.5, 210.]]);
    f.s.execute("u", &json!({})).unwrap();
    assert_eq!(f.s.doc().unwrap().model.len(), before.model.len());
}
#[test]
fn taps_at_either_end_have_automatic_dots_removed_on_erase() {
    let mut f = fixture();
    let base = f.link(wire(&f, f.port(0, "out"), f.port(1, "in"), json!([[27.5, 260.], [45., 260.], [45., 253.], [62.5, 253.]])));
    let l = json!({"version":3,"kind":"wire","from":f.port(2,"in"),"end_tap":{"wire":base["handle"],"part":"route","fraction":0.5},"y":270.,"waypoints":[[97.5,280.],[80.,280.],[80.,256.5]],"route_mode":"diagonal"});
    let added = f.link(l);
    assert_eq!(dot_count(&f), 1);
    let graph = f.s.execute("electrical.unifilar.network.read", &json!({"model":f.model})).unwrap();
    assert!(graph["wires"].as_array().unwrap().iter().any(|w| w["end_junction"].is_object()));
    f.s.execute("electrical.unifilar.link.delete", &json!({"handle":added["handle"],"backup_dir":f.root})).unwrap();
    assert_eq!(dot_count(&f), 0);
    f.s.execute("u", &json!({})).unwrap();
    assert_eq!(dot_count(&f), 1);
}
#[test]
fn trace_diagonal_pe_roundtrip_and_click_backtracking() {
    use cadcraft_engine::{Input, geom::Vec2};
    let mut f = fixture();
    f.s.execute("electrical.unifilar.link.prepare", &json!({"model":f.model,"trace":true,"backup_dir":f.root})).unwrap();
    f.s.start("electrical.unifilar.link.place").unwrap();
    f.s.input(Input::Point(Vec2::new(132.5, 225.5))).unwrap();
    f.s.input(Input::Keyword("Diagonale".into())).unwrap();
    f.s.input(Input::Point(Vec2::new(150., 200.))).unwrap();
    assert!(f.s.unifilar_link.as_ref().unwrap().link.waypoints.len() >= 2);
    f.s.input(Input::Keyword("Indietro".into())).unwrap();
    assert!(f.s.unifilar_link.as_ref().unwrap().link.waypoints.is_empty());
    f.s.input(Input::Point(Vec2::new(150., 161.))).unwrap();
    assert!(f.s.running.is_none());
    assert_eq!(dot_count(&f), 1);
    let g = f.s.execute("electrical.unifilar.network.read", &json!({"model":f.model})).unwrap();
    let h = g["wires"][0]["handle"].clone();
    let path = f.path(&h);
    assert!(path.windows(2).any(|p| (p[0][0] - p[1][0]).abs() > 1e-6 && ((p[0][0] - p[1][0]).abs() - (p[0][1] - p[1][1]).abs()).abs() < 1e-6));
    let bytes = cadcraft_io::write(f.s.doc().unwrap(), "wire.dxf").unwrap();
    let mut reopened = Session::empty();
    reopened.open_drawing(cadcraft_io::read(&bytes, "wire.dxf").unwrap(), "reopened", None);
    assert_eq!(reopened.execute("electrical.unifilar.link.read", &json!({"handle":h})).unwrap()["link"]["version"], 3);
}
#[test]
fn wire_to_wire_join_is_symmetric_and_dependency_cycles_are_atomic() {
    let mut f = fixture();
    let a = f.link(wire(&f, f.port(0, "out"), f.port(1, "in"), json!([[27.5, 260.], [45., 260.], [45., 253.], [62.5, 253.]])));
    let b = f.link(wire(&f, f.port(2, "out"), f.port(3, "in"), json!([[97.5, 235.], [115., 235.], [115., 242.], [132.5, 242.]])));
    let c=f.link(json!({"version":3,"kind":"wire","tap":{"wire":a["handle"],"part":"route","fraction":0.5},"end_tap":{"wire":b["handle"],"part":"route","fraction":0.5},"y":220.,"waypoints":[[80.,256.5],[80.,270.],[125.,270.],[125.,239.25]],"route_mode":"diagonal"}));
    assert_eq!(dot_count(&f), 2);
    let g = f.s.execute("electrical.unifilar.network.read", &json!({"model":f.model})).unwrap();
    let nets: Vec<_> = g["wires"].as_array().unwrap().iter().map(|v| v["net"].clone()).collect();
    assert!(nets.iter().all(|n| n == &nets[0]));
    let before = f.s.state().unwrap().doc.clone();
    let mut update = f.s.execute("electrical.unifilar.link.read", &json!({"handle":a["handle"]})).unwrap();
    update["model"] = json!(f.model);
    update["link"]["from"] = Value::Null;
    update["link"]["tap"] = json!({"wire":c["handle"],"part":"route","fraction":0.5});
    assert!(f.s.execute("electrical.unifilar.link.insert", &update).is_err());
    assert!(Arc::ptr_eq(&before, &f.s.state().unwrap().doc));
}

#[test]
fn inserting_device_on_wire_splits_and_remaps_downstream_taps_atomically() {
    let mut f = fixture();
    let lower =
        f.s.execute("electrical.unifilar.insert", &json!({"model":f.model,"id":"dev","at":[62.5,185.],"fields":{},"backup_dir":f.root})).unwrap();
    let base = f.link(wire(&f, f.port(1, "out"), json!({"handle":lower["handle"],"port":"in"}), json!([])));
    let branch=f.link(json!({"version":3,"kind":"wire","tap":{"wire":base["handle"],"part":"route","fraction":0.8},"to":f.port(2,"out"),"y":200.,"waypoints":[[80.,195.6],[80.,230.],[97.5,230.]],"route_mode":"orthogonal"}));
    let before = f.s.state().unwrap().doc.clone();
    let device =
        f.s.execute("electrical.unifilar.insert", &json!({"model":f.model,"id":"dev","at":[62.5,220.],"fields":{},"backup_dir":f.root})).unwrap();
    let graph = f.s.execute("electrical.unifilar.network.read", &json!({"model":f.model})).unwrap();
    assert_eq!(graph["wires"].as_array().unwrap().len(), 3);
    let left = f.s.execute("electrical.unifilar.link.read", &json!({"handle":base["handle"]})).unwrap();
    assert_eq!(left["link"]["to"]["handle"], device["handle"]);
    let moved = f.s.execute("electrical.unifilar.link.read", &json!({"handle":branch["handle"]})).unwrap();
    assert_ne!(moved["link"]["tap"]["wire"], base["handle"]);
    assert_eq!(f.path(&branch["handle"])[0], [62.5, 195.6]);
    f.s.execute("u", &json!({})).unwrap();
    assert_eq!(f.s.doc().unwrap().model.len(), before.model.len());
}
#[test]
fn moved_trace_retains_port_attachments_and_octilinear_constraints() {
    let mut f = fixture();
    let l = f.link(wire(&f, f.port(0, "out"), f.port(1, "in"), json!([[27.5, 260.], [45., 260.], [45., 253.], [62.5, 253.]])));
    let mut edit = f.s.execute("electrical.unifilar.read", &json!({"handle":f.devices[1]})).unwrap();
    edit["model"] = json!(f.model);
    edit["backup_dir"] = json!(f.root);
    edit["at"] = json!([62.5, 220.]);
    f.s.execute("electrical.unifilar.update", &edit).unwrap();
    let p = f.path(&l["handle"]);
    assert_eq!(p.last().copied(), Some([62.5, 220.]));
    assert!(p.windows(2).all(|v| {
        let dx = (v[1][0] - v[0][0]).abs();
        let dy = (v[1][1] - v[0][1]).abs();
        dx < 1e-6 || dy < 1e-6 || (dx - dy).abs() < 1e-6
    }));
}

#[test]
fn shared_vertical_prefix_is_trimmed_and_marks_the_actual_departure_in_both_directions() {
    for reverse in [false, true] {
        let mut f = fixture();
        let lower =
            f.s.execute("electrical.unifilar.insert", &json!({"model":f.model,"id":"dev","at":[62.5,185.],"fields":{},"backup_dir":f.root})).unwrap();
        let base = f.link(wire(&f, f.port(1, "out"), json!({"handle":lower["handle"],"port":"in"}), json!([])));
        let tap = json!({"wire":base["handle"],"part":"route","fraction":0.3});
        // Original contact is Y222.1; the branch follows the parent down to Y215.
        let mut l = json!({"version":3,"kind":"wire","tap":tap,"to":f.port(2,"out"),"y":215.,"waypoints":[[62.5,215.],[80.,215.],[80.,230.],[97.5,230.]],"route_mode":"orthogonal"});
        if reverse {
            l.as_object_mut().unwrap().remove("tap");
            l.as_object_mut().unwrap().remove("to");
            l["from"] = f.port(2, "out");
            l["end_tap"] = tap;
            l["waypoints"] = json!([[97.5, 230.], [80., 230.], [80., 215.], [62.5, 215.]]);
        }
        let added = f.link(l);
        let path = f.path(&added["handle"]);
        assert_eq!(if reverse { path.last() } else { path.first() }, Some(&[62.5, 215.]));
        assert_eq!(dot_count(&f), 1);
        let graph = f.s.execute("electrical.unifilar.network.read", &json!({"model":f.model})).unwrap();
        assert_eq!(graph["junctions"][0]["at"], json!([62.5, 215.]));
        f.s.execute("electrical.unifilar.link.delete", &json!({"handle":added["handle"],"backup_dir":f.root})).unwrap();
        assert_eq!(dot_count(&f), 0);
        f.s.execute("u", &json!({})).unwrap();
        assert_eq!(dot_count(&f), 1);
    }
}
#[test]
fn electrical_tracing_uses_native_polar_and_acquired_point_guides() {
    use cadcraft_engine::{
        Input,
        geom::Vec2,
        snap::tracking::{CursorQuery, DWELL},
    };
    let mut f = fixture();
    f.s.execute("setvar", &json!({"name":"OSMODE","value":0})).unwrap();
    f.s.execute("setvar", &json!({"name":"SNAPUNIT","value":[0.5,12.5]})).unwrap();
    f.s.execute("setvar", &json!({"name":"SNAPMODE","value":1})).unwrap();
    f.s.settings.polarmode = true;
    f.s.settings.otrack = true;
    f.s.settings.polarang = std::f64::consts::FRAC_PI_4;
    f.s.execute("electrical.unifilar.link.prepare", &json!({"model":f.model,"trace":true,"backup_dir":f.root})).unwrap();
    f.s.start("electrical.unifilar.link.place").unwrap();
    assert!(f.s.current_prompt().unwrap().base.is_none());
    f.s.input(Input::Point(Vec2::new(62.5, 238.))).unwrap();
    let base = f.s.current_prompt().unwrap().base;
    assert_eq!(base, Some(Vec2::new(62.5, 238.)));
    let guide_doc = f.s.state().unwrap().doc.clone();
    let query = |now| CursorQuery { base, deferred: false, corner: false, aperture: 0.2, tol: 0.1, now, shift: false };
    let horizontal = f.s.snap_cursor(Vec2::new(80., 238.04), &query(1.));
    assert!(horizontal.track.is_some());
    assert!((horizontal.point.y - 238.).abs() < 1e-6);
    let diagonal = f.s.snap_cursor(Vec2::new(72.5, 248.04), &query(2.));
    assert!(diagonal.track.is_some());
    let d = diagonal.point - base.unwrap();
    assert!((d.x - d.y).abs() < 1e-6);
    f.s.settings.polarmode = false;
    let target = Vec2::new(97.5, 250.);
    assert!(f.s.snap_cursor(target, &query(3.)).snap.is_some());
    f.s.snap_cursor(target, &query(3. + DWELL + 0.1));
    let acquired = f.s.snap_cursor(Vec2::new(110., 250.04), &query(4.));
    assert!(acquired.track.is_some());
    assert!((acquired.point.y - 250.).abs() < 1e-6);
    f.s.settings.otrack = false;
    f.s.tracking.clear();
    assert!(f.s.snap_cursor(Vec2::new(110., 250.04), &query(5.)).track.is_none());
    assert!(Arc::ptr_eq(&guide_doc, &f.s.state().unwrap().doc));
    f.s.input(Input::Point(Vec2::new(80., 220.))).unwrap();
    assert_eq!(f.s.current_prompt().unwrap().base, Some(Vec2::new(80., 220.)));
    f.s.input(Input::Keyword("Undo".into())).unwrap();
    assert_eq!(f.s.current_prompt().unwrap().base, base);
    // An abandoned API draft must not alter the normal CAD tracking/grid order.
    let stale = f.s.unifilar_link.clone();
    f.s.cancel();
    f.s.unifilar_link = stale;
    f.s.settings.polarmode = true;
    f.s.tracking.clear();
    assert!(f.s.snap_cursor(Vec2::new(80., 238.04), &query(6.)).track.is_none());
}

#[test]
fn refreshing_an_older_overlapping_branch_marks_its_separation_before_explicit_edit_normalizes_it() {
    use cadcraft_engine::{Input, doc::RawTag, geom::Vec2};
    let mut f = fixture();
    let lower =
        f.s.execute("electrical.unifilar.insert", &json!({"model":f.model,"id":"dev","at":[62.5,185.],"fields":{},"backup_dir":f.root})).unwrap();
    let base = f.link(wire(&f, f.port(1, "out"), json!({"handle":lower["handle"],"port":"in"}), json!([])));
    let added=f.link(json!({"version":3,"kind":"wire","tap":{"wire":base["handle"],"part":"route","fraction":0.3},"to":f.port(2,"out"),"y":215.,"waypoints":[[62.5,215.],[80.,215.],[80.,230.],[97.5,230.]],"route_mode":"orthogonal"}));
    let h = Handle::parse_hex(added["handle"].as_str().unwrap()).unwrap();
    let e = f.s.doc().unwrap().entity(h).unwrap();
    let EntityKind::Insert(i) = &e.kind else { panic!() };
    let name = i.block.clone();
    let mut meta: Value =
        serde_json::from_str(&e.common.xdata.iter().filter(|t| t.code == 1000).map(|t| t.value.as_str()).collect::<String>()).unwrap();
    meta["link"]["tap"]["fraction"] = json!(0.3);
    meta["link"]["waypoints"].as_array_mut().unwrap().insert(0, json!([62.5, 215.]));
    meta["link"]["segment_modes"] = json!([]);
    meta["path"].as_array_mut().unwrap().insert(0, json!([62.5, 222.1]));
    let mut b = f.s.doc().unwrap().block(&name).unwrap().as_ref().clone();
    let raw_handle = f.s.doc_mut().unwrap().new_handle();
    b.entities.push(Entity::new(raw_handle, EntityKind::Line(Line { a: Vec3::new(62.5, 222.1, 0.), b: Vec3::new(62.5, 215., 0.) })));
    f.s.doc_mut().unwrap().blocks.insert(name.clone(), Arc::new(b));
    f.s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            e.common.xdata = vec![RawTag { code: 1001, value: "CC_UNIFILARE_LINK".into() }, RawTag { code: 1000, value: meta.to_string() }]
        })
        .unwrap();
    let mut edit = f.s.execute("electrical.unifilar.read", &json!({"handle":f.devices[0]})).unwrap();
    edit["model"] = json!(f.model);
    edit["backup_dir"] = json!(f.root);
    f.s.execute("electrical.unifilar.update", &edit).unwrap();
    assert_eq!(f.path(&added["handle"])[0], [62.5, 222.1]);
    let b = f.s.doc().unwrap().block(&name).unwrap();
    assert!(b.entities.iter().any(|e| matches!(&e.kind,EntityKind::Solid(s) if s.corners[0].xy().near(Vec2::new(62.5,215.),1e-6))));
    f.s.execute("electrical.unifilar.link.prepare", &json!({"model":f.model,"handle":h.hex(),"trace":true,"backup_dir":f.root})).unwrap();
    f.s.start("electrical.unifilar.link.place").unwrap();
    f.s.input(Input::Enter).unwrap();
    assert_eq!(f.path(&added["handle"])[0], [62.5, 215.]);
}
