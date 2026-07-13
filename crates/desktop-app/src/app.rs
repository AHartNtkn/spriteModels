use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

use desktop_app::document::Document;
use eframe::egui;
use relief_core::CanonicalView;
use relief_render::{FrameBuffer, RenderDiagnostic};

use crate::{
    export_ui::{self, ExportOptions},
    jobs::{RenderEvent, RenderWorker, install_if_current},
    viewport::{self, ViewPreset, ViewportState},
};

const MODEL_FILTER: &[&str] = &["depthsprite"];
const PNG_FILTER: &[&str] = &["png"];

pub(crate) fn select_initial_path(
    command_line: Option<OsString>,
    bundled_bowl: &Path,
) -> Option<PathBuf> {
    command_line
        .map(PathBuf::from)
        .or_else(|| bundled_bowl.is_file().then(|| bundled_bowl.to_owned()))
}

pub(crate) struct DepthSpriteApp {
    document: Option<Document>,
    worker: RenderWorker,
    viewport: ViewportState,
    frame: Option<FrameBuffer>,
    texture: Option<egui::TextureHandle>,
    export_options: ExportOptions,
    message: Option<String>,
}

impl DepthSpriteApp {
    pub(crate) fn new(initial_path: Option<PathBuf>) -> Self {
        let mut app = Self {
            document: None,
            worker: RenderWorker::new(),
            viewport: ViewportState::default(),
            frame: None,
            texture: None,
            export_options: ExportOptions::default(),
            message: None,
        };
        if let Some(path) = initial_path {
            match Document::open(&path) {
                Ok(document) => {
                    app.document = Some(document);
                    app.queue_render(app.viewport.generation());
                }
                Err(error) => {
                    app.message = Some(format!("Could not open {}: {error}", path.display()));
                }
            }
        }
        app
    }

    fn queue_render(&mut self, generation: u64) {
        let Some(document) = &self.document else {
            return;
        };
        if let Err(error) = self.worker.submit(
            generation,
            document.model().clone(),
            self.viewport.request(),
        ) {
            self.message = Some(error.to_string());
        }
    }

    fn poll_worker(&mut self, context: &egui::Context) {
        let mut installed = false;
        loop {
            match self.worker.try_recv() {
                Ok(RenderEvent::Complete(result)) => {
                    installed |=
                        install_if_current(self.viewport.generation(), result, &mut self.frame);
                }
                Ok(RenderEvent::Failed { generation, error }) => {
                    if generation == self.viewport.generation() {
                        self.message = Some(format!("Preview render failed: {error}"));
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.message = Some("Preview render worker stopped".to_owned());
                    break;
                }
            }
        }
        if installed {
            self.upload_frame(context);
        }
    }

    fn upload_frame(&mut self, context: &egui::Context) {
        let Some(frame) = &self.frame else {
            return;
        };
        let rgba = frame
            .pixels()
            .iter()
            .flat_map(|pixel| pixel.iter().copied())
            .collect::<Vec<_>>();
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [frame.width() as usize, frame.height() as usize],
            &rgba,
        );
        self.texture = Some(context.load_texture(
            format!("preview-{}", self.viewport.generation()),
            image,
            egui::TextureOptions::NEAREST,
        ));
    }

    fn open_document(&mut self, path: PathBuf) {
        let outcome = if let Some(document) = &mut self.document {
            document.replace_from_path(&path)
        } else {
            Document::open(&path).map(|document| self.document = Some(document))
        };
        match outcome {
            Ok(()) => {
                self.frame = None;
                self.texture = None;
                self.message = None;
                let generation = self.viewport.document_changed();
                self.queue_render(generation);
            }
            Err(error) => {
                self.message = Some(format!("Could not open {}: {error}", path.display()));
            }
        }
    }

    fn save_as(&mut self, path: PathBuf) {
        let Some(document) = &mut self.document else {
            return;
        };
        match document.save_as(&path) {
            Ok(()) => self.message = Some(format!("Saved {}", path.display())),
            Err(error) => {
                self.message = Some(format!("Could not save {}: {error}", path.display()));
            }
        }
    }

    fn export_sheet(&mut self, path: PathBuf) {
        let Some(document) = &self.document else {
            return;
        };
        let result = self
            .export_options
            .request()
            .map_err(|error| error.to_string())
            .and_then(|request| {
                document
                    .export_sheet(&path, &request)
                    .map_err(|error| error.to_string())
            });
        self.message = Some(match result {
            Ok(()) => format!("Exported {}", path.display()),
            Err(error) => format!("Could not export {}: {error}", path.display()),
        });
    }

