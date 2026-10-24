//! Read-only symbol previews from an external DXF library. No catalog artwork is embedded.

use std::sync::Arc;

use cadcraft_color::Rgb;
use cadcraft_engine::doc::Drawing;
use cadcraft_geom::Bounds2;
use cadcraft_render::DisplayList;
use egui::{Color32, Rect, Stroke, pos2, vec2};

use crate::canvas::Xf;

use super::{State, catalog::Component};

#[cfg(not(target_arch = "wasm32"))]
const MAX_LIBRARY_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone)]
pub(super) struct Library {
    pub name: String,
    pub source_path: String,
    symbols: Vec<Option<Arc<DisplayList>>>,
}

impl Library {
    pub fn symbol(&self, index: usize) -> Option<&DisplayList> {
        self.symbols.get(index).and_then(Option::as_deref)
    }
    pub fn remap(&self, old: &[Component], new: &[Component]) -> Self {
        Self {
            name: self.name.clone(),
            source_path: self.source_path.clone(),
            symbols: new.iter().map(|c| old.iter().position(|v| v.block == c.block).and_then(|i| self.symbols.get(i)).cloned().flatten()).collect(),
        }
    }
    pub fn available(&self) -> usize {
        self.symbols.iter().filter(|symbol| symbol.is_some()).count()
    }

    pub fn load_project(session: &cadcraft_engine::Session, path: &str, components: &[Component]) -> Result<Self, String> {
        if let Some(bytes) = cadcraft_engine::project::resource(session, path, 32 * 1024 * 1024).map_err(|e| e.to_string())? {
            let hooks = cadcraft_engine::cmd::file::io().ok_or("Lettore DXF assente.")?;
            let drawing = (hooks.read)(&bytes, "drawing.dxf")?;
            let mut library = Self::from_drawing(&drawing, "Progetto".into(), components);
            library.source_path = path.into();
            return Ok(library);
        }
        Self::load(path, components)
    }
    pub fn load(path: &str, components: &[Component]) -> Result<Self, String> {
        let bytes = read_library(path)?;
        let hooks = cadcraft_engine::cmd::file::io().ok_or("Lettore DXF non disponibile.")?;
        let drawing = (hooks.read)(&bytes, path)?;
        let name = std::path::Path::new(path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let mut library = Self::from_drawing(&drawing, name, components);
        library.source_path = path.to_string();
        if library.available() == 0 && !components.is_empty() {
            return Err("Nessun simbolo del catalogo trovato: verifica i nomi dei blocchi e la libreria DXF scelta.".into());
        }
        Ok(library)
    }

    fn from_drawing(drawing: &Drawing, name: String, components: &[Component]) -> Self {
        let symbols = components
            .iter()
            .map(|component| {
                let block = drawing.block(&component.block)?;
                let rough = cadcraft_render::build_entities(drawing, block.entities.iter().map(|entity| entity.as_ref()), &Default::default());
                let size = rough.bounds.width().max(rough.bounds.height());
                if rough.bounds.is_empty() || !size.is_finite() || !rough.bounds.center().is_finite() || rough.prims.is_empty() {
                    return None;
                }
                let options = cadcraft_render::Options { tolerance: (size / 800.0).max(1e-9), ..Default::default() };
                let mut list = cadcraft_render::build_entities(drawing, block.entities.iter().map(|entity| entity.as_ref()), &options);
                for primitive in &mut list.prims {
                    primitive.color = Rgb(255, 255, 255);
                    primitive.aci7 = true;
                }
                Some(Arc::new(list))
            })
            .collect();
        Self { name, source_path: String::new(), symbols }
    }
}

fn read_library(_path: &str) -> Result<Vec<u8>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::io::Read;
        if _path.len() > 32768 {
            return Err("Percorso della libreria troppo lungo.".into());
        }
        let file = std::fs::File::open(_path).map_err(|error| format!("Libreria simboli: {error}"))?;
        if file.metadata().map_err(|error| error.to_string())?.len() > MAX_LIBRARY_BYTES {
            return Err("Libreria troppo grande: limite 16 MB.".into());
        }
        let mut bytes = Vec::new();
        file.take(MAX_LIBRARY_BYTES + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
        if bytes.len() as u64 > MAX_LIBRARY_BYTES {
            return Err("Libreria troppo grande: limite 16 MB.".into());
        }
        Ok(bytes)
    }
    #[cfg(target_arch = "wasm32")]
    Err("La libreria locale è disponibile nella versione desktop.".into())
}

/// Fit the actual symbol bounds, including text, into the component's own occupied slots.
fn fit_symbol(bounds: Bounds2, slot: Rect, unit: f32) -> Option<Xf> {
    let width = bounds.width();
    let height = bounds.height();
    if bounds.is_empty() || !width.is_finite() || !height.is_finite() || !bounds.center().is_finite() {
        return None;
    }
    let scale = (f64::from(slot.width() - 0.65 * unit) / width.max(1e-9)).min(f64::from(slot.height() - 1.1 * unit) / height.max(1e-9));
    (scale.is_finite() && scale > 0.0).then(|| Xf { rect: slot, center: bounds.center(), scale })
}

pub(super) fn show_symbol(ui: &mut egui::Ui, list: Option<&DisplayList>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(100.0), 150.0), egui::Sense::hover());
    paint_symbol(ui, rect, list);
}

