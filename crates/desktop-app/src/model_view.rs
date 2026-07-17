use editor_core::{EditorDocument, EditorError, OrbitCamera, PreviewCache};
use eframe::egui;
use relief_render::FrameBuffer;

const FIT_FRACTION: f32 = 0.9;
const DEFAULT_ZOOM_MILLI: u32 = 1_000;
const ZOOM_EXPONENT_PER_POINT: f64 = 0.002;
const MIN_ZOOM_MILLI: u32 = 250;
const MAX_ZOOM_MILLI: u32 = 4_000;

pub struct ModelView {
    camera: OrbitCamera,
    zoom_milli: u32,
    preview: PreviewCache,
    texture: Option<egui::TextureHandle>,
    uploaded_generation: Option<u64>,
    #[cfg(test)]
    texture_upload_calls: u64,
}

impl Default for ModelView {
    fn default() -> Self {
        Self {
            camera: OrbitCamera::default(),
            zoom_milli: DEFAULT_ZOOM_MILLI,
            preview: PreviewCache::default(),
            texture: None,
            uploaded_generation: None,
            #[cfg(test)]
            texture_upload_calls: 0,
        }
    }
}

pub struct ModelViewOutput {
    pub response: egui::Response,
    #[cfg(test)]
    pub(crate) observation: ModelViewObservation,
}

#[cfg(test)]
pub(crate) struct ModelViewObservation {
    pub rect: egui::Rect,
    pub image_rect: egui::Rect,
}

impl ModelView {
    pub fn camera(&self) -> OrbitCamera {
        self.camera
    }

