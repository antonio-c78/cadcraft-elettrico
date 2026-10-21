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
    let active = s.active;
    let count = s.docs.len();
    std::fs::write(&model, config.to_string().replace("template.dxf", "../template.dxf")).unwrap();
    assert!(s.execute("electrical.unifilar.new", &json!({"model":model})).is_err());
    assert_eq!(s.docs.len(), count);
    assert_eq!(s.active, active);
    std::fs::remove_dir_all(root).unwrap();
}
