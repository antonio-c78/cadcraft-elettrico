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
