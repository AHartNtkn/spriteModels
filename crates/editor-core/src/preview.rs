use relief_render::{FrameBuffer, RenderRequest, render_model};

use crate::{EditorDocument, EditorError, OrbitCamera, camera::OrbitOrientation};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PreviewKey {
    document_identity: u64,
    revision: u64,
    orientation: OrbitOrientation,
}

#[derive(Default)]
pub struct PreviewCache {
    key: Option<PreviewKey>,
    framebuffer: Option<FrameBuffer>,
    generation: u64,
    #[cfg(test)]
    render_count: u64,
}

pub struct PreviewFrame<'a> {
    framebuffer: &'a FrameBuffer,
    generation: u64,
}

impl PreviewFrame<'_> {
    pub fn framebuffer(&self) -> &FrameBuffer {
        self.framebuffer
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

impl PreviewCache {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn frame(
        &mut self,
        document: &EditorDocument,
        camera: OrbitCamera,
    ) -> Result<PreviewFrame<'_>, EditorError> {
        let (document_identity, revision) = document.render_key();
        let key = PreviewKey {
            document_identity,
            revision,
            orientation: camera.orientation(),
        };
        if self.key != Some(key) {
            let charts = document.resolved_charts()?;
            let side = native_cell_side(document.bounds());
            let request = RenderRequest::new(side, side, camera.target_view());
            let framebuffer = render_model(document.bounds(), &charts, &request)?;
            self.generation = self
                .generation
                .checked_add(1)
                .expect("preview generation must remain monotonic");
            self.framebuffer = Some(framebuffer);
            self.key = Some(key);
            #[cfg(test)]
            {
                self.render_count = self
                    .render_count
                    .checked_add(1)
                    .expect("preview render count must remain monotonic");
            }
        }

        Ok(PreviewFrame {
            framebuffer: self
                .framebuffer
                .as_ref()
                .expect("a successful preview request always stores its framebuffer"),
            generation: self.generation,
        })
    }
}

fn native_cell_side(bounds: relief_core::Bounds) -> u32 {
    bounds
        .width()
        .saturating_add(bounds.height())
        .saturating_add(bounds.depth())
        .saturating_add(4)
}

#[cfg(test)]
mod tests {
    use depthsprite_format::DepthSpriteModel;
    use relief_core::{Bounds, CanonicalView, Chart};

    use super::PreviewCache;
    use crate::{EditorDocument, OrbitCamera, SourceSprite};

    fn document() -> EditorDocument {
        let bounds = Bounds::new(1, 1, 1).unwrap();
        let chart = Chart::from_rgba(CanonicalView::Front, 1, 1, vec![[11, 22, 33, 255]]).unwrap();
        let model = DepthSpriteModel::new(bounds, vec![chart]).unwrap();
        EditorDocument::from_model(model, None).unwrap()
    }

    fn recolor(document: &mut EditorDocument, rgb: [u8; 3]) {
        document
            .replace_source(
                SourceSprite::from_rgba(
                    CanonicalView::Front,
                    1,
                    1,
                    vec![[rgb[0], rgb[1], rgb[2], 255]],
                )
                .unwrap(),
            )
            .unwrap();
    }

    #[test]
    fn unchanged_key_renders_once() {
        let document = document();
        let camera = OrbitCamera::default();
        let mut preview = PreviewCache::default();

        preview.frame(&document, camera).unwrap();
        preview.frame(&document, camera).unwrap();

        assert_eq!(preview.render_count, 1);
    }

    #[test]
    fn several_document_mutations_before_request_render_once() {
        let mut document = document();
        let camera = OrbitCamera::default();
        let mut preview = PreviewCache::default();
        preview.frame(&document, camera).unwrap();

        for rgb in [[40, 50, 60], [70, 80, 90], [100, 110, 120]] {
            recolor(&mut document, rgb);
        }
        preview.frame(&document, camera).unwrap();
        preview.frame(&document, camera).unwrap();

        assert_eq!(document.revision(), 3);
        assert_eq!(preview.render_count, 2);
    }

    #[test]
    fn orbit_change_renders_once() {
        let document = document();
        let mut camera = OrbitCamera::default();
        let mut preview = PreviewCache::default();
        preview.frame(&document, camera).unwrap();

        camera.drag(9.0, 4.0);
        preview.frame(&document, camera).unwrap();
        preview.frame(&document, camera).unwrap();

        assert_eq!(preview.render_count, 2);
    }
}
