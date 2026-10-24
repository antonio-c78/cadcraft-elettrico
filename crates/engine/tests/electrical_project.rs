//! Synthetic project fixtures: persistence, identity, page continuity and failure isolation.
use cadcraft_engine::{
    Session,
    cmd::file::{IoHooks, set_io, set_write_as},
    doc::{Drawing, EntityKind, Handle},
    project,
};
use serde_json::{Value, json};
struct Temp(std::path::PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("cadcraft-project-{}", project::id()));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn file(&self) -> String {
        self.0.join("test.cce").to_string_lossy().into_owned()
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        if self.0.starts_with(std::env::temp_dir()) && self.0.file_name().is_some_and(|n| n.to_string_lossy().starts_with("cadcraft-project-")) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
fn setup() -> Session {
    set_write_as(|d, n, o| {
        cadcraft_io::write_with(d, n, &cadcraft_io::SaveOptions { version: o.version, binary: o.binary }).map_err(|e| e.to_string())
    });
    set_io(IoHooks {
        read: |b, n| cadcraft_io::read(b, n).map_err(|e| e.to_string()),
        write: |d, n| cadcraft_io::write(d, n).map_err(|e| e.to_string()),
        plot: None,
    });
    let mut s = Session::empty();
    s.execute("electrical.project.new", &json!({"name":"Villa"})).unwrap();
    for title in ["Generale", "Piano terra"] {
        s.open_drawing(Drawing::new_metric(), title, None);
        s.execute("electrical.project.add", &json!({})).unwrap();
    }
    s
}
fn listing(s: &mut Session) -> Value {
    s.execute("electrical.project.list", &json!({})).unwrap()
}
fn sheet(s: &mut Session, i: usize) -> String {
    listing(s)["sheets"][i]["id"].as_str().unwrap().into()
}
fn switch(s: &mut Session, id: &str) {
    s.execute("electrical.project.switch", &json!({"id":id})).unwrap();
}
fn text(s: &Session, h: &Value) -> String {
    let e = s.doc().unwrap().entity(Handle::parse_hex(h.as_str().unwrap()).unwrap()).unwrap();
    let EntityKind::Insert(i) = &e.kind else { panic!() };
    s.doc()
        .unwrap()
        .block(&i.block)
        .unwrap()
        .entities
        .iter()
        .find_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t.value.clone()) } else { None })
        .unwrap()
}
#[test]
fn cce_roundtrip_closed_tabs_and_save_copy_keep_sheet_identities_without_touching_sources() {
    let t = Temp::new();
    let mut s = setup();
    let first = sheet(&mut s, 0);
    let second = sheet(&mut s, 1);
    switch(&mut s, &first);
    s.execute("line", &json!({"points":[[0,0],[10,10]]})).unwrap();
    let uid = s.state().unwrap().uid;
    s.execute("close", &json!({})).unwrap();
    assert!(s.docs.iter().all(|d| d.uid != uid));
    assert_eq!(s.project.as_ref().unwrap().closed.len(), 1);
    let path = t.file();
    s.execute("electrical.project.save", &json!({"path":path})).unwrap();
    assert!(!project::dirty(&s));
    let id = listing(&mut s)["id"].clone();
    let mut opened = Session::empty();
    opened.execute("open", &json!({"path":path})).unwrap();
    let data = listing(&mut opened);
    assert_eq!(data["id"], id);
    assert_eq!(data["sheets"][0]["id"], first);
    assert_eq!(data["sheets"][1]["id"], second);
    assert!(!project::dirty(&opened));
    switch(&mut opened, &first);
    assert_eq!(opened.doc().unwrap().model.len(), 1);
    opened.execute("line", &json!({"points":[[20,0],[20,10]]})).unwrap();
    opened.execute("qsave", &json!({})).unwrap();
    assert!(!project::dirty(&opened));
    assert!(std::path::Path::new(&format!("{path}.bak")).exists());
    opened.execute("line", &json!({"points":[[30,0],[30,10]]})).unwrap();
    let header = opened.doc().unwrap().header.clone();
    opened.execute("saveas", &json!({"path":t.0.join("export.dxf"),"version":"2013"})).unwrap();
    assert!(project::dirty(&opened), "exporting a DXF must not mark the CCE saved");
    assert_eq!(opened.doc().unwrap().header, header);
    let copy = t.0.join("copy.cce");
    opened.execute("electrical.project.save", &json!({"path":copy})).unwrap();
    assert_eq!(listing(&mut opened)["id"], id);
}
#[test]
fn reference_pair_is_bidirectional_follows_renumbering_and_reports_erased_end_after_undo() {
    let mut s = setup();
    let a = sheet(&mut s, 0);
    let b = sheet(&mut s, 1);
    switch(&mut s, &a);
    let first = s.execute("electrical.project.reference.insert", &json!({"at":[40,100]})).unwrap();
    assert_eq!(listing(&mut s)["references"][0]["status"], "pending");
    switch(&mut s, &b);
    let second = s.execute("electrical.project.reference.insert", &json!({"at":[80,100],"pair":first["pair"],"direction":"left"})).unwrap();
    assert_eq!(listing(&mut s)["references"][0]["status"], "connected");
    assert_eq!(text(&s, &second["handle"]), "01");
    s.execute("electrical.project.sheet", &json!({"id":a,"number":"07","position":1})).unwrap();
    assert_eq!(text(&s, &second["handle"]), "07");
    let nav = s.execute("electrical.project.reference.follow", &json!({"handle":second["handle"]})).unwrap();
    assert_ne!(nav["uid"], second["uid"]);
    assert_eq!(text(&s, &first["handle"]), "02");
    s.execute("electrical.project.reference.follow", &json!({"handle":first["handle"]})).unwrap();
    s.execute("u", &json!({})).unwrap();
    assert_eq!(listing(&mut s)["references"][0]["status"], "pending");
    s.execute("redo", &json!({})).unwrap();
    assert_eq!(listing(&mut s)["references"][0]["status"], "connected");
    assert!(s.execute("electrical.project.reference.insert", &json!({"at":[100,100],"pair":first["pair"]})).is_err());
}
#[test]
fn duplicate_gives_new_sheet_and_reference_id_and_does_not_join_original_pair() {
    let mut s = setup();
    let a = sheet(&mut s, 0);
    switch(&mut s, &a);
    let end = s.execute("electrical.project.reference.insert", &json!({"at":[40,100]})).unwrap();
    s.execute("electrical.project.duplicate", &json!({"id":a})).unwrap();
    let data = listing(&mut s);
    assert_eq!(data["sheets"].as_array().unwrap().len(), 3);
    let refs = data["references"].as_array().unwrap();
    assert_eq!(refs.len(), 2);
    assert!(refs.iter().all(|r| r["status"] == "pending"));
    assert_eq!(refs.iter().filter(|r| r["id"] == end["pair"]).count(), 1);
}
#[test]
fn invalid_import_conflicts_and_damaged_package_preserve_session_and_last_valid_file() {
    let t = Temp::new();
    let mut s = setup();
    let path = t.file();
    s.execute("electrical.project.save", &json!({"path":path})).unwrap();
    let original = std::fs::read(&path).unwrap();
    let mut other = Session::empty();
    other.execute("electrical.project.open", &json!({"path":path})).unwrap();
    other.execute("electrical.project.sheet", &json!({"name":"Altro autore"})).unwrap();
    other.execute("electrical.project.save", &json!({})).unwrap();
    let changed = std::fs::read(&path).unwrap();
    assert_ne!(changed, original);
    assert!(s.execute("electrical.project.save", &json!({})).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), changed);
    let before = listing(&mut s);
    assert!(s.execute("electrical.project.sheet", &json!({"id":before["sheets"][0]["id"],"number":"02"})).is_err());
    assert_eq!(listing(&mut s)["sheets"], before["sheets"]);
    s.execute("new", &json!({"metric":true})).unwrap();
    let count = listing(&mut s)["sheets"].as_array().unwrap().len();
    assert!(s.execute("electrical.project.add", &json!({"model":"missing-model.json"})).is_err());
    assert_eq!(listing(&mut s)["sheets"].as_array().unwrap().len(), count);
    let mut empty = Session::new();
    let uid = empty.state().unwrap().uid;
    std::fs::write(&path, b"not a project").unwrap();
    assert!(empty.execute("open", &json!({"path":path})).is_err());
    assert_eq!(empty.state().unwrap().uid, uid);
    assert!(empty.project.is_none());
}
