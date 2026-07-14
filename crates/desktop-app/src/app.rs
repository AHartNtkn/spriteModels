use std::path::PathBuf;

use editor_core::EditorDocument;
use eframe::egui;
use relief_core::{Bounds, CanonicalView};

use crate::{
    layout::{self, Rect, Size},
    menu::{MenuAction, PendingDestructiveAction, UnsavedChoice, show_menu_bar},
    model_view::ModelView,
    palette::PaletteState,
    source_grid::SourceGridState,
};

const MODEL_FILTER: &[&str] = &["depthsprite"];

pub struct ShellState {
    document: EditorDocument,
    pending_destructive_action: Option<PendingDestructiveAction>,
    file_error: Option<String>,
    quit_requested: bool,
}

impl ShellState {
    pub fn new(document: EditorDocument) -> Self {
        Self {
            document,
            pending_destructive_action: None,
            file_error: None,
            quit_requested: false,
        }
    }

    pub fn from_startup_path(path: Option<PathBuf>) -> Self {
        let mut state = Self::new(default_document());
        if let Some(path) = path {
            state.complete_destructive(PendingDestructiveAction::Open(path));
        }
        state
    }

    pub fn document(&self) -> &EditorDocument {
        &self.document
    }

    pub fn pending_destructive_action(&self) -> Option<&PendingDestructiveAction> {
        self.pending_destructive_action.as_ref()
    }

    pub fn file_error(&self) -> Option<&str> {
        self.file_error.as_deref()
    }

    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    pub fn request_destructive(&mut self, action: PendingDestructiveAction) {
        if self.document.is_dirty() {
            self.pending_destructive_action = Some(action);
        } else {
            self.complete_destructive(action);
        }
    }

    pub fn resolve_unsaved(&mut self, choice: UnsavedChoice, save_path: Option<PathBuf>) {
        match choice {
            UnsavedChoice::Cancel => {
                self.pending_destructive_action = None;
            }
            UnsavedChoice::Discard => {
                if let Some(action) = self.pending_destructive_action.take() {
                    self.complete_destructive(action);
                }
            }
            UnsavedChoice::Save => {
                if self.save_document(save_path)
                    && let Some(action) = self.pending_destructive_action.take()
                {
                    self.complete_destructive(action);
                }
            }
        }
    }

    pub fn save_document(&mut self, save_path: Option<PathBuf>) -> bool {
        let result = if let Some(path) = save_path {
            self.document.save_as(path)
        } else if self.document.path().is_some() {
            self.document.save()
        } else {
            return false;
        };
        match result {
            Ok(()) => {
                self.file_error = None;
                true
            }
            Err(error) => {
                self.file_error = Some(format!("Could not save the current document: {error}"));
                false
            }
        }
    }

    pub fn undo(&mut self) {
        self.document.undo();
    }

    pub fn redo(&mut self) {
        self.document.redo();
    }

    pub fn dismiss_file_error(&mut self) {
        self.file_error = None;
    }

    fn complete_destructive(&mut self, action: PendingDestructiveAction) {
        match action {
            PendingDestructiveAction::New => {
                self.document = EditorDocument::new(self.document.bounds(), CanonicalView::Front);
                self.file_error = None;
            }
            PendingDestructiveAction::Open(path) => match EditorDocument::open(&path) {
                Ok(document) => {
                    self.document = document;
                    self.file_error = None;
                }
                Err(error) => {
                    self.file_error = Some(format!("Could not open {}: {error}", path.display()));
                }
            },
            PendingDestructiveAction::Quit => {
                self.quit_requested = true;
            }
        }
    }
}

pub struct DepthSpriteApp {
    shell: ShellState,
    palette: PaletteState,
    model_view: ModelView,
    source_grid: SourceGridState,
    #[cfg(test)]
    last_composition: Option<CompositionObservation>,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CompositionStage {
    TopMenu,
    Palette,
    ModelViewport,
    SourceGrid,
    TransientModals,
}

#[cfg(test)]
struct SourceCardObservation {
    column: usize,
    row: usize,
    card: egui::Rect,
    color: egui::Rect,
    depth: egui::Rect,
}

#[cfg(test)]
struct CompositionObservation {
    stages: Vec<CompositionStage>,
    menu: egui::Rect,
    palette: egui::Rect,
    model: egui::Rect,
    sources: egui::Rect,
    source_cards: [SourceCardObservation; layout::SOURCE_SLOT_COUNT],
}

impl DepthSpriteApp {
    pub fn from_startup_path(path: Option<PathBuf>) -> Self {
        let shell = ShellState::from_startup_path(path);
        let palette = PaletteState::new(shell.document());
        Self {
            shell,
            palette,
            model_view: ModelView::default(),
            source_grid: SourceGridState::default(),
            #[cfg(test)]
            last_composition: None,
        }
    }

