use std::path::{Path, PathBuf};

use editor_core::{EditorDocument, EditorError};
use eframe::egui;
use relief_core::CanonicalView;

use crate::{
    canvas::CanvasPairState,
    layout::{CANONICAL_SOURCE_ORDER, SourceCardLayout},
};

const PNG_FILTER: &[&str] = &["png"];

pub fn add_next_source(document: &mut EditorDocument) -> Result<CanonicalView, EditorError> {
    let view = CANONICAL_SOURCE_ORDER
        .into_iter()
        .find(|view| document.source(*view).is_none())
        .ok_or(EditorError::SourceLimit)?;
    document.add_source(view)?;
    Ok(view)
}

pub fn replace_source_from_png(
    document: &mut EditorDocument,
    view: CanonicalView,
    path: impl AsRef<Path>,
) -> Result<(), EditorError> {
    document.import_source_png(view, path)
}

pub fn remove_source(
    document: &mut EditorDocument,
    view: CanonicalView,
) -> Result<(), EditorError> {
    document.remove_source(view)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CardHeader {
    pub label: &'static str,
}

pub fn card_header(document: &EditorDocument, view: CanonicalView) -> Option<CardHeader> {
    document.source(view)?;
    let label = if document.source(editor_core::opposite(view)).is_none() {
        match view {
            CanonicalView::Front => "Front → Back",
            CanonicalView::Right => "Right → Left",
            CanonicalView::Top => "Top → Bottom",
            CanonicalView::Back => "Back → Front",
            CanonicalView::Left => "Left → Right",
            CanonicalView::Bottom => "Bottom → Top",
        }
    } else {
        view_label(view)
    };
    Some(CardHeader { label })
}

pub struct SourceGridState {
    cards: [CanvasPairState; 6],
    error: Option<String>,
}

pub(crate) struct SourceGridOutput {
    #[cfg(test)]
    pub observation: SourceGridObservation,
}

#[cfg(test)]
pub(crate) struct SourceGridObservation {
    pub cards: Vec<SourceCardObservation>,
    pub add_button: Option<egui::Rect>,
}

#[cfg(test)]
pub(crate) struct SourceCardObservation {
    pub view: CanonicalView,
    pub column: usize,
    pub row: usize,
    pub card: egui::Rect,
    pub color: egui::Rect,
    pub depth: egui::Rect,
    pub header: egui::Rect,
    pub label: egui::Rect,
    pub menu: egui::Rect,
}

struct AuthoredCardOutput {
    #[cfg(test)]
    canvas: crate::canvas::CanvasPairOutput,
    #[cfg(test)]
    header: egui::Rect,
    #[cfg(test)]
    label: egui::Rect,
    #[cfg(test)]
    menu: egui::Rect,
}

impl Default for SourceGridState {
    fn default() -> Self {
        Self {
            cards: std::array::from_fn(|_| CanvasPairState::default()),
            error: None,
        }
    }
}

impl SourceGridState {
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        document: &mut EditorDocument,
        layouts: &[SourceCardLayout],
        add_button: Option<crate::layout::Rect>,
        origin: egui::Pos2,
    ) -> SourceGridOutput {
        #[cfg(test)]
        let mut observed_cards = Vec::new();
        let authored_views = CANONICAL_SOURCE_ORDER
            .into_iter()
            .filter(|view| document.source(*view).is_some())
            .collect::<Vec<_>>();
        assert_eq!(authored_views.len(), layouts.len());
        for (layout, view) in layouts.iter().zip(authored_views) {
            let _card_rect = to_egui(layout.card, origin);
            let state_index = CANONICAL_SOURCE_ORDER
                .iter()
                .position(|candidate| *candidate == view)
                .expect("authored views come from canonical order");
            let authored = self.show_authored_card(ui, document, state_index, view, layout, origin);
            #[cfg(test)]
            observed_cards.push(SourceCardObservation {
                view,
                column: layout.column,
                row: layout.row,
                card: _card_rect,
                color: authored.canvas.observation.color,
                depth: authored.canvas.observation.depth,
                header: authored.header,
                label: authored.label,
                menu: authored.menu,
            });
            #[cfg(not(test))]
            {
                let _ = authored;
            }
        }

        let add_button = add_button.map(|rect| to_egui(rect, origin));
        if let Some(rect) = add_button
            && ui.put(rect, egui::Button::new("Add Sprite")).clicked()
        {
            self.capture(add_next_source(document).map(|_| ()));
        }

        if let Some(error) = &self.error {
            ui.painter().text(
                origin + egui::vec2(layouts[0].card.left(), layouts[0].card.bottom() + 2.0),
                egui::Align2::LEFT_TOP,
                error,
                egui::FontId::monospace(10.0),
                egui::Color32::LIGHT_RED,
            );
        }
        SourceGridOutput {
            #[cfg(test)]
            observation: SourceGridObservation {
                cards: observed_cards,
                add_button,
            },
        }
    }

    fn show_authored_card(
        &mut self,
        ui: &mut egui::Ui,
        document: &mut EditorDocument,
        index: usize,
        view: CanonicalView,
        layout: &SourceCardLayout,
        origin: egui::Pos2,
    ) -> AuthoredCardOutput {
        let card_rect = to_egui(layout.card, origin);
        ui.painter()
            .rect_filled(card_rect, 4.0, egui::Color32::from_gray(36));
        let header =
            card_header(document, view).expect("authored slot modes always have a card header");
        let header_rect = egui::Rect::from_min_max(
            card_rect.min + egui::vec2(6.0, 2.0),
            egui::pos2(card_rect.right() - 4.0, to_egui(layout.color, origin).top()),
        );
        let menu_rect = egui::Rect::from_min_size(
            egui::pos2(header_rect.right() - 24.0, header_rect.top()),
            egui::vec2(24.0, header_rect.height()),
        );
        let label_area = egui::Rect::from_min_max(
            header_rect.min,
            egui::pos2(menu_rect.left() - 4.0, header_rect.bottom()),
        );
        let _label_rect = ui.painter().text(
            label_area.left_center(),
            egui::Align2::LEFT_CENTER,
            header.label,
            egui::FontId::monospace(9.0),
            egui::Color32::LIGHT_GRAY,
        );
        let _menu = ui
            .scope_builder(egui::UiBuilder::new().max_rect(menu_rect), |ui| {
                ui.menu_button("⋮", |ui| {
                    if ui.button("Import PNG…").clicked() {
                        ui.close();
                        if let Some(path) = pick_source_png() {
                            self.capture(replace_source_from_png(document, view, path));
                        }
                    }
                    if ui.button("Remove").clicked() {
                        ui.close();
                        self.capture(remove_source(document, view));
                    }
                })
            })
            .inner;

        let color_rect = to_egui(layout.color, origin);
        let depth_rect = to_egui(layout.depth, origin);
        let _canvas = self.cards[index].show_pair(ui, document, view, color_rect, depth_rect);
        AuthoredCardOutput {
            #[cfg(test)]
            canvas: _canvas,
            #[cfg(test)]
            header: header_rect,
            #[cfg(test)]
            label: _label_rect,
            #[cfg(test)]
            menu: _menu.response.rect,
        }
    }

    fn capture(&mut self, result: Result<(), EditorError>) {
        match result {
            Ok(()) => self.error = None,
            Err(error) => self.error = Some(error.to_string()),
        }
    }
}

