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

    pub fn report_file_error(&mut self, message: String) {
        self.file_error = Some(message);
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
            PendingDestructiveAction::Import(model) => {
                self.document = EditorDocument::from_unsaved_model(model);
                self.file_error = None;
            }
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
    pending_import_scene: Option<(mesh_import::TriangleScene, String)>,
    #[cfg(test)]
    last_composition: Option<CompositionObservation>,
}

#[cfg(test)]
struct CompositionObservation {
    menu: crate::menu::MenuObservation,
    palette: crate::palette::PaletteObservation,
    model: crate::model_view::ModelViewObservation,
    source_grid: crate::source_grid::SourceGridObservation,
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
            pending_import_scene: None,
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
            MenuAction::ImportModel => {
                if let Some(path) = pick_import_path() {
                    match mesh_import::load_scene(&path) {
                        Ok(scene) => {
                            let label = path.file_name().map_or_else(
                                || path.display().to_string(),
                                |name| name.to_string_lossy().into_owned(),
                            );
                            self.pending_import_scene = Some((scene, label));
                        }
                        Err(error) => self.shell.report_file_error(format!(
                            "Could not import {}: {error}",
                            path.display()
                        )),
                    }
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

    fn show_import_modal(&mut self, context: &egui::Context) {
        let Some((scene, _label)) = self.pending_import_scene.as_ref() else {
            return;
        };
        let triangle_count = scene.triangles.len();
        let mut cancel = false;
        egui::Modal::new("import-model-modal".into()).show(context, |ui| {
            ui.heading("Import 3D Model");
            ui.label(format!("Triangles: {triangle_count}"));
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
        if cancel {
            self.pending_import_scene = None;
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

        let mut menu_output = None;
        egui::Panel::top("top-menu")
            .exact_size(layout::MENU_HEIGHT)
            .show(root, |ui| menu_output = Some(show_menu_bar(ui)));
        let menu_output = menu_output.expect("the top panel always renders its menu bar");
        if let Some(action) = menu_output.action {
            self.handle_menu_action(action, &context);
        }
        #[cfg(test)]
        let menu_observation = menu_output.observation;

        let root_rect = root.max_rect();
        #[cfg(test)]
        let mut composition = None;
        egui::CentralPanel::default().show(root, |ui| {
            let source_count = self.shell.document.sources().len();
            let layout = layout::calculate_layout(
                Size::new(root_rect.width(), root_rect.height()),
                source_count,
            )
            .expect("native window must respect the derived minimum size");
            let tools_rect = to_egui(layout.tools, root_rect.min);
            let palette_output = ui
                .scope_builder(egui::UiBuilder::new().max_rect(tools_rect), |ui| {
                    self.palette.show(ui, &mut self.shell.document)
                })
                .inner;
            let model_rect = to_egui(layout.model, root_rect.min);
            let model_output = self.model_view.show(ui, self.shell.document(), model_rect);
            if let Err(error) = &model_output {
                ui.painter().text(
                    model_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    format!("Preview unavailable: {error}"),
                    egui::FontId::monospace(12.0),
                    egui::Color32::LIGHT_RED,
                );
            }
            let source_grid_output = self.source_grid.show(
                ui,
                &mut self.shell.document,
                &layout.source_cards,
                layout.add_button,
                root_rect.min,
            );
            #[cfg(test)]
            {
                composition = Some(CompositionObservation {
                    menu: menu_observation,
                    palette: palette_output.observation,
                    model: model_output
                        .expect("the valid document always renders a model preview")
                        .observation,
                    source_grid: source_grid_output.observation,
                });
            }
            #[cfg(not(test))]
            {
                let _ = (palette_output, model_output, source_grid_output);
            }
        });

        if self.shell.file_error().is_some() {
            self.show_file_error_modal(&context);
        } else if self.pending_import_scene.is_some() {
            self.show_import_modal(&context);
        } else {
            self.show_unsaved_modal(&context);
        }
        #[cfg(test)]
        {
            self.last_composition = Some(
                composition.expect("actual workspace widget observations are always recorded"),
            );
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

fn pick_import_path() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("glTF", &["gltf", "glb"])
        .pick_file()
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
    fn real_frame_observes_every_rendered_workspace_widget_and_ratio() {
        let context = egui::Context::default();
        let mut app = DepthSpriteApp::from_startup_path(None);
        app.shell
            .document
            .set_source_opposite(CanonicalView::Front, false)
            .unwrap();
        for view in layout::CANONICAL_SOURCE_ORDER.into_iter().skip(1) {
            app.shell.document.add_source(view).unwrap();
        }
        assert_eq!(app.shell.document.sources().len(), 6);

        let _output = run_frame(&context, &mut app, Vec::new());
        let composition = app.last_composition.as_ref().unwrap();

        assert!(composition.menu.rect.is_positive());
        assert!(composition.menu.rect.left() >= 0.0);
        assert!(composition.menu.rect.right() <= 1600.0);

        assert!(composition.palette.rect.is_positive());
        assert_eq!(composition.palette.controls.len(), 8);
        for control in &composition.palette.controls {
            assert!(control.is_positive());
            assert!(composition.palette.rect.contains_rect(*control));
        }
        for pair in composition.palette.controls.windows(2) {
            assert!(pair[0].top() < pair[1].top());
        }

        assert!(composition.model.rect.is_positive());
        assert!(composition.model.image_rect.is_positive());
        assert!(
            composition
                .model
                .rect
                .contains_rect(composition.model.image_rect)
        );
        assert!(composition.menu.rect.bottom() <= composition.palette.rect.top());
        assert!(composition.menu.rect.bottom() <= composition.model.rect.top());
        assert!(composition.palette.rect.right() < composition.model.rect.left());
        assert_eq!(composition.source_grid.cards.len(), 6);
        assert!(composition.source_grid.add_button.is_none());
        assert_eq!(
            composition
                .source_grid
                .cards
                .iter()
                .map(|card| card.view)
                .collect::<Vec<_>>(),
            layout::CANONICAL_SOURCE_ORDER
        );

        for (index, card) in composition.source_grid.cards.iter().enumerate() {
            assert_eq!(card.column, index % layout::SOURCE_COLUMNS);
            assert_eq!(card.row, index / layout::SOURCE_COLUMNS);
            assert!(card.card.is_positive());
            assert!(card.card.contains_rect(card.color));
            assert!(card.card.contains_rect(card.depth));
            assert!(card.color.bottom() < card.depth.top());
            assert!((card.color.width() - card.depth.width()).abs() <= 0.05);
            assert!(
                (card.color.height() - card.depth.height()).abs() <= 0.05,
                "egui may snap independently derived canvas edges by one subpixel"
            );
            assert!(
                composition.model.rect.width() + 0.1
                    >= card.color.width() * layout::MODEL_TO_CANVAS_RATIO
            );
            assert!(
                composition.model.rect.height() + 0.1
                    >= card.color.height() * layout::MODEL_TO_CANVAS_RATIO
            );
        }
        for row in 0..layout::SOURCE_ROWS {
            for column in 1..layout::SOURCE_COLUMNS {
                let previous =
                    &composition.source_grid.cards[row * layout::SOURCE_COLUMNS + column - 1];
                let current = &composition.source_grid.cards[row * layout::SOURCE_COLUMNS + column];
                assert!(previous.card.right() < current.card.left());
            }
        }
        for column in 0..layout::SOURCE_COLUMNS {
            let above = &composition.source_grid.cards[column];
            let below = &composition.source_grid.cards[layout::SOURCE_COLUMNS + column];
            assert!(above.card.bottom() < below.card.top());
        }
        let sources_rect = composition
            .source_grid
            .cards
            .iter()
            .skip(1)
            .fold(composition.source_grid.cards[0].card, |bounds, card| {
                bounds.union(card.card)
            });
        assert!(composition.model.rect.right() < sources_rect.left());
        assert!(composition.model.rect.width() > sources_rect.width());
    }

    #[test]
    fn reset_menu_action_restores_the_integrated_model_camera() {
        let context = egui::Context::default();
        let mut app = DepthSpriteApp::from_startup_path(None);
        run_frame(&context, &mut app, Vec::new());
        let center = app.last_composition.as_ref().unwrap().model.rect.center();
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
