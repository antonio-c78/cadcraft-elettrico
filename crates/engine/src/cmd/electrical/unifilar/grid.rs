//! Per-sheet defaults and native overrides. UI controls remain the ordinary CAD controls.
use super::*;
use crate::{DocState, Settings};
use cadcraft_doc::HVal;
const APP: &str = "CC_UNIFILARE_DRAFTING";
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct Preferences {
    pub grid: Vec2,
    pub snap: Vec2,
    pub origin: Vec2,
    pub angle: f64,
    pub major: u32,
    pub visible: bool,
    pub enabled: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct Grid {
    /// Immutable layout basis: changing drafting settings must not renumber columns.
    pub origin: Vec2,
    pub spacing: Vec2,
    pub(crate) owner: cadcraft_doc::Handle,
    pub(crate) preferences: Preferences,
    pub(crate) customized: bool,
}
impl Grid {
    pub(super) fn new(d: &cadcraft_doc::Drawing, owner: cadcraft_doc::Handle, origin: Vec2, spacing: Vec2) -> Self {
        let defaults = Preferences { grid: spacing, snap: Vec2::new(spacing.x, 12.5), origin, angle: 0., major: 5, visible: true, enabled: true };
        let stored = d
            .entity(owner)
            .and_then(|e| placement::data(e, APP).ok().flatten())
            .filter(|v| v["version"] == 1)
            .and_then(|v| serde_json::from_value::<Preferences>(v["settings"].clone()).ok())
            .filter(Preferences::valid);
        Self { origin, spacing, owner, preferences: stored.unwrap_or(defaults), customized: stored.is_some() }
    }
    pub(crate) fn unsaved(&self, saved: &cadcraft_doc::Drawing) -> bool {
        self.customized && self.preferences != Self::new(saved, self.owner, self.origin, self.spacing).preferences
    }
}
impl Preferences {
    fn valid(&self) -> bool {
        self.grid.is_finite()
            && self.grid.x >= 0.
            && self.grid.y >= 0.
            && self.grid.x < 1e12
            && self.grid.y < 1e12
            && self.snap.is_finite()
            && self.snap.x > 0.
            && self.snap.y > 0.
            && self.snap.x < 1e12
            && self.snap.y < 1e12
            && self.origin.is_finite()
            && self.origin.x.abs() < 1e12
            && self.origin.y.abs() < 1e12
            && self.angle.is_finite()
            && self.angle.abs() < 1e12
            && (1..=100).contains(&self.major)
    }
    pub(crate) fn apply(&self, settings: &mut Settings) {
        settings.gridunit = self.grid;
        settings.snapunit = self.snap;
        settings.gridmajor = self.major;
        settings.gridmode = self.visible;
        settings.snapmode = self.enabled;
    }
}
pub(crate) fn active(s: &Session) -> Option<Grid> {
    s.state().ok().filter(|st| s.space() == Space::Model && st.block_edit.is_none()).and_then(|st| st.unifilar_grid)
}
/// Called only with values validated by the ordinary native variable parser.
pub(crate) fn set(s: &mut Session, name: &str, value: HVal) -> Result<()> {
    let st = s.state_mut()?;
    let Some(g) = &mut st.unifilar_grid else { return Ok(()) };
    let mut next = g.preferences;
    match (name, &value) {
        ("GRIDUNIT", HVal::Point(v)) => next.grid = v.xy(),
        ("SNAPUNIT", HVal::Point(v)) => next.snap = v.xy(),
        ("SNAPBASE", HVal::Point(v)) => next.origin = v.xy(),
        ("SNAPANG", HVal::Real(v)) => next.angle = *v,
        ("GRIDMAJOR", HVal::Int(v)) => next.major = u32::try_from(*v).map_err(|_| bad("setvar", "Invalid GRIDMAJOR"))?,
        ("GRIDMODE", HVal::Int(v)) => next.visible = *v != 0,
        ("SNAPMODE", HVal::Int(v)) => next.enabled = *v != 0,
        _ => return Err(bad("setvar", "Invalid drafting variable")),
    }
    if !next.valid() {
        return Err(bad("setvar", "Invalid drafting value"));
    }
    g.preferences = next;
    g.customized = true;
    Ok(())
}
fn sync_state(st: &mut DocState) -> Result<()> {
    let Some(g) = st.unifilar_grid.filter(|g| g.customized && st.block_edit.is_none()) else { return Ok(()) };
    let data = json!({"version":1,"settings":g.preferences});
    if st.doc.entity(g.owner).and_then(|e| placement::data(e, APP).ok().flatten()).as_ref() == Some(&data) {
        return Ok(());
    }
    let mut drawing = st.doc.as_ref().clone();
    let p = g.preferences;
    for (name, value) in [
        ("GRIDUNIT", HVal::Point(p.grid.to3(0.))),
        ("SNAPUNIT", HVal::Point(p.snap.to3(0.))),
        ("SNAPBASE", HVal::Point(p.origin.to3(0.))),
        ("SNAPANG", HVal::Real(p.angle)),
        ("GRIDMAJOR", HVal::Int(i64::from(p.major))),
        ("GRIDMODE", HVal::Int(i64::from(p.visible))),
        ("SNAPMODE", HVal::Int(i64::from(p.enabled))),
    ] {
        drawing.header.set(name, value);
    }
    drawing.modify_entity(g.owner, |e| placement::tags(&mut e.common, APP, &data))?;
    st.doc = Arc::new(drawing);
    Ok(())
}
/// Transparent F9/settings changes cannot replace a geometry command's baseline.
/// They are persisted after that command finishes (including Escape), or before save.
pub(crate) fn sync(s: &mut Session) -> Result<()> {
    if s.running.is_some() || s.suspended.is_some() {
        return Ok(());
    }
    for st in &mut s.docs {
        sync_state(st)?;
    }
    if let Some(p) = &mut s.project {
        for st in &mut p.closed {
            sync_state(st)?;
        }
    }
    Ok(())
}
