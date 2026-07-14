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
    export_jobs::{ExportEvent, ExportWorker},
    export_ui::{self, ExportOptions},
    jobs::{RenderEvent, RenderResult, RenderWorker, install_if_current},
    viewport::{self, ViewPreset, ViewportState},
};

const MODEL_FILTER: &[&str] = &["depthsprite"];
const PNG_FILTER: &[&str] = &["png"];
pub(crate) const BUNDLED_BOWL: &[u8] = include_bytes!("../../../assets/examples/bowl.depthsprite");

struct StartupDocument {
    document: Option<Document>,
    message: Option<String>,
}

fn startup_document(explicit_path: Option<PathBuf>, bundled: &'static [u8]) -> StartupDocument {
    let explicit_message = if let Some(path) = explicit_path {
        match Document::open(&path) {
            Ok(document) => {
                return StartupDocument {
                    document: Some(document),
                    message: None,
                };
            }
            Err(error) => Some(format!("Could not open {}: {error}", path.display())),
        }
    } else {
        None
    };
    match Document::from_bundled("bowl.depthsprite", bundled) {
        Ok(document) => StartupDocument {
            document: Some(document),
            message: explicit_message,
        },
        Err(error) => StartupDocument {
            document: None,
            message: Some(match explicit_message {
                Some(message) => format!("{message}; bundled bowl also failed: {error}"),
                None => format!("Could not load bundled bowl: {error}"),
            }),
        },
    }
}

fn ensure_extension(path: &Path, extension: &str) -> PathBuf {
    if path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
    {
        return path.to_owned();
    }
    let mut value: OsString = path.as_os_str().to_owned();
    value.push(".");
    value.push(extension);
    PathBuf::from(value)
}

pub(crate) struct DepthSpriteApp {
    document: Option<Document>,
    worker: RenderWorker,
    export_worker: ExportWorker,
    export_pending: Option<u64>,
    next_export_tag: u64,
    viewport: ViewportState,
    frame: Option<FrameBuffer>,
    texture: Option<egui::TextureHandle>,
    export_options: ExportOptions,
    message: Option<String>,
    preview_error: Option<String>,
}

