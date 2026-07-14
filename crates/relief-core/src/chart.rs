use thiserror::Error;

use crate::{DecodedTexel, decode_rgba};

/// Validated, nonzero model dimensions.
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
        if width == 0 || height == 0 || depth == 0 {
            return Err(ChartError::ZeroBounds);
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

    pub fn rgba_at(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        (x < self.width && y < self.height).then(|| self.rgba[(y * self.width + x) as usize])
    }

    pub fn texel_at(&self, x: u32, y: u32) -> Option<DecodedTexel> {
        self.rgba_at(x, y).map(decode_rgba)
    }

    pub fn texels(&self) -> impl ExactSizeIterator<Item = DecodedTexel> + '_ {
        self.rgba.iter().copied().map(decode_rgba)
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ChartError {
    #[error("model bounds must be nonzero")]
    ZeroBounds,
    #[error("RGBA pixel count does not match image dimensions")]
    PixelCount,
}