    fn document_panel(&self, ui: &mut egui::Ui) {
        let Some(document) = &self.document else {
            ui.heading("No document");
            ui.label("Open a .depthsprite file to inspect it.");
            return;
        };
        ui.heading(document.display_name());
        let bounds = document.model().bounds();
        ui.label(format!(
            "Bounds: {} × {} × {}",
            bounds.width(),
            bounds.height(),
            bounds.depth()
        ));
        ui.label(format!(
            "Current view: {}",
            self.viewport.current_view_name()
        ));
        ui.label(format!("Zoom: {}×", self.viewport.zoom()));
        ui.separator();
        ui.label("Charts:");
        for chart in document.model().charts() {
            ui.label(format!("• {}", view_name(chart.view())));
        }
        ui.separator();
        ui.label("Warnings:");
        match &self.frame {
            Some(frame) if frame.diagnostics().is_empty() => {
                ui.label("None");
            }
            Some(frame) => {
                for warning in frame.diagnostics() {
                    ui.label(diagnostic_text(warning));
                }
            }
            None => {
                ui.label("Preview pending");
            }
        }
        ui.label("Unsupported regions remain transparent.");
    }
}

impl eframe::App for DepthSpriteApp {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = root.ctx().clone();
        self.poll_worker(&context);
        context.request_repaint_after(Duration::from_millis(16));

        let mut open = false;
        let mut save_as = false;
        let mut preset = None;
        egui::Panel::top("commands").show(root, |ui| {
            ui.horizontal(|ui| {
                open = ui.button("Open…").clicked();
                save_as = ui
                    .add_enabled(self.document.is_some(), egui::Button::new("Save As…"))
                    .clicked();
                ui.separator();
                for (label, choice) in [
                    ("Front", ViewPreset::Front),
                    ("Top", ViewPreset::Top),
                    ("Side", ViewPreset::Side),
                    ("Isometric", ViewPreset::Isometric),
                ] {
                    if ui.button(label).clicked() {
                        preset = Some(choice);
                    }
                }
            });
        });

        let mut export = false;
        egui::Panel::right("document").show(root, |ui| {
            self.document_panel(ui);
            ui.separator();
            export = self.document.is_some() && export_ui::show(ui, &mut self.export_options);
        });

        if let Some(message) = &self.message {
            egui::Panel::bottom("status").show(root, |ui| {
                ui.label(message);
            });
        }

        let mut viewport_input = None;
        egui::CentralPanel::default().show(root, |ui| {
            if self.document.is_none() {
                ui.centered_and_justified(|ui| {
                    ui.heading("No depth sprite loaded");
                });
            } else {
                viewport_input = Some(viewport::show(ui, self.texture.as_ref()));
            }
        });

        if open
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("DepthSprite", MODEL_FILTER)
                .pick_file()
        {
            self.open_document(path);
        }
        if save_as
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("DepthSprite", MODEL_FILTER)
                .save_file()
        {
            self.save_as(path);
        }
        if export
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("PNG", PNG_FILTER)
                .save_file()
        {
            self.export_sheet(path);
        }
        if let Some(preset) = preset {
            let generation = self.viewport.select_preset(preset);
            self.queue_render(generation);
        }
        if let Some(input) = viewport_input {
            if let Some((x, y)) = input.drag
                && let Some(generation) = self.viewport.drag(x, y)
            {
                self.queue_render(generation);
            }
            if let Some(generation) = self.viewport.wheel(input.wheel_steps) {
                self.queue_render(generation);
            }
        }
    }
}

fn view_name(view: CanonicalView) -> &'static str {
    match view {
        CanonicalView::Front => "front",
        CanonicalView::Back => "back",
        CanonicalView::Left => "left",
        CanonicalView::Right => "right",
        CanonicalView::Top => "top",
        CanonicalView::Bottom => "bottom",
    }
}

fn diagnostic_text(diagnostic: &RenderDiagnostic) -> String {
    format!("• {diagnostic:?}")
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, fs};

    use tempfile::tempdir;

    use super::select_initial_path;

    #[test]
    fn command_line_path_has_priority_even_when_it_does_not_exist() {
        let temp = tempdir().unwrap();
        let bowl = temp.path().join("bowl.depthsprite");
        fs::write(&bowl, b"bundled").unwrap();
        let command_line = temp.path().join("requested.depthsprite");

        assert_eq!(
            select_initial_path(Some(OsString::from(&command_line)), &bowl),
            Some(command_line)
        );
    }

    #[test]
    fn startup_falls_back_to_existing_bowl_then_honest_empty_state() {
        let temp = tempdir().unwrap();
        let bowl = temp.path().join("bowl.depthsprite");

        assert_eq!(select_initial_path(None, &bowl), None);
        fs::write(&bowl, b"bundled").unwrap();
        assert_eq!(select_initial_path(None, &bowl), Some(bowl));
    }
}