impl DepthSpriteApp {
    pub(crate) fn new(initial_path: Option<PathBuf>) -> Self {
        let startup = startup_document(initial_path, BUNDLED_BOWL);
        let mut app = Self {
            document: startup.document,
            worker: RenderWorker::new(),
            export_worker: ExportWorker::new(),
            export_pending: None,
            next_export_tag: 0,
            viewport: ViewportState::default(),
            frame: None,
            texture: None,
            export_options: ExportOptions::default(),
            message: startup.message,
            preview_error: None,
        };
        if app.document.is_some() {
            app.queue_render(app.viewport.generation());
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
                    installed |= install_preview_if_current(
                        self.viewport.generation(),
                        result,
                        &mut self.frame,
                        &mut self.preview_error,
                    );
                }
                Ok(RenderEvent::Failed { generation, error }) => {
                    if generation == self.viewport.generation() {
                        self.preview_error = Some(format!("Preview render failed: {error}"));
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.preview_error = Some("Preview render worker stopped".to_owned());
                    break;
                }
            }
        }
        if installed {
            self.upload_frame(context);
        }
    }

    fn poll_export_worker(&mut self) {
        loop {
            match self.export_worker.try_recv() {
                Ok(ExportEvent::Complete { tag, path, .. }) => {
                    if self.export_pending == Some(tag) {
                        self.export_pending = None;
                        self.message = Some(format!("Exported {}", path.display()));
                    }
                }
                Ok(ExportEvent::Failed {
                    tag, path, error, ..
                }) => {
                    if self.export_pending == Some(tag) {
                        self.export_pending = None;
                        self.message =
                            Some(format!("Could not export {}: {error}", path.display()));
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    if self.export_pending.take().is_some() {
                        self.message = Some("Export worker stopped".to_owned());
                    }
                    break;
                }
            }
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
                self.preview_error = None;
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

    fn queue_export(&mut self, path: PathBuf) {
        let Some(document) = &self.document else {
            return;
        };
        let request = match self.export_options.request() {
            Ok(request) => request,
            Err(error) => {
                self.message = Some(format!("Could not export {}: {error}", path.display()));
                return;
            }
        };
        let Some(tag) = self.next_export_tag.checked_add(1) else {
            self.message = Some("Could not export: export identifier exhausted".to_owned());
            return;
        };
        match self
            .export_worker
            .submit(tag, document.clone(), request, path.clone())
        {
            Ok(()) => {
                self.next_export_tag = tag;
                self.export_pending = Some(tag);
                self.message = Some(format!("Exporting {}…", path.display()));
            }
            Err(error) => {
                self.message = Some(format!("Could not export {}: {error}", path.display()));
            }
        }
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
        self.poll_export_worker();
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
            export = self.document.is_some()
                && export_ui::show(ui, &mut self.export_options, self.export_pending.is_none());
        });

        if self.message.is_some() || self.preview_error.is_some() {
            egui::Panel::bottom("status").show(root, |ui| {
                if let Some(message) = &self.message {
                    ui.label(message);
                }
                if let Some(error) = &self.preview_error {
                    ui.label(error);
                }
            });
        }

        let mut viewport_input = None;
        egui::CentralPanel::default().show(root, |ui| {
            if self.document.is_none() {
                ui.centered_and_justified(|ui| {
                    ui.heading("No depth sprite loaded");
                });
            } else {
                egui::ScrollArea::both().show(ui, |ui| {
                    viewport_input =
                        Some(viewport::show(ui, self.texture.as_ref(), &self.viewport));
                });
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
            self.save_as(ensure_extension(&path, "depthsprite"));
        }
        if export
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("PNG", PNG_FILTER)
                .save_file()
        {
            self.queue_export(ensure_extension(&path, "png"));
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
            self.viewport.wheel(input.wheel_steps);
        }
    }
}

fn install_preview_if_current(
    current_generation: u64,
    result: RenderResult,
    frame: &mut Option<FrameBuffer>,
    preview_error: &mut Option<String>,
) -> bool {
    let installed = install_if_current(current_generation, result, frame);
    if installed {
        *preview_error = None;
    }
    installed
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
    use std::path::{Path, PathBuf};

    use tempfile::tempdir;

    use crate::jobs::RenderResult;
    use relief_render::{RenderRequest, TargetView, render_model};

    use super::{BUNDLED_BOWL, ensure_extension, install_preview_if_current, startup_document};

    fn asset(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("assets/examples")
            .join(name)
    }

    fn transparent_frame() -> relief_render::FrameBuffer {
        render_model(&[], &RenderRequest::new(1, 1, TargetView::front_v1())).unwrap()
    }

    #[test]
    fn valid_command_line_document_has_priority_over_bundled_bowl() {
        let initial = asset("block.depthsprite");

        let startup = startup_document(Some(initial.clone()), BUNDLED_BOWL);

        assert_eq!(startup.document.unwrap().path(), Some(initial.as_path()));
        assert!(startup.message.is_none());
    }

    #[test]
    fn failed_command_line_open_falls_back_to_embedded_bowl_and_keeps_error_visible() {
        let temp = tempdir().unwrap();
        let missing = temp.path().join("missing.depthsprite");

        let startup = startup_document(Some(missing.clone()), BUNDLED_BOWL);

        assert_eq!(startup.document.unwrap().display_name(), "bowl.depthsprite");
        let message = startup.message.unwrap();
        assert!(message.contains(&missing.display().to_string()));
        assert!(message.contains("Could not open"));
    }

    #[test]
    fn startup_is_empty_only_when_embedded_document_is_invalid() {
        let startup = startup_document(None, b"not a depth sprite");

        assert!(startup.document.is_none());
        assert!(startup.message.unwrap().contains("bundled bowl"));
    }

    #[test]
    fn save_extensions_are_appended_unless_exact_suffix_exists_case_insensitively() {
        assert_eq!(
            ensure_extension(Path::new("sprite"), "depthsprite"),
            PathBuf::from("sprite.depthsprite")
        );
        assert_eq!(
            ensure_extension(Path::new("sprite.DEPTHSPRITE"), "depthsprite"),
            PathBuf::from("sprite.DEPTHSPRITE")
        );
        assert_eq!(
            ensure_extension(Path::new("sprite.backup"), "depthsprite"),
            PathBuf::from("sprite.backup.depthsprite")
        );
        assert_eq!(
            ensure_extension(Path::new("sheet.PNG"), "png"),
            PathBuf::from("sheet.PNG")
        );

        let non_utf8_safe = ensure_extension(Path::new("sheet"), "png");
        assert_eq!(non_utf8_safe, PathBuf::from("sheet.png"));
    }

    #[test]
    fn only_a_current_success_clears_the_preview_error() {
        let mut frame = None;
        let mut error = Some("old failure".to_owned());

        assert!(!install_preview_if_current(
            2,
            RenderResult {
                generation: 1,
                frame: transparent_frame(),
            },
            &mut frame,
            &mut error,
        ));
        assert_eq!(error.as_deref(), Some("old failure"));

        assert!(install_preview_if_current(
            2,
            RenderResult {
                generation: 2,
                frame: transparent_frame(),
            },
            &mut frame,
            &mut error,
        ));
        assert!(error.is_none());
    }
}
