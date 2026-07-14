use relief_core::{Bounds, CanonicalView, Chart};

use crate::EditorError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSprite {
    view: CanonicalView,
    width: u32,
    height: u32,
    rgba: Vec<[u8; 4]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourcePixel {
    rgb: [u8; 3],
    alpha: u8,
}

impl SourcePixel {
    pub const fn rgb(self) -> [u8; 3] {
        self.rgb
    }

    pub const fn alpha(self) -> u8 {
        self.alpha
    }
}

impl SourceSprite {
    pub fn from_rgba(
        view: CanonicalView,
        width: u32,
        height: u32,
        rgba: Vec<[u8; 4]>,
    ) -> Result<Self, EditorError> {
        Chart::from_rgba(view, width, height, rgba.clone())?;
        Ok(Self {
            view,
            width,
            height,
            rgba,
        })
    }

    pub fn view(&self) -> CanonicalView {
        self.view
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn rgba(&self) -> &[[u8; 4]] {
        &self.rgba
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<SourcePixel> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let raw = self.rgba[(y as usize) * (self.width as usize) + (x as usize)];
        Some(SourcePixel {
            rgb: [raw[0], raw[1], raw[2]],
            alpha: raw[3],
        })
    }

    pub(crate) fn empty(view: CanonicalView, bounds: Bounds) -> Self {
        let (width, height) = view.dimensions(bounds);
        let pixel_count = (width as usize)
            .checked_mul(height as usize)
            .expect("canonical source dimensions must fit the address space");
        Self {
            view,
            width,
            height,
            rgba: vec![[0, 0, 0, 0]; pixel_count],
        }
    }

    pub(crate) fn from_chart(chart: &Chart) -> Self {
        let (width, height) = chart.dimensions();
        Self {
            view: chart.view(),
            width,
            height,
            rgba: chart.rgba().to_vec(),
        }
    }

    pub(crate) fn to_chart(&self) -> Result<Chart, EditorError> {
        self.to_chart_for(self.view)
    }

    pub(crate) fn to_chart_for(&self, view: CanonicalView) -> Result<Chart, EditorError> {
        Ok(Chart::from_rgba(
            view,
            self.width,
            self.height,
            self.rgba.clone(),
        )?)
    }
}
