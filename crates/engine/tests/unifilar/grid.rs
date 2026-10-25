//! Native drafting controls and the electrical cursor share one per-sheet lattice.
use super::*;
use cadcraft_engine::{Input, geom::Vec2, snap::tracking::CursorQuery};
fn grid_fixture() -> Fixture {
    let mut f = fixture();
    let model: Value = serde_json::from_slice(&std::fs::read(&f.model).unwrap()).unwrap();
    let snapshot = json!({"snapshot_version":1,"id":"test","columns":model["columns"],
        "rows":[{"id":"sigla","label":"Sigla","y0":146.,"y1":153.,"center_y":149.5,"padding_mm":1.5,"max_lines":1}],
        "drawing_area":model["drawing_area"],"pe":model["pe"]});
    let h =
        f.s.doc().unwrap().model.iter().find(|e| e.common.xdata.iter().any(|t| t.code == 1001 && t.value == "CC_UNIFILARE_MODELO")).unwrap().handle;
    f.s.doc_mut()
        .unwrap()
        .modify_entity(h, |e| {
            e.common.xdata.push(RawTag { code: 1001, value: "CC_UNIFILARE_LAYOUT".into() });
            for chunk in snapshot.to_string().chars().collect::<Vec<_>>().chunks(200) {
                e.common.xdata.push(RawTag { code: 1000, value: chunk.iter().collect() });
            }
        })
        .unwrap();
    let drawing = f.s.doc().unwrap().clone();
    f.s.open_drawing(drawing, "Unifilar grid", None);
    assert!(f.s.state().unwrap().unifilar_grid.is_some());
    f.s.settings.osmode = 0;
    f.s.settings.polarmode = false;
    f.s.settings.otrack = false;
    f
}
fn q(base: Option<Vec2>) -> CursorQuery {
    CursorQuery { base, deferred: false, corner: false, aperture: 0.1, tol: 0.1, now: 1., shift: false }
}
#[test]
fn native_grid_settings_are_independent_persisted_and_scoped_to_the_sheet() {
    let mut f = grid_fixture();
    let profile = f.s.settings.clone();
    let before = f.s.state().unwrap().doc.clone();
    assert_eq!(f.s.snap_cursor(Vec2::new(80.3, 225.9), &q(None)).point, Vec2::new(80., 225.));
    for (name, value) in
        [("SNAPUNIT", json!([3., 4.])), ("GRIDUNIT", json!([1., 2.])), ("GRIDMAJOR", json!(3)), ("SNAPBASE", json!([1., 2.])), ("SNAPANG", json!(90))]
    {
        f.s.execute("setvar", &json!({"name":name,"value":value})).unwrap();
    }
    assert!(Arc::ptr_eq(&before, &f.s.state().unwrap().doc));
    assert!(f.s.state().unwrap().is_dirty());
    let settings = f.s.drafting_settings();
    assert_eq!(settings.snapunit, Vec2::new(3., 4.));
    assert_eq!(settings.gridunit, Vec2::new(1., 2.));
    assert_eq!(settings.gridmajor, 3);
    let raw = Vec2::new(1., 2.) + Vec2::new(4.2, 5.1).rotate(std::f64::consts::FRAC_PI_2);
    assert!(f.s.snap_cursor(raw, &q(None)).point.near(Vec2::new(-3., 5.), 1e-9));
    let path = f.root.join("custom-grid.dxf");
    f.s.execute("saveas", &json!({"path":path})).unwrap();
    assert!(!f.s.state().unwrap().is_dirty());
    let drawing = cadcraft_io::read(&std::fs::read(path).unwrap(), "sheet.dxf").unwrap();
    f.s.open_drawing(drawing, "Reload", None);
    assert_eq!(f.s.drafting_settings().snapunit, settings.snapunit);
    assert_eq!(f.s.drafting_settings().gridunit, settings.gridunit);
    assert_eq!(f.s.drafting_settings().gridmajor, 3);
    assert!(f.s.drafting_grid_frame().0.near(Vec2::new(1., 2.), 1e-9));
    assert!((f.s.drafting_grid_frame().1 - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
    f.s.active = 0;
    assert_eq!(f.s.drafting_settings().snapunit, profile.snapunit);
    assert_eq!(f.s.settings.gridmajor, profile.gridmajor);
}
#[test]
fn native_f9_and_spacing_changes_do_not_invalidate_a_running_connection() {
    let mut f = grid_fixture();
    f.s.execute("electrical.unifilar.link.prepare", &json!({"model":f.model,"trace":true,"backup_dir":f.root})).unwrap();
    f.s.start("electrical.unifilar.link.place").unwrap();
    f.s.input(Input::Point(Vec2::new(62.5, 238.))).unwrap();
    let before = f.s.state().unwrap().doc.clone();
    f.s.settings.polarmode = true;
    let base = Some(Vec2::new(62.5, 238.));
    let on = f.s.snap_cursor(Vec2::new(80.1, 238.02), &q(base));
    assert!(on.point.near(Vec2::new(80., 237.5), 1e-6));
    f.s.execute("snap", &json!({"on":false})).unwrap();
    assert!(Arc::ptr_eq(&before, &f.s.state().unwrap().doc));
    let off = f.s.snap_cursor(Vec2::new(80.1, 238.02), &q(base));
    assert!((off.point.y - 238.).abs() < 1e-6);
    assert!(off.track.is_some());
    f.s.execute("snap", &json!({"on":true})).unwrap();
    f.s.execute("setvar", &json!({"name":"SNAPUNIT","value":[2.5,12.5]})).unwrap();
    assert!(Arc::ptr_eq(&before, &f.s.state().unwrap().doc));
    for p in [(62.5, 225.), (80., 225.), (80., 262.5), (97.5, 262.5), (97.5, 250.)] {
        f.s.input(Input::Point(Vec2::new(p.0, p.1))).unwrap();
    }
    assert!(f.s.running.is_none());
    assert_eq!(f.s.drafting_settings().snapunit, Vec2::new(2.5, 12.5));
    assert_eq!(f.s.doc().unwrap().header.snap_unit(), Vec2::new(2.5, 12.5));
}
#[test]
fn wire_attachment_uses_the_grid_when_f9_is_on_and_is_free_when_it_is_off() {
    let mut f = grid_fixture();
    f.link(wire(&f, f.port(0, "in"), f.port(3, "in"), json!([[132.5, 275.]])));
    f.s.execute("electrical.unifilar.link.prepare", &json!({"model":f.model,"trace":true,"backup_dir":f.root})).unwrap();
    f.s.start("electrical.unifilar.link.place").unwrap();
    let point = Vec2::new(80.6, 275.02);
    assert!(f.s.snap_cursor(point, &q(None)).point.near(Vec2::new(80., 275.), 1e-6));
    // Exact device points take precedence even when their library coordinates are off grid.
    assert_eq!(f.s.snap_cursor(Vec2::new(62.51, 238.01), &q(None)).point, Vec2::new(62.5, 238.));
    f.s.execute("snap", &json!({"on":false})).unwrap();
    assert!(f.s.snap_cursor(point, &q(None)).point.near(Vec2::new(80.6, 275.), 1e-6));
    f.s.input(Input::Cancel).unwrap();
    assert!(!f.s.drafting_settings().snapmode);
    assert_eq!(f.s.doc().unwrap().header.i64("SNAPMODE", 1), 0);
}
