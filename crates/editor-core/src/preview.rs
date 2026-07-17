use relief_core::{Bounds, CanonicalView, Chart};
use relief_render::{FrameBuffer, PreparedModel, RenderRequest, render_model};

use crate::{EditorDocument, EditorError, OrbitCamera, camera::OrbitOrientation};

const FRAME_BREATHING_ROOM: u32 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PreparedKey {
    document_identity: u64,
    revision: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PreviewKey {
    document_identity: u64,
    revision: u64,
    orientation: OrbitOrientation,
}

struct PreparedFrame {
    model: PreparedModel,
    side: u32,
}

#[derive(Default)]
pub struct PreviewCache {
    prepared_key: Option<PreparedKey>,
    prepared: Option<PreparedFrame>,
    key: Option<PreviewKey>,
    framebuffer: Option<FrameBuffer>,
    generation: u64,
    #[cfg(test)]
    render_count: u64,
    #[cfg(test)]
    prepare_count: u64,
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
        let prepared_key = PreparedKey {
            document_identity,
            revision,
        };
        if self.prepared_key != Some(prepared_key) {
            let charts = document.model().resolve();
            let side = native_cell_side(document.bounds(), charts.charts());
            self.prepared = Some(PreparedFrame {
                model: PreparedModel::new(&charts),
                side,
            });
            self.prepared_key = Some(prepared_key);
            #[cfg(test)]
            {
                self.prepare_count = self
                    .prepare_count
                    .checked_add(1)
                    .expect("preview prepare count must remain monotonic");
            }
        }

        let key = PreviewKey {
            document_identity,
            revision,
            orientation: camera.orientation(),
        };
        if self.key != Some(key) {
            let prepared = self
                .prepared
                .as_ref()
                .expect("prepared_key set above implies a prepared model is stored");
            let request = RenderRequest::new(prepared.side, prepared.side, camera.target_view());
            let framebuffer = render_model(&prepared.model, &request)?;
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

fn native_cell_side(bounds: Bounds, charts: &[Chart]) -> u32 {
    let mut minimum = [0.0_f64; 3];
    let mut maximum = [
        f64::from(bounds.width()),
        f64::from(bounds.height()),
        f64::from(bounds.depth()),
    ];

    for chart in charts {
        let relief = chart
            .rgba()
            .iter()
            .filter(|pixel| pixel[3] != 0)
            .map(|pixel| f64::from(255 - pixel[3]) / 8.0)
            .fold(0.0_f64, f64::max);
        match chart.view() {
            CanonicalView::Front => maximum[2] = maximum[2].max(relief),
            CanonicalView::Back => minimum[2] = minimum[2].min(f64::from(bounds.depth()) - relief),
            CanonicalView::Left => maximum[0] = maximum[0].max(relief),
            CanonicalView::Right => minimum[0] = minimum[0].min(f64::from(bounds.width()) - relief),
            CanonicalView::Top => maximum[1] = maximum[1].max(relief),
            CanonicalView::Bottom => {
                minimum[1] = minimum[1].min(f64::from(bounds.height()) - relief);
            }
        }
    }

    let spans = std::array::from_fn::<_, 3, _>(|axis| maximum[axis] - minimum[axis]);
    let diagonal = spans.iter().map(|span| span * span).sum::<f64>().sqrt();
    (diagonal.ceil() as u32).saturating_add(FRAME_BREATHING_ROOM * 2)
}

#[cfg(test)]
mod tests {
    use relief_core::{AuthoredModel, Bounds, CanonicalView, Chart};

    use super::PreviewCache;
    use crate::{EditorDocument, OrbitCamera};

    fn document() -> EditorDocument {
        let bounds = Bounds::new(1, 1, 1).unwrap();
        let chart = Chart::from_rgba(CanonicalView::Front, 1, 1, vec![[11, 22, 33, 255]]).unwrap();
        let model = AuthoredModel::new(bounds, vec![chart]).unwrap();
        EditorDocument::from_model(model, None)
    }

    fn recolor(document: &mut EditorDocument, rgb: [u8; 3]) {
        document
            .replace_source(
                Chart::from_rgba(
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

    #[test]
    fn orbit_change_reuses_prepared_model() {
        let document = document();
        let mut camera = OrbitCamera::default();
        let mut preview = PreviewCache::default();
        preview.frame(&document, camera).unwrap();

        for (yaw, pitch) in [(9.0, 4.0), (-15.0, 6.0), (3.0, -8.0)] {
            camera.drag(yaw, pitch);
            preview.frame(&document, camera).unwrap();
        }

        assert_eq!(preview.render_count, 4);
        assert_eq!(
            preview.prepare_count, 1,
            "the prepared model is camera-independent and must survive orbiting"
        );
    }

    #[test]
    fn document_mutation_rebuilds_prepared_model() {
        let mut document = document();
        let camera = OrbitCamera::default();
        let mut preview = PreviewCache::default();
        preview.frame(&document, camera).unwrap();

        recolor(&mut document, [40, 50, 60]);
        preview.frame(&document, camera).unwrap();

        assert_eq!(
            preview.prepare_count, 2,
            "a document mutation must rebuild the prepared model, not just the frame"
        );
    }
}