    pub fn reset(&mut self) {
        self.camera.reset();
        self.zoom_milli = DEFAULT_ZOOM_MILLI;
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        document: &EditorDocument,
        rect: egui::Rect,
    ) -> Result<ModelViewOutput, EditorError> {
        let response = ui.interact(rect, ui.id().with("model-view"), egui::Sense::drag());
        if response.dragged_by(egui::PointerButton::Primary) {
            let delta = response.drag_delta();
            self.camera.drag(delta.x, delta.y);
        }
        if response.hovered() {
            let wheel_delta = ui.input_mut(|input| {
                let delta = input.smooth_scroll_delta.y;
                input.smooth_scroll_delta.y = 0.0;
                delta
            });
            self.zoom(wheel_delta);
        }

        let preview = self.preview.frame(document, self.camera)?;
        let generation = preview.generation();
        let native_size = egui::vec2(
            preview.framebuffer().width() as f32,
            preview.framebuffer().height() as f32,
        );
        let image = (self.uploaded_generation != Some(generation))
            .then(|| color_image(preview.framebuffer()));
        if let Some(image) = image {
            if let Some(texture) = &mut self.texture {
                texture.set(image, egui::TextureOptions::NEAREST);
                #[cfg(test)]
                {
                    self.texture_upload_calls = self
                        .texture_upload_calls
                        .checked_add(1)
                        .expect("texture upload count must remain monotonic");
                }
            } else {
                self.texture = Some(ui.ctx().load_texture(
                    "depthsprite-model-preview",
                    image,
                    egui::TextureOptions::NEAREST,
                ));
                #[cfg(test)]
                {
                    self.texture_upload_calls = self
                        .texture_upload_calls
                        .checked_add(1)
                        .expect("texture upload count must remain monotonic");
                }
            }
            self.uploaded_generation = Some(generation);
        }

        ui.painter()
            .rect_filled(rect, 4.0, egui::Color32::from_gray(24));
        let scale = presentation_scale(native_size, rect.size(), self.zoom_milli);
        let image_rect = egui::Rect::from_center_size(rect.center(), native_size * scale as f32);
        if let Some(texture) = &self.texture {
            ui.painter().with_clip_rect(rect).image(
                texture.id(),
                image_rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        Ok(ModelViewOutput {
            #[cfg(test)]
            observation: ModelViewObservation {
                rect: response.rect,
                image_rect,
            },
            response,
        })
    }

    fn zoom(&mut self, wheel_delta: f32) {
        if !wheel_delta.is_finite() {
            return;
        }
        let zoom =
            f64::from(self.zoom_milli) * (f64::from(wheel_delta) * ZOOM_EXPONENT_PER_POINT).exp();
        self.zoom_milli =
            zoom.round()
                .clamp(f64::from(MIN_ZOOM_MILLI), f64::from(MAX_ZOOM_MILLI)) as u32;
    }
}

fn presentation_scale(native: egui::Vec2, available: egui::Vec2, zoom_milli: u32) -> u32 {
    let fit_width = (available.x * FIT_FRACTION / native.x).floor();
    let fit_height = (available.y * FIT_FRACTION / native.y).floor();
    let fit = fit_width.min(fit_height).max(1.0) as u32;
    ((u64::from(fit) * u64::from(zoom_milli) + 500) / 1_000).max(1) as u32
}

fn color_image(framebuffer: &FrameBuffer) -> egui::ColorImage {
    egui::ColorImage::new(
        [framebuffer.width() as usize, framebuffer.height() as usize],
        framebuffer
            .pixels()
            .iter()
            .map(|pixel| {
                egui::Color32::from_rgba_unmultiplied(pixel[0], pixel[1], pixel[2], pixel[3])
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use editor_core::{ActiveLayer, DepthValue, ReliefValue};
    use relief_core::{Bounds, CanonicalView};

    use super::*;

    fn run_frame(context: &egui::Context, model_view: &mut ModelView, document: &EditorDocument) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(240.0, 160.0),
            )),
            ..Default::default()
        };
        let _ = context.run_ui(input, |ui| {
            model_view.show(ui, document, ui.max_rect()).unwrap();
        });
    }

    #[test]
    fn batched_edits_render_and_upload_once_in_the_next_real_ui_frame() {
        let context = egui::Context::default();
        let mut document = EditorDocument::new(Bounds::new(3, 1, 1).unwrap(), CanonicalView::Front);
        document.set_active_layer(ActiveLayer::Depth);
        document.set_current_depth(DepthValue::Relief(ReliefValue::new(4).unwrap()));
        let mut model_view = ModelView::default();
        run_frame(&context, &mut model_view, &document);
        let generation_before_edits = model_view.preview.generation();
        let uploads_before_edits = model_view.texture_upload_calls;

        for x in 0..3 {
            document.begin_stroke().unwrap();
            document.pencil_pixel(CanonicalView::Front, x, 0).unwrap();
            document.finish_stroke().unwrap();
        }
        run_frame(&context, &mut model_view, &document);
        assert_eq!(model_view.preview.generation() - generation_before_edits, 1);
        assert_eq!(model_view.texture_upload_calls - uploads_before_edits, 1);

        let unchanged_generation = model_view.preview.generation();
        let unchanged_uploads = model_view.texture_upload_calls;

        run_frame(&context, &mut model_view, &document);
        assert_eq!(model_view.preview.generation(), unchanged_generation);
        assert_eq!(model_view.texture_upload_calls, unchanged_uploads);
    }

    #[test]
    fn integer_presentation_scale_fits_then_zooms_without_render_geometry() {
        assert_eq!(
            presentation_scale(egui::vec2(84.0, 84.0), egui::vec2(1000.0, 950.0), 1_000),
            10
        );
        assert_eq!(
            presentation_scale(egui::vec2(84.0, 84.0), egui::vec2(1000.0, 950.0), 1_500),
            15
        );
        assert_eq!(
            presentation_scale(egui::vec2(84.0, 84.0), egui::vec2(420.0, 420.0), 1_000),
            4
        );
    }

    #[test]
    fn normalized_wheel_step_zooms_in_without_hitting_maximum() {
        let mut model_view = ModelView::default();

        model_view.zoom(40.0);

        assert!(model_view.zoom_milli > DEFAULT_ZOOM_MILLI);
        assert!(model_view.zoom_milli < MAX_ZOOM_MILLI);
    }

    #[test]
    fn normalized_wheel_step_zooms_out_without_hitting_minimum() {
        let mut model_view = ModelView::default();

        model_view.zoom(-40.0);

        assert!(model_view.zoom_milli < DEFAULT_ZOOM_MILLI);
        assert!(model_view.zoom_milli > MIN_ZOOM_MILLI);
    }
}
