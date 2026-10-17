//! Category navigation and a virtualized contact sheet sharing the composer's vector cache.
use super::State;
use crate::{elettrico, theme::Tokens};

pub(super) fn indices(state: &State) -> Vec<usize> {
    let query = state.search.trim().to_lowercase();
    let mut indices: Vec<_> = state
        .draft
        .components
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            (!state.insertion || c.standalone)
                && c.category.starts_with(&state.browsing)
                && (!state.favorites_only || state.favorites.contains(&c.id))
                && (query.is_empty()
                    || c.name.to_lowercase().contains(&query)
                    || c.block.to_lowercase().contains(&query)
                    || c.category.join(" / ").to_lowercase().contains(&query))
        })
        .map(|(i, _)| i)
        .collect();
    indices.sort_by_key(|i| state.draft.components.get(*i).map(|c| (c.name.to_lowercase(), c.id.clone())));
    indices
}

fn browse(state: &mut State, path: Vec<String>, favorites: bool) {
    state.browsing = path;
    state.favorites_only = favorites;
    state.selected = None;
    state.delete_confirmation = false;
}

pub(super) fn sidebar(ui: &mut egui::Ui, state: &mut State) {
    ui.heading("Categorie");
    if ui.selectable_label(state.browsing.is_empty() && !state.favorites_only, "Tutti i simboli").clicked() {
        browse(state, vec![], false);
    }
    if ui.selectable_label(state.favorites_only, "★ Preferiti").clicked() {
        browse(state, vec![], true);
    }
    ui.separator();
    egui::ScrollArea::vertical().id_salt("catalog.categories").max_height(440.0).show(ui, |ui| tree(ui, state, &[]));
}

fn tree(ui: &mut egui::Ui, state: &mut State, prefix: &[String]) {
    let children: std::collections::BTreeSet<_> = state
        .draft
        .components
        .iter()
        .filter(|c| (!state.insertion || c.standalone) && c.category.starts_with(prefix))
        .filter_map(|c| c.category.get(prefix.len()).cloned())
        .collect();
    for child in children {
        let mut path = prefix.to_vec();
        path.push(child.clone());
        let count = state.draft.components.iter().filter(|c| (!state.insertion || c.standalone) && c.category.starts_with(&path)).count();
        let id = ui.make_persistent_id(("catalog.category", &path));
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
            .show_header(ui, |ui| {
                if ui.selectable_label(!state.favorites_only && state.browsing == path, format!("{child} ({count})")).clicked() {
                    browse(state, path.clone(), false);
                }
            })
            .body(|ui| tree(ui, state, &path));
    }
}

pub(super) fn title(state: &State) -> String {
    if state.favorites_only {
        "Preferiti".into()
    } else if state.browsing.is_empty() {
        "Tutti i simboli".into()
    } else {
        state.browsing.join(" / ")
    }
}

pub(super) fn sheet(ui: &mut egui::Ui, state: &mut State, live: &elettrico::State) {
    ui.heading(title(state));
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut state.search).hint_text("Cerca in questa categoria...").desired_width(260.0));
        state.thumbnail = state.thumbnail.clamp(100.0, 200.0);
        ui.add(egui::Slider::new(&mut state.thumbnail, 100.0..=200.0).text("Miniature").show_value(false));
    });
    let indices = indices(state);
    ui.label(format!(
        "{} simboli · {}",
        indices.len(),
        if state.insertion { "clicca un'anteprima per inserirla nel disegno" } else { "clicca un'anteprima per modificare gli attributi" }
    ));
    if indices.is_empty() {
        ui.label("Nessun simbolo corrisponde alla ricerca.");
        return;
    }
    let spacing = ui.spacing().item_spacing;
    let columns = ((ui.available_width() + spacing.x) / (state.thumbnail + spacing.x)).floor().clamp(1.0, 8.0) as usize;
    let height = state.thumbnail + 48.0;
    // Stable per category/query/size, and unchanged while visiting a detail: Back restores position.
    let id = egui::Id::new(("catalog.gallery", &state.browsing, state.favorites_only, &state.search, state.thumbnail.to_bits()));
    egui::ScrollArea::vertical().id_salt(id).max_height(440.0).auto_shrink([false, false]).show_rows(
        ui,
        height,
        indices.len().div_ceil(columns),
        |ui, rows| {
            for chunk in indices.chunks(columns).skip(rows.start).take(rows.end.saturating_sub(rows.start)) {
                ui.horizontal(|ui| {
                    for index in chunk {
                        let Some(c) = state.draft.components.get(*index) else { continue };
                        let (rect, response) = ui.allocate_exact_size(egui::vec2(state.thumbnail, height), egui::Sense::click());
                        if ui.is_rect_visible(rect) {
                            let t = Tokens::get();
                            let painter = ui.painter_at(rect);
                            painter.rect_filled(rect, 4.0, if response.hovered() { t.control_hover } else { t.control });
                            let art =
                                egui::Rect::from_min_size(rect.min + egui::vec2(4.0, 4.0), egui::vec2(rect.width() - 8.0, state.thumbnail - 8.0));
                            let list = live
                                .catalog
                                .components
                                .iter()
                                .position(|v| v.block == c.block)
                                .and_then(|i| live.library.as_ref().and_then(|l| l.symbol(i)));
                            super::preview::paint_symbol(ui, art, list);
                            let text_rect = egui::Rect::from_min_max(
                                egui::pos2(rect.left() + 6.0, rect.top() + state.thumbnail + 4.0),
                                rect.right_bottom() - egui::vec2(6.0, 4.0),
                            );
                            let label = format!("{}{}", if state.favorites.contains(&c.id) { "★ " } else { "" }, c.name);
                            let galley = painter.layout(label, egui::FontId::proportional(13.0), t.text, text_rect.width());
                            painter.with_clip_rect(text_rect).galley(text_rect.left_top(), galley, t.text);
                            painter.rect_stroke(
                                rect,
                                4.0,
                                egui::Stroke::new(1.0, if response.hovered() { t.accent } else { t.border }),
                                egui::StrokeKind::Inside,
                            );
                        }
                        let clicked = response.clicked();
                        response.on_hover_text(&c.name);
                        if clicked {
                            state.select(*index);
                        }
                    }
                });
            }
        },
    );
}