fn pick_source_png() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("PNG image", PNG_FILTER)
        .pick_file()
}

fn to_egui(rect: crate::layout::Rect, origin: egui::Pos2) -> egui::Rect {
    egui::Rect::from_min_max(
        origin + egui::vec2(rect.left(), rect.top()),
        origin + egui::vec2(rect.right(), rect.bottom()),
    )
}

pub const fn view_label(view: CanonicalView) -> &'static str {
    match view {
        CanonicalView::Front => "Front",
        CanonicalView::Right => "Right",
        CanonicalView::Top => "Top",
        CanonicalView::Back => "Back",
        CanonicalView::Left => "Left",
        CanonicalView::Bottom => "Bottom",
    }
}

#[cfg(test)]
mod tests {
    use editor_core::EditorDocument;
    use relief_core::Bounds;

    use super::*;
    use crate::layout::{Size, calculate_layout};

    fn render_grid(
        context: &egui::Context,
        grid: &mut SourceGridState,
        document: &mut EditorDocument,
        layout: &crate::layout::WorkspaceLayout,
    ) -> SourceGridObservation {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 1000.0),
            )),
            ..Default::default()
        };
        let mut observation = None;
        let _ = context.run_ui(input, |ui| {
            observation = Some(
                grid.show(
                    ui,
                    document,
                    &layout.source_cards,
                    layout.add_button,
                    egui::Pos2::ZERO,
                )
                .observation,
            );
        });
        observation.unwrap()
    }

    #[test]
    fn every_fallback_header_renders_inside_its_card_without_touching_the_menu() {
        for view in CANONICAL_SOURCE_ORDER {
            let context = egui::Context::default();
            let mut document = EditorDocument::new(Bounds::new(32, 32, 32).unwrap(), view);
            let layout = calculate_layout(Size::new(1600.0, 1000.0), 1).unwrap();
            let mut grid = SourceGridState::default();
            let observation = render_grid(&context, &mut grid, &mut document, &layout);
            let card = observation
                .cards
                .iter()
                .find(|card| card.view == view)
                .expect("the authored card is rendered");
            assert!(card.card.contains_rect(card.header));
            assert!(card.header.contains_rect(card.label));
            assert!(
                card.header.contains_rect(card.menu),
                "header {:?} does not contain menu {:?}",
                card.header,
                card.menu
            );
            assert!(!card.label.intersects(card.menu));
        }
    }

    #[test]
    fn front_and_top_pack_adjacent_with_one_compact_add_control() {
        let context = egui::Context::default();
        let mut document =
            EditorDocument::new(Bounds::new(32, 32, 32).unwrap(), CanonicalView::Front);
        document.add_source(CanonicalView::Top).unwrap();
        let layout = calculate_layout(Size::new(1600.0, 1000.0), 2).unwrap();
        let mut grid = SourceGridState::default();
        let observation = render_grid(&context, &mut grid, &mut document, &layout);

        assert_eq!(
            observation
                .cards
                .iter()
                .map(|card| (card.view, card.column, card.row))
                .collect::<Vec<_>>(),
            [(CanonicalView::Front, 0, 0), (CanonicalView::Top, 1, 0),]
        );
        let add = observation
            .add_button
            .expect("fewer than six sources can add");
        assert_eq!(add.height(), crate::layout::SOURCE_ACTION_HEIGHT);
        for card in observation.cards {
            assert!(!add.intersects(card.card));
        }
    }
}
