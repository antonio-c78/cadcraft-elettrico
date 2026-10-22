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
fn fixture() -> Fixture {
    set_io(IoHooks {
        read: |b, n| cadcraft_io::read(b, n).map_err(|e| e.to_string()),
        write: |d, n| cadcraft_io::write(d, n).map_err(|e| e.to_string()),
        plot: None,
    });
    let root = std::env::temp_dir()
        .join(format!("cadcraft-link-test-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
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
