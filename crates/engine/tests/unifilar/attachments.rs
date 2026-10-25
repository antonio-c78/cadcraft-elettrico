//! Terminal clearance and retraced-parent contacts use synthetic artwork only.
use super::*;
use cadcraft_engine::{Input, geom::Vec2};

#[test]
fn snapped_terminal_dogleg_connects_to_a_trunk_in_both_directions() {
    for offset in [-0.25, 0.25] {
        for reverse in [false, true] {
            let mut f = fixture();
            f.link(wire(&f, f.port(0, "in"), f.port(3, "in"), json!([[27.5, 282.5], [132.5, 282.5]])));
            let terminal = Vec2::new(62.5, 250.);
            let trunk = Vec2::new(62.5 + offset, 282.5);
            f.s.execute("electrical.unifilar.link.prepare", &json!({"model":f.model,"trace":true,"backup_dir":f.root})).unwrap();
            f.s.start("electrical.unifilar.link.place").unwrap();
            let (start, end) = if reverse { (terminal, trunk) } else { (trunk, terminal) };
            f.s.input(Input::Point(start)).unwrap();
            f.s.input(Input::Point(end)).unwrap();
            assert!(f.s.running.is_none(), "offset {offset}, reverse {reverse}");
            assert_eq!(dot_count(&f), 1);
            f.s.execute("undo", &json!({})).unwrap();
            assert_eq!(dot_count(&f), 0);
        }
    }
}

#[test]
fn terminal_window_does_not_allow_an_inward_dogleg_through_the_device_or_its_text() {
    let mut f = fixture();
    let before = f.s.doc().unwrap().model.clone();
    let params =
        json!({"model":f.model,"link":wire(&f,f.port(1,"in"),f.port(2,"in"),json!([[62.75,250.],[62.75,240.],[97.5,240.]])),"backup_dir":f.root});
    assert!(f.s.execute("electrical.unifilar.link.insert", &params).is_err());
    assert_eq!(before, f.s.doc().unwrap().model);
    let h = Handle::parse_hex(&f.devices[1]).unwrap();
    let EntityKind::Insert(i) = &f.s.doc().unwrap().entity(h).unwrap().kind else { panic!() };
    let name = i.block.clone();
    let mut b = f.s.doc().unwrap().block(&name).unwrap().as_ref().clone();
    let mut text = b.entities.iter().find(|e| matches!(e.kind, EntityKind::Text(_))).unwrap().as_ref().clone();
    if let EntityKind::Text(t) = &mut text.kind {
        t.insert = Vec3::new(-0.5, 1.5, 0.);
        t.height = 1.;
    }
    b.entities.replace(text);
    f.s.doc_mut().unwrap().blocks.insert(name, Arc::new(b));
    assert!(f.s.execute("electrical.unifilar.link.insert",&json!({"model":f.model,"link":wire(&f,f.port(1,"in"),f.port(0,"in"),json!([[62.75,250.],[62.75,282.5],[27.5,282.5]])),"backup_dir":f.root})).is_err());
}

#[test]
fn branch_on_an_overshot_corner_marks_the_actual_tee_and_clears_after_delete() {
    let mut f = fixture();
    let base = f.link(wire(&f, f.port(0, "out"), f.port(1, "in"), json!([[27.5, 180.], [45., 180.], [45., 282.5], [62.9, 282.5], [62.5, 282.5]])));
    assert_eq!(dot_count(&f), 0);
    let path = f.path(&base["handle"]);
    let total: f64 = path.windows(2).map(|s| Vec2::new(s[0][0], s[0][1]).dist(Vec2::new(s[1][0], s[1][1]))).sum();
    let mut along = 0.;
    for s in path.windows(2) {
        along += Vec2::new(s[0][0], s[0][1]).dist(Vec2::new(s[1][0], s[1][1]));
        if s[1] == [62.9, 282.5] {
            break;
        }
    }
    let branch=f.link(json!({"version":3,"kind":"wire","tap":{"wire":base["handle"],"part":"route","fraction":along/total},"to":f.port(2,"in"),"waypoints":[[97.5,282.5]],"y":282.5}));
    assert_eq!(f.path(&branch["handle"])[0], [62.5, 282.5]);
    assert_eq!(dot_count(&f), 1);
    let at = Vec2::new(62.5, 282.5);
    assert!(
        f.s.doc()
            .unwrap()
            .model
            .iter()
            .filter_map(|e| if let EntityKind::Insert(i) = &e.kind { f.s.doc().unwrap().block(&i.block) } else { None })
            .any(|b| b.entities.iter().any(|e| matches!(&e.kind,EntityKind::Solid(s) if s.corners[0].xy().near(at,1e-6))))
    );
    f.s.execute("electrical.unifilar.link.delete", &json!({"handle":branch["handle"],"backup_dir":f.root})).unwrap();
    assert_eq!(dot_count(&f), 0);
    f.s.execute("undo", &json!({})).unwrap();
    assert_eq!(dot_count(&f), 1);
}