    pub fn shell(&self) -> &ShellState {
        &self.shell
    }

    fn handle_menu_action(&mut self, action: MenuAction, context: &egui::Context) {
        match action {
            MenuAction::New => self
                .shell
                .request_destructive(PendingDestructiveAction::New),
            MenuAction::Open => {
                if let Some(path) = pick_open_path() {
                    self.shell
                        .request_destructive(PendingDestructiveAction::Open(path));
                }
            }
            MenuAction::Save => {
                let path = self
                    .shell
                    .document()
                    .path()
                    .is_none()
                    .then(pick_save_path)
                    .flatten();
                self.shell.save_document(path);
            }
            MenuAction::SaveAs => {
                if let Some(path) = pick_save_path() {
                    self.shell.save_document(Some(path));
                }
            }
            MenuAction::Quit => self
                .shell
                .request_destructive(PendingDestructiveAction::Quit),
            MenuAction::Undo => self.shell.undo(),
            MenuAction::Redo => self.shell.redo(),
            MenuAction::ResetView => self.model_view.reset(),
        }
        self.finish_quit(context);
    }

    fn show_unsaved_modal(&mut self, context: &egui::Context) {
        if self.shell.pending_destructive_action().is_none() {
            return;
        }
        let mut choice = None;
        egui::Modal::new("unsaved-changes-modal".into()).show(context, |ui| {
            ui.heading("Unsaved changes");
            ui.label("Save changes before continuing?");
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    choice = Some(UnsavedChoice::Save);
                }
                if ui.button("Discard").clicked() {
                    choice = Some(UnsavedChoice::Discard);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(UnsavedChoice::Cancel);
                }
            });
        });

        if let Some(choice) = choice {
            let save_path = (choice == UnsavedChoice::Save
                && self.shell.document().path().is_none())
            .then(pick_save_path)
            .flatten();
            self.shell.resolve_unsaved(choice, save_path);
            self.finish_quit(context);
        }
    }

    fn show_file_error_modal(&mut self, context: &egui::Context) {
        let Some(message) = self.shell.file_error().map(str::to_owned) else {
            return;
        };
        let mut dismiss = false;
        egui::Modal::new("file-error-modal".into()).show(context, |ui| {
            ui.heading("File operation failed");
            ui.label(message);
            if ui.button("Dismiss").clicked() {
                dismiss = true;
            }
        });
        if dismiss {
            self.shell.dismiss_file_error();
        }
    }

    fn finish_quit(&self, context: &egui::Context) {
        if self.shell.quit_requested() {
            context.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for DepthSpriteApp {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = root.ctx().clone();
        if context.input(|input| input.viewport().close_requested()) && !self.shell.quit_requested()
        {
            context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.shell
                .request_destructive(PendingDestructiveAction::Quit);
        }

        let mut selected = None;
        let _menu = egui::Panel::top("top-menu")
            .exact_size(layout::MENU_HEIGHT)
            .show(root, |ui| selected = show_menu_bar(ui));
        if let Some(action) = selected {
            self.handle_menu_action(action, &context);
        }

        let root_rect = root.max_rect();
        #[cfg(test)]
        let mut composition = None;
        egui::CentralPanel::default().show(root, |ui| {
            let layout = layout::calculate_layout(Size::new(root_rect.width(), root_rect.height()))
                .expect("native window must respect the derived minimum size");
            let tools_rect = to_egui(layout.tools, root_rect.min);
            ui.scope_builder(egui::UiBuilder::new().max_rect(tools_rect), |ui| {
                self.palette.show(ui, &mut self.shell.document);
            });
            let model_rect = to_egui(layout.model, root_rect.min);
            if let Err(error) = self.model_view.show(ui, self.shell.document(), model_rect) {
                ui.painter().text(
                    model_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    format!("Preview unavailable: {error}"),
                    egui::FontId::monospace(12.0),
                    egui::Color32::LIGHT_RED,
                );
            }
            self.source_grid.show(
                ui,
                &mut self.shell.document,
                &layout.source_cards,
                root_rect.min,
            );
            #[cfg(test)]
            {
                composition = Some(CompositionObservation {
                    stages: vec![
                        CompositionStage::TopMenu,
                        CompositionStage::Palette,
                        CompositionStage::ModelViewport,
                        CompositionStage::SourceGrid,
                    ],
                    menu: _menu.response.rect,
                    palette: tools_rect,
                    model: model_rect,
                    sources: to_egui(layout.sources, root_rect.min),
                    source_cards: layout.source_cards.map(|card| SourceCardObservation {
                        column: card.column,
                        row: card.row,
                        card: to_egui(card.card, root_rect.min),
                        color: to_egui(card.color, root_rect.min),
                        depth: to_egui(card.depth, root_rect.min),
                    }),
                });
            }
        });

        if self.shell.file_error().is_some() {
            self.show_file_error_modal(&context);
        } else {
            self.show_unsaved_modal(&context);
        }
        #[cfg(test)]
        {
            let mut composition = composition.expect("workspace composition is always recorded");
            composition.stages.push(CompositionStage::TransientModals);
            self.last_composition = Some(composition);
        }
        self.finish_quit(&context);
    }
}

fn default_document() -> EditorDocument {
    EditorDocument::new(
        Bounds::new(32, 32, 32).expect("default model bounds are positive"),
        CanonicalView::Front,
    )
}

fn pick_open_path() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("DepthSprite", MODEL_FILTER)
        .pick_file()
}

