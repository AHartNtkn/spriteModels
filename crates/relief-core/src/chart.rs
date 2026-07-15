use thiserror::Error;

use crate::{DecodedTexel, EMPTY_RGBA, ImageEdge, ResizeDelta, decode_rgba};

/// Validated fixed-scale model dimensions.
///
/// Callers cannot bypass validation with a struct literal:
///
/// ```compile_fail
/// use relief_core::Bounds;
///
/// let _ = Bounds {
///     width: 0,
///     height: 1,
///     depth: 1,
/// };
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Bounds {
    width: u32,
    height: u32,
    depth: u32,
}

impl Bounds {
    pub fn new(width: u32, height: u32, depth: u32) -> Result<Self, ChartError> {
        if !(1..=63).contains(&width) || !(1..=63).contains(&height) || !(1..=63).contains(&depth) {
            return Err(ChartError::BoundsOutOfRange {
                width,
                height,
                depth,
            });
        }
        Ok(Self {
            width,
            height,
            depth,
        })
    }

    pub fn width(self) -> u32 {
        self.width
    }

    pub fn height(self) -> u32 {
        self.height
    }

    pub fn depth(self) -> u32 {
        self.depth
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CanonicalView {
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
}

impl CanonicalView {
    pub const fn from_rank(rank: u8) -> Option<Self> {
        match rank {
            0 => Some(Self::Front),
            1 => Some(Self::Right),
            2 => Some(Self::Back),
            3 => Some(Self::Left),
            4 => Some(Self::Top),
            5 => Some(Self::Bottom),
            _ => None,
        }
    }

    pub fn dimensions(self, bounds: Bounds) -> (u32, u32) {
        match self {
            Self::Front | Self::Back => (bounds.width(), bounds.height()),
            Self::Left | Self::Right => (bounds.depth(), bounds.height()),
            Self::Top | Self::Bottom => (bounds.width(), bounds.depth()),
        }
    }

    pub const fn rank(self) -> u8 {
        match self {
            Self::Front => 0,
            Self::Right => 1,
            Self::Back => 2,
            Self::Left => 3,
            Self::Top => 4,
            Self::Bottom => 5,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Chart {
    view: CanonicalView,
    supplies_opposite: bool,
    mirrors_opposite: bool,
    width: u32,
    height: u32,
    rgba: Vec<[u8; 4]>,
}

impl Chart {
    pub fn from_rgba(
        view: CanonicalView,
        width: u32,
        height: u32,
        rgba: Vec<[u8; 4]>,
    ) -> Result<Self, ChartError> {
        if rgba.len() != (width as usize) * (height as usize) {
            return Err(ChartError::PixelCount);
        }
        Ok(Self {
            view,
            supplies_opposite: false,
            mirrors_opposite: false,
            width,
            height,
            rgba,
        })
    }

    pub fn view(&self) -> CanonicalView {
        self.view
    }

    pub fn with_opposite_assignment(mut self) -> Self {
        self.supplies_opposite = true;
        self
    }

    pub fn without_opposite_assignment(mut self) -> Self {
        self.supplies_opposite = false;
        self
    }

    pub fn supplies_opposite(&self) -> bool {
        self.supplies_opposite
    }

    pub fn with_mirrored_opposite(mut self) -> Self {
        self.mirrors_opposite = true;
        self
    }

    pub fn without_mirrored_opposite(mut self) -> Self {
        self.mirrors_opposite = false;
        self
    }

    pub fn mirrors_opposite(&self) -> bool {
        self.mirrors_opposite
    }

    pub(crate) fn with_assignments_from(mut self, source: &Self) -> Self {
        self.supplies_opposite = source.supplies_opposite;
        self.mirrors_opposite = source.mirrors_opposite;
        self
    }

    pub fn assigned_views(&self) -> impl Iterator<Item = CanonicalView> {
        [
            Some(self.view),
            self.supplies_opposite.then(|| self.view.opposite()),
        ]
        .into_iter()
        .flatten()
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn rgba(&self) -> &[[u8; 4]] {
        &self.rgba
    }

    pub fn rgba_at(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        (x < self.width && y < self.height).then(|| self.rgba[(y * self.width + x) as usize])
    }

    pub fn texel_at(&self, x: u32, y: u32) -> Option<DecodedTexel> {
        self.rgba_at(x, y).map(decode_rgba)
    }

    pub fn texels(&self) -> impl ExactSizeIterator<Item = DecodedTexel> + '_ {
        self.rgba.iter().copied().map(decode_rgba)
    }

    pub(crate) fn edge_contains_authored_pixel(&self, edge: ImageEdge) -> bool {
        match edge {
            ImageEdge::Left => (0..self.height).any(|y| self.rgba_at(0, y) != Some(EMPTY_RGBA)),
            ImageEdge::Right => {
                (0..self.height).any(|y| self.rgba_at(self.width - 1, y) != Some(EMPTY_RGBA))
            }
            ImageEdge::Top => (0..self.width).any(|x| self.rgba_at(x, 0) != Some(EMPTY_RGBA)),
            ImageEdge::Bottom => {
                (0..self.width).any(|x| self.rgba_at(x, self.height - 1) != Some(EMPTY_RGBA))
            }
        }
    }

    pub(crate) fn resized(&self, edge: ImageEdge, delta: ResizeDelta) -> Self {
        let (width, height) = match (edge, delta) {
            (ImageEdge::Left | ImageEdge::Right, ResizeDelta::Add) => (self.width + 1, self.height),
            (ImageEdge::Left | ImageEdge::Right, ResizeDelta::Remove) => {
                (self.width - 1, self.height)
            }
            (ImageEdge::Top | ImageEdge::Bottom, ResizeDelta::Add) => (self.width, self.height + 1),
            (ImageEdge::Top | ImageEdge::Bottom, ResizeDelta::Remove) => {
                (self.width, self.height - 1)
            }
        };
        let mut rgba = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                let source = match (edge, delta) {
                    (ImageEdge::Left, ResizeDelta::Add) => x.checked_sub(1).map(|x| (x, y)),
                    (ImageEdge::Right, ResizeDelta::Add) => (x < self.width).then_some((x, y)),
                    (ImageEdge::Top, ResizeDelta::Add) => y.checked_sub(1).map(|y| (x, y)),
                    (ImageEdge::Bottom, ResizeDelta::Add) => (y < self.height).then_some((x, y)),
                    (ImageEdge::Left, ResizeDelta::Remove) => Some((x + 1, y)),
                    (ImageEdge::Right, ResizeDelta::Remove) => Some((x, y)),
                    (ImageEdge::Top, ResizeDelta::Remove) => Some((x, y + 1)),
                    (ImageEdge::Bottom, ResizeDelta::Remove) => Some((x, y)),
                };
                rgba.push(
                    source
                        .and_then(|(x, y)| self.rgba_at(x, y))
                        .unwrap_or(EMPTY_RGBA),
                );
            }
        }
        Self {
            view: self.view,
            supplies_opposite: self.supplies_opposite,
            mirrors_opposite: self.mirrors_opposite,
            width,
            height,
            rgba,
        }
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ChartError {
    #[error("model bounds must be in 1..=63, got ({width}, {height}, {depth})")]
    BoundsOutOfRange { width: u32, height: u32, depth: u32 },
    #[error("RGBA pixel count does not match image dimensions")]
    PixelCount,
}
