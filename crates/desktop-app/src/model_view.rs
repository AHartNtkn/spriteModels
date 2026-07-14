use editor_core::{EditorDocument, EditorError, OrbitCamera, PreviewCache};
use eframe::egui;
use relief_render::FrameBuffer;

#[derive(Default)]
pub struct ModelView {
    camera: OrbitCamera,
    preview: PreviewCache,
    texture: Option<egui::TextureHandle>,
    #[cfg(test)]
    last_frame: FrameObservation,
}

#[cfg(test)]
#[derive(Default)]
struct FrameObservation {
    preview_renders: u8,
    texture_updates: u8,
}

impl ModelView {
    pub fn camera(&self) -> OrbitCamera {
        self.camera
    }

    pub fn reset(&mut self) {
        self.camera.reset();
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        document: &EditorDocument,
        rect: egui::Rect,
    ) -> Result<egui::Response, EditorError> {
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
            self.camera.zoom(wheel_delta);
        }

        let width = rect.width().round().max(1.0) as u32;
        let height = rect.height().round().max(1.0) as u32;
        let preview = self.preview.frame(document, self.camera, width, height)?;
        let changed = preview.changed();
        #[cfg(test)]
        {
            self.last_frame = FrameObservation {
                preview_renders: u8::from(changed),
                texture_updates: u8::from(changed),
            };
        }
        let image = changed.then(|| color_image(preview.framebuffer()));
        if let Some(image) = image {
            if let Some(texture) = &mut self.texture {
                texture.set(image, egui::TextureOptions::NEAREST);
            } else {
                self.texture = Some(ui.ctx().load_texture(
                    "depthsprite-model-preview",
                    image,
                    egui::TextureOptions::NEAREST,
                ));
            }
        }

        ui.painter()
            .rect_filled(rect, 4.0, egui::Color32::from_gray(24));
        if let Some(texture) = &self.texture {
            ui.painter().image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        Ok(response)
    }
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
        document.set_current_depth(DepthValue::Relief(ReliefValue::new(8).unwrap()));
        let mut model_view = ModelView::default();
        run_frame(&context, &mut model_view, &document);

        for x in 0..3 {
            document.begin_stroke().unwrap();
            document.pencil_pixel(CanonicalView::Front, x, 0).unwrap();
            document.finish_stroke().unwrap();
        }
        run_frame(&context, &mut model_view, &document);
        assert_eq!(model_view.last_frame.preview_renders, 1);
        assert_eq!(model_view.last_frame.texture_updates, 1);

        run_frame(&context, &mut model_view, &document);
        assert_eq!(model_view.last_frame.preview_renders, 0);
        assert_eq!(model_view.last_frame.texture_updates, 0);
    }
}
