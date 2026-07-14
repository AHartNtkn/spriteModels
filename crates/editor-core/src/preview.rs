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
            self.render_count = self
                .render_count
                .checked_add(1)
                .expect("preview render count must remain monotonic");
        }

        Ok(self
            .framebuffer
            .as_ref()
            .expect("a successful preview request always stores its framebuffer"))
    }

    #[doc(hidden)]
    pub fn render_count_for_test(&self) -> u64 {
        self.render_count
    }
}
