//! Read-only symbol previews from an external DXF library. No catalog artwork is embedded.

use std::sync::Arc;

use cadcraft_color::Rgb;
use cadcraft_engine::doc::Drawing;
use cadcraft_geom::Bounds2;
use cadcraft_render::DisplayList;
use egui::{Color32, Rect, Stroke, pos2, vec2};

use crate::canvas::Xf;

use super::{COMPONENTS, State};

#[cfg(not(target_arch = "wasm32"))]
const MAX_LIBRARY_BYTES: u64 = 16 * 1024 * 1024;

pub(super) struct Library {
    pub name: String,
    symbols: Vec<Option<Arc<DisplayList>>>,
}

impl Library {
    pub fn available(&self) -> usize {
        self.symbols.iter().filter(|symbol| symbol.is_some()).count()
    }

    pub fn load(path: &str) -> Result<Self, String> {
        let bytes = read_library(path)?;
        let hooks = cadcraft_engine::cmd::file::io().ok_or("Lettore DXF non disponibile.")?;
        let drawing = (hooks.read)(&bytes, path)?;
        let name = std::path::Path::new(path).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let library = Self::from_drawing(&drawing, name);
        if library.available() == 0 {
            return Err("La libreria non contiene i simboli del compositore. Scegli simboli-generali.dxf.".into());
        }
        Ok(library)
    }

    fn from_drawing(drawing: &Drawing, name: String) -> Self {
        let symbols = COMPONENTS
            .iter()
            .map(|(_, _, name)| {
                let block = drawing.block(name)?;
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
        Self { name, symbols }
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

pub(super) fn show(ui: &mut egui::Ui, state: &State) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(100.0), 200.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let background = crate::theme::Tokens::DARK.canvas;
    painter.rect_filled(rect, 3.0, background);
    let capacity = f32::from(state.capacity.clamp(1, 6));
    let label = if state.height.len() <= 32 {
        state.height.trim().replace(',', ".").parse::<f64>().ok().filter(|value| value.is_finite() && (0.0..=10000.0).contains(value))
    } else {
        None
    };
    let extra_height = if label.is_some() { 3.0 } else { 0.0 };
    let inner = rect.shrink(16.0);
    let unit = (inner.width() / (capacity * 3.0)).min(inner.height() / (5.0 + extra_height));
    let box_size = vec2(capacity * 3.0 * unit, 5.0 * unit);
    let box_center = inner.center() + vec2(0.0, extra_height * unit / 2.0);
    let box_rect = Rect::from_center_size(box_center, box_size);
    let stroke = Stroke::new(1.5, Color32::WHITE);
    painter.rect_stroke(box_rect, 0.0, stroke, egui::StrokeKind::Inside);
    if let Some(height) = label {
        painter.text(
            pos2(box_center.x, box_rect.top() - unit),
            egui::Align2::CENTER_BOTTOM,
            format!("H{height}"),
            egui::FontId::proportional(1.8 * unit),
            Color32::WHITE,
        );
    }
    let mut position = 0.0;
    for index in &state.items {
        let Some((_, modules, _)) = COMPONENTS.get(*index) else { continue };
        let span = f32::from(*modules) * 3.0 * unit;
        let slot = Rect::from_min_size(pos2(box_rect.left() + position, box_rect.top()), vec2(span, box_rect.height()));
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
        session.execute("block", &json!({"name": COMPONENTS[0].2, "base": [0, 0], "handles": line["handles"]})).unwrap();
        let before = session.doc().unwrap().clone();
        let library = Library::from_drawing(&before, "synthetic".into());
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
