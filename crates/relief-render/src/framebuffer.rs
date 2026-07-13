use num_rational::Ratio;

use crate::RenderDiagnostic;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct FragmentKey {
    pub depth: Ratio<i64>,
    pub chart_rank: u8,
    pub source_y: u32,
    pub source_x: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameBuffer {
    width: u32,
    height: u32,
    pub(crate) keys: Vec<Option<FragmentKey>>,
    pub(crate) rgba: Vec<[u8; 4]>,
    pub(crate) diagnostics: Vec<RenderDiagnostic>,
}

impl FrameBuffer {
    pub(crate) fn transparent(width: u32, height: u32) -> Self {
        let pixel_count = (width as usize).saturating_mul(height as usize);
        Self {
            width,
            height,
            keys: vec![None; pixel_count],
            rgba: vec![[0, 0, 0, 0]; pixel_count],
            diagnostics: Vec::new(),
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &[[u8; 4]] {
        &self.rgba
    }

    pub fn diagnostics(&self) -> &[RenderDiagnostic] {
        &self.diagnostics
    }

    pub fn rgba_at(&self, x: u32, y: u32) -> [u8; 4] {
        self.rgba[(y * self.width + x) as usize]
    }
}

pub fn commit_fragment(frame: &mut FrameBuffer, x: u32, y: u32, key: FragmentKey, rgb: [u8; 3]) {
    let index = (y * frame.width() + x) as usize;
    if frame.keys[index]
        .as_ref()
        .is_none_or(|current| key < *current)
    {
        frame.keys[index] = Some(key);
        frame.rgba[index] = [rgb[0], rgb[1], rgb[2], 255];
    }
}
