use std::path::{Path, PathBuf};

use editor_core::{EditorDocument, EditorError};
use eframe::egui;
use relief_core::CanonicalView;

use crate::{
    canvas::{CanvasKind, CanvasPairState},
    layout::{CANONICAL_SOURCE_ORDER, SourceCardLayout},
};

const PNG_FILTER: &[&str] = &["png"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotMode {
    Authored,
    AddSprite,
    Hidden,
}

pub fn slot_modes(document: &EditorDocument) -> [SlotMode; 6] {
    let next_empty = CANONICAL_SOURCE_ORDER
        .into_iter()
        .find(|view| document.source(*view).is_none());
    CANONICAL_SOURCE_ORDER.map(|view| {
        if document.source(view).is_some() {
            SlotMode::Authored
        } else if Some(view) == next_empty {
            SlotMode::AddSprite
        } else {
            SlotMode::Hidden
        }
    })
}

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
    pub title: &'static str,
    pub assignment: &'static str,
}

pub fn card_header(document: &EditorDocument, view: CanonicalView) -> Option<CardHeader> {
    document.source(view)?;
    let assignment = if document.source(editor_core::opposite(view)).is_none() {
        match editor_core::opposite(view) {
            CanonicalView::Front => "Fallback for Front",
            CanonicalView::Right => "Fallback for Right",
            CanonicalView::Top => "Fallback for Top",
            CanonicalView::Back => "Fallback for Back",
            CanonicalView::Left => "Fallback for Left",
            CanonicalView::Bottom => "Fallback for Bottom",
        }
    } else {
        "Authored only"
    };
    Some(CardHeader {
        title: view_label(view),
        assignment,
    })
}

pub struct SourceGridState {
    cards: [CanvasPairState; 6],
    error: Option<String>,
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
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        document: &mut EditorDocument,
        layouts: &[SourceCardLayout; 6],
        origin: egui::Pos2,
    ) {
        let modes = slot_modes(document);
        for (index, layout) in layouts.iter().enumerate() {
            let card_rect = to_egui(layout.card, origin);
            match modes[index] {
                SlotMode::Authored => {
                    self.show_authored_card(ui, document, index, layout, card_rect, origin);
                }
                SlotMode::AddSprite => {
                    ui.painter()
                        .rect_filled(card_rect, 4.0, egui::Color32::from_gray(36));
                    let button =
                        egui::Button::new(format!("Add Sprite\n{}", view_label(layout.view)));
                    if ui.put(card_rect.shrink(12.0), button).clicked() {
                        self.capture(add_next_source(document).map(|_| ()));
                    }
                }
                SlotMode::Hidden => {}
            }
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
    }

    fn show_authored_card(
        &mut self,
        ui: &mut egui::Ui,
        document: &mut EditorDocument,
        index: usize,
        layout: &SourceCardLayout,
        card_rect: egui::Rect,
        origin: egui::Pos2,
    ) {
        ui.painter()
            .rect_filled(card_rect, 4.0, egui::Color32::from_gray(36));
        let header = card_header(document, layout.view)
            .expect("authored slot modes always have a card header");
        let header_rect = egui::Rect::from_min_max(
            card_rect.min + egui::vec2(6.0, 2.0),
            egui::pos2(card_rect.right() - 4.0, to_egui(layout.color, origin).top()),
        );
        ui.painter().text(
            header_rect.left_center(),
            egui::Align2::LEFT_CENTER,
            format!("{} · {}", header.title, header.assignment),
            egui::FontId::monospace(9.0),
            egui::Color32::LIGHT_GRAY,
        );

        let menu_rect = egui::Rect::from_min_size(
            egui::pos2(header_rect.right() - 16.0, header_rect.top()),
            egui::vec2(16.0, header_rect.height()),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(menu_rect), |ui| {
            ui.menu_button("⋮", |ui| {
                if ui.button("Import PNG…").clicked() {
                    ui.close();
                    if let Some(path) = pick_source_png() {
                        self.capture(replace_source_from_png(document, layout.view, path));
                    }
                }
                if ui.button("Remove").clicked() {
                    ui.close();
                    self.capture(remove_source(document, layout.view));
                }
            });
        });

        let color_rect = to_egui(layout.color, origin);
        ui.scope_builder(egui::UiBuilder::new().max_rect(color_rect), |ui| {
            self.cards[index].show_canvas(
                ui,
                document,
                layout.view,
                CanvasKind::Color,
                color_rect.size(),
            );
        });
        let depth_rect = to_egui(layout.depth, origin);
        ui.scope_builder(egui::UiBuilder::new().max_rect(depth_rect), |ui| {
            self.cards[index].show_canvas(
                ui,
                document,
                layout.view,
                CanvasKind::Depth,
                depth_rect.size(),
            );
        });
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