fn pick_save_path() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("DepthSprite", MODEL_FILTER)
        .set_file_name("untitled.depthsprite")
        .save_file()
}

fn to_egui(rect: Rect, origin: egui::Pos2) -> egui::Rect {
    egui::Rect::from_min_max(
        origin + egui::vec2(rect.left(), rect.top()),
        origin + egui::vec2(rect.right(), rect.bottom()),
    )
}

#[cfg(test)]
mod tests {
    use eframe::App as _;

    use super::*;

    fn run_frame(
        context: &egui::Context,
        app: &mut DepthSpriteApp,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 1000.0),
            )),
            events,
            ..Default::default()
        };
        let mut frame = eframe::Frame::_new_kittest();
        context.run_ui(input, |ui| app.ui(ui, &mut frame))
    }

    #[test]
    fn real_frame_records_the_complete_semantic_workspace_and_ratios() {
        let context = egui::Context::default();
        let mut app = DepthSpriteApp::from_startup_path(None);

        let output = run_frame(&context, &mut app, Vec::new());
        let composition = app.last_composition.as_ref().unwrap();

        assert_eq!(
            composition.stages,
            [
                CompositionStage::TopMenu,
                CompositionStage::Palette,
                CompositionStage::ModelViewport,
                CompositionStage::SourceGrid,
                CompositionStage::TransientModals,
            ]
        );
        assert_eq!(composition.menu.left(), 0.0);
        assert_eq!(composition.menu.top(), 0.0);
        assert_eq!(composition.menu.right(), 1600.0);
        assert!(composition.palette.height() > composition.palette.width() * 10.0);
        assert!(composition.palette.right() < composition.model.left());
        assert!(composition.model.width() > composition.sources.width());

        for (index, card) in composition.source_cards.iter().enumerate() {
            assert_eq!(card.column, index % layout::SOURCE_COLUMNS);
            assert_eq!(card.row, index / layout::SOURCE_COLUMNS);
            assert!(card.color.bottom() < card.depth.top());
            assert_eq!(card.color.size(), card.depth.size());
            assert!(
                composition.model.width() >= card.color.width() * layout::MODEL_TO_CANVAS_RATIO
            );
            assert!(
                composition.model.height() >= card.color.height() * layout::MODEL_TO_CANVAS_RATIO
            );
        }
        for row in 0..layout::SOURCE_ROWS {
            for column in 1..layout::SOURCE_COLUMNS {
                let previous = &composition.source_cards[row * layout::SOURCE_COLUMNS + column - 1];
                let current = &composition.source_cards[row * layout::SOURCE_COLUMNS + column];
                assert!(previous.card.right() < current.card.left());
            }
        }
        for column in 0..layout::SOURCE_COLUMNS {
            let above = &composition.source_cards[column];
            let below = &composition.source_cards[layout::SOURCE_COLUMNS + column];
            assert!(above.card.bottom() < below.card.top());
        }
        assert!(output.shapes.iter().any(|clipped| {
            let bounds = clipped.shape.visual_bounding_rect();
            bounds.contains_rect(composition.model)
        }));
    }

    #[test]
    fn reset_menu_action_restores_the_integrated_model_camera() {
        let context = egui::Context::default();
        let mut app = DepthSpriteApp::from_startup_path(None);
        run_frame(&context, &mut app, Vec::new());
        let center = app.last_composition.as_ref().unwrap().model.center();
        run_frame(
            &context,
            &mut app,
            vec![
                egui::Event::PointerMoved(center),
                egui::Event::PointerButton {
                    pos: center,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        run_frame(
            &context,
            &mut app,
            vec![egui::Event::PointerMoved(center + egui::vec2(16.0, 7.0))],
        );
        assert_ne!(app.model_view.camera(), editor_core::OrbitCamera::default());

        app.handle_menu_action(MenuAction::ResetView, &context);

        assert_eq!(app.model_view.camera(), editor_core::OrbitCamera::default());
    }
}
