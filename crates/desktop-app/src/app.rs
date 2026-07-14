use std::path::PathBuf;

use editor_core::EditorDocument;
use eframe::egui;
use relief_core::{Bounds, CanonicalView};

use crate::{
    layout::{self, Rect, Size, WorkspaceLayout},
    menu::{MenuAction, PendingDestructiveAction, UnsavedChoice, show_menu_bar},
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
    source_grid: SourceGridState,
}

impl DepthSpriteApp {
    pub fn from_startup_path(path: Option<PathBuf>) -> Self {
        let shell = ShellState::from_startup_path(path);
        let palette = PaletteState::new(shell.document());
        Self {
            shell,
            palette,
            source_grid: SourceGridState::default(),
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
            MenuAction::ResetView => {}
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
        egui::Panel::top("top-menu")
            .exact_size(layout::MENU_HEIGHT)
            .show(root, |ui| selected = show_menu_bar(ui));
        if let Some(action) = selected {
            self.handle_menu_action(action, &context);
        }

        let root_rect = root.max_rect();
        egui::CentralPanel::default().show(root, |ui| {
            let layout = layout::calculate_layout(Size::new(root_rect.width(), root_rect.height()))
                .expect("native window must respect the derived minimum size");
            paint_model_placeholder(ui, &layout, root_rect.min);
            let tools_rect = to_egui(layout.tools, root_rect.min);
            ui.scope_builder(egui::UiBuilder::new().max_rect(tools_rect), |ui| {
                self.palette.show(ui, &mut self.shell.document);
            });
            self.source_grid.show(
                ui,
                &mut self.shell.document,
                &layout.source_cards,
                root_rect.min,
            );
        });

        if self.shell.file_error().is_some() {
            self.show_file_error_modal(&context);
        } else {
            self.show_unsaved_modal(&context);
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

fn paint_model_placeholder(ui: &mut egui::Ui, layout: &WorkspaceLayout, origin: egui::Pos2) {
    let painter = ui.painter();
    painter.rect_filled(
        to_egui(layout.model, origin),
        4.0,
        egui::Color32::from_gray(24),
    );
    painter.text(
        to_egui(layout.model, origin).left_top() + egui::vec2(10.0, 10.0),
        egui::Align2::LEFT_TOP,
        "MODEL",
        egui::FontId::monospace(12.0),
        egui::Color32::LIGHT_GRAY,
    );
}

fn to_egui(rect: Rect, origin: egui::Pos2) -> egui::Rect {
    egui::Rect::from_min_max(
        origin + egui::vec2(rect.left(), rect.top()),
        origin + egui::vec2(rect.right(), rect.bottom()),
    )
}
