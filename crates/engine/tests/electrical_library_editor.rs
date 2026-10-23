//! Original synthetic geometry only. Catalog publication must never alter an open project.
use cadcraft_engine::cmd::file::{IoHooks, set_io};
use cadcraft_engine::{
    Session,
    doc::{Block, Drawing, Entity, EntityKind, Line},
    geom::Vec3,
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};

struct Fixture {
    s: Session,
    root: PathBuf,
    catalog: PathBuf,
    bytes: Vec<u8>,
}
static FIXTURE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
impl Fixture {
    fn new(general: bool) -> Self {
        set_io(IoHooks {
            read: |b, n| cadcraft_io::read(b, n).map_err(|e| e.to_string()),
            write: |d, n| cadcraft_io::write(d, n).map_err(|e| e.to_string()),
            plot: None,
        });
        let root = std::env::temp_dir().join(format!(
            "cadcraft-editor-{}-{}-{}",
            std::process::id(),
            FIXTURE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let mut d = Drawing::new_metric();
        let mut b = Block::new("DEVICE");
        b.entities.push(Entity::new(d.new_handle(), EntityKind::Line(Line { a: Vec3::ZERO, b: Vec3::new(0., -12., 0.) })));
        d.blocks.insert("DEVICE".into(), Arc::new(b));
        let bytes = cadcraft_io::write(&d, "library.dxf").unwrap();
        std::fs::write(root.join("library.dxf"), &bytes).unwrap();
        let sym = json!({"id":"device","name":"Device","block":"DEVICE","category":if general {json!(["Test"])}else{json!("Test")},"kind":"device","modules":1,"composer":true,"standalone":true,"ports":[{"id":"in","role":"power","position":[0,0,0]},{"id":"out","role":"power","position":[0,-12,0]}]});
        let catalog = root.join("catalogo.json");
        let raw = if general {
            json!({"version":2,"library":"library.dxf","components":[sym],"favorites":["device"]})
        } else {
            json!({"format":"cadcraft-unifilar-symbols","version":1,"library":"library.dxf","symbols":[sym]})
        };
        std::fs::write(&catalog, raw.to_string()).unwrap();
        let mut s = Session::new();
        s.execute("line", &json!({"points":[[1,2],[3,4]]})).unwrap();
        Self { s, root, catalog, bytes }
    }
    fn open(&mut self) {
        self.s.execute("electrical.library.open", &json!({"catalog":self.catalog,"id":"device"})).unwrap();
    }
    fn point(&mut self, id: &str, at: [f64; 2]) {
        self.s.execute("electrical.library.point", &json!({"point":id,"at":at})).unwrap();
    }
    fn raw(&self) -> Value {
        serde_json::from_slice(&std::fs::read(&self.catalog).unwrap()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn copy_publishes_geometry_ports_and_origin_without_changing_project_or_original() {
    let mut f = Fixture::new(false);
    let project = f.s.state().unwrap().doc.clone();
    let project_uid = f.s.state().unwrap().uid;
    f.open();
    assert_eq!(f.s.docs.len(), 2);
    f.s.execute("line", &json!({"points":[[-2,-6],[2,-6]]})).unwrap();
    f.point("origin", [0., 1.]);
    f.point("in", [0., 2.]);
    let r = f.s.execute("electrical.library.save", &json!({"name":"Device copy"})).unwrap();
    assert_eq!(f.s.docs.len(), 1);
    assert_eq!(f.s.state().unwrap().uid, project_uid);
    assert!(Arc::ptr_eq(&project, &f.s.state().unwrap().doc));
    assert_eq!(std::fs::read(f.root.join("library.dxf")).unwrap(), f.bytes);
    let raw = f.raw();
    let entries = raw["symbols"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["block"], "DEVICE");
    assert_eq!(entries[0]["ports"][0]["position"], json!([0, 0, 0]));
    let copy = &entries[1];
    assert_eq!(copy["id"], r["id"]);
    assert_eq!(copy["name"], "Device copy");
    assert_eq!(copy["ports"][0]["position"], json!([0., 1., 0]));
    assert_eq!(copy["ports"][1]["position"], json!([0., -13., 0]));
    let bytes = std::fs::read(f.root.join(raw["library"].as_str().unwrap())).unwrap();
    let library = cadcraft_io::read(&bytes, "new.dxf").unwrap();
    let b = library.block(copy["block"].as_str().unwrap()).unwrap();
    assert_eq!(b.entities.len(), 2);
    assert!(b.entities.iter().all(|e| !matches!(e.kind, EntityKind::Point(_))));
    assert_eq!(library.block("DEVICE").unwrap().entities.len(), 1);
    assert!(std::fs::read_dir(f.root.join("backups")).unwrap().next().is_some());
    assert_eq!(copy["bounds"]["min"][1], -13.);
}
#[test]
fn conflicting_disk_changes_refuse_publication_and_keep_editor_open() {
    let mut f = Fixture::new(false);
    f.open();
    f.point("out", [0., -14.]);
    let mut raw = f.raw();
    raw["symbols"][0]["name"] = json!("Changed outside");
    let external = raw.to_string();
    std::fs::write(&f.catalog, &external).unwrap();
    assert!(f.s.execute("electrical.library.save", &json!({})).is_err());
    assert_eq!(std::fs::read_to_string(&f.catalog).unwrap(), external);
    assert_eq!(f.s.docs.len(), 2);
    assert!(f.s.execute("electrical.library.read", &json!({})).is_ok());
    assert_eq!(std::fs::read(f.root.join("library.dxf")).unwrap(), f.bytes);
}
#[test]
fn point_edits_are_undoable_and_missing_origin_prevents_save() {
    let mut f = Fixture::new(false);
    f.open();
    f.point("out", [0., -20.]);
    f.s.execute("undo", &json!({})).unwrap();
    let data = f.s.execute("electrical.library.read", &json!({})).unwrap();
    assert_eq!(data["points"]["out"], json!([0., -12.]));
    let h = f.s.doc().unwrap().model.iter().find(|e| e.common.xdata.iter().any(|t| t.code == 1000 && t.value == "origin")).unwrap().handle;
    f.s.execute("erase", &json!({"handles":[h.hex()]})).unwrap();
    let original = std::fs::read(&f.catalog).unwrap();
    assert!(f.s.execute("electrical.library.save", &json!({})).is_err());
    assert_eq!(std::fs::read(&f.catalog).unwrap(), original);
    assert!(f.s.execute("electrical.library.read", &json!({})).unwrap()["points_error"].is_string());
    f.point("origin", [0., 0.]);
    assert!(f.s.execute("electrical.library.save", &json!({})).is_ok());
}
#[test]
fn general_catalog_copy_keeps_favorites_and_is_independent_of_shared_blocks() {
    let mut f = Fixture::new(true);
    f.open();
    f.point("origin", [0., 0.]);
    f.s.execute("circle", &json!({"center":[0,-6],"radius":2})).unwrap();
    f.s.execute("electrical.library.save", &json!({"name":"Round variant","copy":true})).unwrap();
    let raw = f.raw();
    assert_eq!(raw["favorites"], json!(["device"]));
    assert_eq!(raw["components"].as_array().unwrap().len(), 2);
    assert_eq!(raw["components"][0]["block"], "DEVICE");
    assert_ne!(raw["components"][0]["block"], raw["components"][1]["block"]);
    let c = &raw["components"][1];
    let r=f.s.execute("electrical.symbol.insert",&json!({"library":f.root.join(raw["library"].as_str().unwrap()),"symbol":{"id":c["id"],"name":c["name"],"block":c["block"],"category":["Test"],"factor":1,"width_mm":8,"height_mm":8,"anchor":[0.5,0.5],"origin":c["editor_origin"]},"at":[20,20],"rotation":0,"backup_dir":f.root})).unwrap();
    let handle: cadcraft_engine::doc::Handle = serde_json::from_value(r["handle"].clone()).unwrap();
    let inserted = f.s.doc().unwrap().entity(handle).unwrap();
    let EntityKind::Insert(i) = &inserted.kind else { panic!("expected reference") };
    assert!(
        f.s.doc()
            .unwrap()
            .block(&i.block)
            .unwrap()
            .entities
            .iter()
            .any(|e| matches!(&e.kind,EntityKind::Line(l) if l.a.xy().near(cadcraft_engine::geom::Vec2::ZERO,1e-7)))
    );
}
#[test]
fn cancel_requires_discard_confirmation_and_returns_to_original_project_by_uid() {
    let mut f = Fixture::new(false);
    let uid = f.s.state().unwrap().uid;
    let bytes = std::fs::read(&f.catalog).unwrap();
    f.open();
    assert!(f.s.execute("electrical.library.cancel", &json!({})).is_err());
    assert!(f.s.execute("electrical.library.point", &json!({"point":"in","at":[1e100,0]})).is_err());
    assert!(f.s.execute("electrical.library.open", &json!({"catalog":f.catalog,"id":"device"})).is_err());
    f.s.execute("electrical.library.cancel", &json!({"discard":true})).unwrap();
    assert_eq!(f.s.state().unwrap().uid, uid);
    assert_eq!(std::fs::read(&f.catalog).unwrap(), bytes);
}

#[test]
fn replacing_an_entry_does_not_change_other_variants_sharing_its_old_block() {
    let mut f = Fixture::new(true);
    let mut raw = f.raw();
    let mut other = raw["components"][0].clone();
    other["id"] = json!("other");
    other["name"] = json!("Other variant");
    raw["components"].as_array_mut().unwrap().push(other);
    std::fs::write(&f.catalog, raw.to_string()).unwrap();
    f.open();
    f.s.execute("circle", &json!({"center":[0,-6],"radius":2})).unwrap();
    f.s.execute("electrical.library.save", &json!({"copy":false})).unwrap();
    let raw = f.raw();
    assert_eq!(raw["components"].as_array().unwrap().len(), 2);
    assert_ne!(raw["components"][0]["block"], "DEVICE");
    assert_eq!(raw["components"][1]["block"], "DEVICE");
    let library = cadcraft_io::read(&std::fs::read(f.root.join(raw["library"].as_str().unwrap())).unwrap(), "new.dxf").unwrap();
    assert_eq!(library.block("DEVICE").unwrap().entities.len(), 1);
    assert_eq!(library.block(raw["components"][0]["block"].as_str().unwrap()).unwrap().entities.len(), 2);
}

#[test]
fn legacy_general_symbols_keep_their_existing_insertion_anchor() {
    let mut f = Fixture::new(true);
    f.open();
    let state = f.s.execute("electrical.library.read", &json!({})).unwrap();
    assert_eq!(state["points"]["origin"], json!([0., -6.]));
    let saved = f.s.execute("electrical.library.save", &json!({"copy":true})).unwrap();
    f.s.execute("electrical.library.open", &json!({"catalog":f.catalog,"id":saved["id"]})).unwrap();
    let state = f.s.execute("electrical.library.read", &json!({})).unwrap();
    assert_eq!(state["points"]["origin"], json!([0., 0.]));
    assert_eq!(state["points"]["in"], json!([0., 6.]));
}