pub(super) fn paint_symbol(ui: &egui::Ui, rect: Rect, list: Option<&DisplayList>) {
    let painter = ui.painter_at(rect);
    let background = crate::theme::Tokens::DARK.canvas;
    painter.rect_filled(rect, 3.0, background);
    let inner = rect.shrink(18.0);
    if let Some(list) = list {
        let bounds = list.bounds;
        let scale = (f64::from(inner.width()) / bounds.width().max(1e-9)).min(f64::from(inner.height()) / bounds.height().max(1e-9));
        if !bounds.is_empty() && bounds.center().is_finite() && scale.is_finite() && scale > 0.0 {
            let transform = Xf { rect: inner, center: bounds.center(), scale };
            crate::canvas::draw_list(&painter, &transform, list, Rgb(background.r(), background.g(), background.b()), false);
            return;
        }
    }
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, "Anteprima non disponibile", crate::theme::small(), Color32::WHITE);
}

pub(super) fn show_edit(ui: &mut egui::Ui, state: &State, side: u8) -> Option<usize> {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(100.0), 200.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let background = crate::theme::Tokens::DARK.canvas;
    painter.rect_filled(rect, 3.0, background);
    let capacity = f32::from(state.capacity.clamp(1, 7));
    let label = if state.height.len() <= 32 {
        state.height.trim().replace(',', ".").parse::<f64>().ok().filter(|value| value.is_finite() && (0.0..=10000.0).contains(value))
    } else {
        None
    };
    let extra_height = if label.is_some() && side < 2 { 3.0 } else { 0.0 };
    let extra_width = if label.is_some() && side >= 2 { 8.0 } else { 0.0 };
    let inner = rect.shrink(16.0);
    let unit = (inner.width() / (capacity * 3.0 + extra_width)).min(inner.height() / (5.0 + extra_height));
    let box_size = vec2(capacity * 3.0 * unit, 5.0 * unit);
    let box_center = inner.center()
        + vec2(if side == 2 { -extra_width } else { extra_width }, if side == 1 { -extra_height } else { extra_height }) * (unit / 2.0);
    let box_rect = Rect::from_center_size(box_center, box_size);
    let stroke = Stroke::new(1.5, Color32::WHITE);
    painter.rect_stroke(box_rect, 0.0, stroke, egui::StrokeKind::Inside);
    if let Some(height) = label {
        let (point, align) = match side {
            1 => (pos2(box_center.x, box_rect.bottom() + unit), egui::Align2::CENTER_TOP),
            2 => (pos2(box_rect.right() + unit, box_center.y), egui::Align2::LEFT_CENTER),
            3 => (pos2(box_rect.left() - unit, box_center.y), egui::Align2::RIGHT_CENTER),
            _ => (pos2(box_center.x, box_rect.top() - unit), egui::Align2::CENTER_BOTTOM),
        };
        painter.text(point, align, format!("H{height}"), egui::FontId::proportional(1.8 * unit), Color32::WHITE);
    }
    let mut position = 0.0;
    let mut selected = None;
    for (item_position, index) in state.items.iter().enumerate() {
        let Some(component) = state.catalog.components.get(*index) else { continue };
        let span = f32::from(component.modules) * 3.0 * unit;
        let slot = Rect::from_min_size(pos2(box_rect.left() + position, box_rect.top()), vec2(span, box_rect.height()));
        if ui.interact(slot, ui.id().with(("electrical-preview", item_position)), egui::Sense::click()).clicked() {
            selected = Some(item_position);
        }
        if position > 0.0 {
            painter.line_segment([slot.left_top(), slot.left_bottom()], stroke);
        }
        let symbol = state.library.as_ref().and_then(|library| library.symbols.get(*index)).and_then(Option::as_deref);
        match symbol.and_then(|list| fit_symbol(list.bounds, slot, unit).map(|transform| (list, transform))) {
            Some((list, transform)) => {
                crate::canvas::draw_list(&painter, &transform, list, Rgb(background.r(), background.g(), background.b()), false);
            }
            None => {
                painter.text(slot.center(), egui::Align2::CENTER_CENTER, "Simbolo\nassente", crate::theme::small(), Color32::WHITE);
            }
        }
        position += span;
    }
    if position > 0.0 && position < box_rect.width() - 0.01 {
        let x = box_rect.left() + position;
        painter.line_segment([pos2(x, box_rect.top()), pos2(x, box_rect.bottom())], stroke);
    }
    selected
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadcraft_geom::Vec2;
    use serde_json::json;

    #[test]
    fn external_block_geometry_is_cached_without_touching_the_active_drawing() {
        let mut session = cadcraft_engine::Session::new();
        let line = session.execute("line", &json!({"points": [[100, 200], [140, 220]]})).unwrap();
        session.execute("block", &json!({"name": "SyntheticSwitch", "base": [0, 0], "handles": line["handles"]})).unwrap();
        let before = session.doc().unwrap().clone();
        let component = Component {
            id: "switch".into(),
            name: "Switch".into(),
            block: "SyntheticSwitch".into(),
            modules: 1,
            category: vec![],
            ..Default::default()
        };
        let library = Library::from_drawing(&before, "synthetic".into(), &[component]);
        assert_eq!(library.available(), 1);
        assert_eq!(session.doc().unwrap().model.len(), before.model.len());
        let symbol = library.symbols[0].as_ref().unwrap();
        assert_eq!(symbol.bounds.center(), Vec2::new(120.0, 210.0));
        assert!(symbol.prims.iter().all(|primitive| primitive.color == Rgb(255, 255, 255)));
        let slot = Rect::from_min_size(pos2(30.0, 40.0), vec2(120.0, 200.0));
        let transform = fit_symbol(symbol.bounds, slot, 40.0).unwrap();
        assert_eq!(transform.center, symbol.bounds.center());
        assert!(symbol.bounds.width() * transform.scale < f64::from(slot.width()));
        assert!(symbol.bounds.height() * transform.scale < f64::from(slot.height()));
    }
}
