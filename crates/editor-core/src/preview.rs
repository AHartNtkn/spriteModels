use relief_render::{FrameBuffer, RenderRequest, render_model};

use crate::{EditorDocument, EditorError, OrbitCamera};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PreviewKey {
    revision: u64,
    camera: OrbitCamera,
    width: u32,
    height: u32,
}

#[derive(Default)]
pub struct PreviewCache {
    key: Option<PreviewKey>,
    framebuffer: Option<FrameBuffer>,
    #[cfg(test)]
    render_count: u64,
}

impl PreviewCache {
    pub fn frame(
        &mut self,
        document: &EditorDocument,
        camera: OrbitCamera,
        width: u32,
        height: u32,
    ) -> Result<&FrameBuffer, EditorError> {
        let key = PreviewKey {
            revision: document.revision(),
            camera,
            width,
            height,
        };
        if self.key != Some(key) {
            let charts = document.resolved_charts()?;
            let request = RenderRequest::new(width, height, camera.target_view());
            let framebuffer = render_model(document.bounds(), &charts, &request)?;
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

        Ok(self
            .framebuffer
            .as_ref()
            .expect("a successful preview request always stores its framebuffer"))
    }
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

        preview.frame(&document, camera, 48, 32).unwrap();
        preview.frame(&document, camera, 48, 32).unwrap();

        assert_eq!(preview.render_count, 1);
    }

    #[test]
    fn several_document_mutations_before_request_render_once() {
        let mut document = document();
        let camera = OrbitCamera::default();
        let mut preview = PreviewCache::default();
        preview.frame(&document, camera, 48, 32).unwrap();

        for rgb in [[40, 50, 60], [70, 80, 90], [100, 110, 120]] {
            recolor(&mut document, rgb);
        }
        preview.frame(&document, camera, 48, 32).unwrap();
        preview.frame(&document, camera, 48, 32).unwrap();

        assert_eq!(document.revision(), 3);
        assert_eq!(preview.render_count, 2);
    }

    #[test]
    fn orbit_change_renders_once() {
        let document = document();
        let mut camera = OrbitCamera::default();
        let mut preview = PreviewCache::default();
        preview.frame(&document, camera, 48, 32).unwrap();

        camera.drag(9.0, 4.0);
        preview.frame(&document, camera, 48, 32).unwrap();
        preview.frame(&document, camera, 48, 32).unwrap();

        assert_eq!(preview.render_count, 2);
    }
}
