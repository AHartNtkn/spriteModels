use relief_core::{Bounds, CanonicalView};
use serde::{Deserialize, Serialize};

use crate::PackageError;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CanonicalViewName {
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
}

impl CanonicalViewName {
    pub(crate) fn entry_name(self) -> &'static str {
        match self {
            Self::Front => "views/front.png",
            Self::Back => "views/back.png",
            Self::Left => "views/left.png",
            Self::Right => "views/right.png",
            Self::Top => "views/top.png",
            Self::Bottom => "views/bottom.png",
        }
    }

    pub(crate) fn rank(self) -> u8 {
        CanonicalView::from(self).rank()
    }
}

impl From<CanonicalViewName> for CanonicalView {
    fn from(view: CanonicalViewName) -> Self {
        match view {
            CanonicalViewName::Front => Self::Front,
            CanonicalViewName::Back => Self::Back,
            CanonicalViewName::Left => Self::Left,
            CanonicalViewName::Right => Self::Right,
            CanonicalViewName::Top => Self::Top,
            CanonicalViewName::Bottom => Self::Bottom,
        }
    }
}

impl From<CanonicalView> for CanonicalViewName {
    fn from(view: CanonicalView) -> Self {
        match view {
            CanonicalView::Front => Self::Front,
            CanonicalView::Back => Self::Back,
            CanonicalView::Left => Self::Left,
            CanonicalView::Right => Self::Right,
            CanonicalView::Top => Self::Top,
            CanonicalView::Bottom => Self::Bottom,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestV1 {
    pub format: String,
    pub version: u32,
    pub bounds_pixels: [u32; 3],
    pub views: Vec<CanonicalViewName>,
}

impl ManifestV1 {
    pub(crate) fn validate(&self) -> Result<Bounds, PackageError> {
        if self.format != "depthsprite" {
            return Err(PackageError::WrongFormat(self.format.clone()));
        }
        if self.version != 1 {
            return Err(PackageError::UnsupportedVersion(self.version));
        }
        let [width, height, depth] = self.bounds_pixels;
        if width == 0 || height == 0 || depth == 0 || width > 512 || height > 512 || depth > 512 {
            return Err(PackageError::InvalidBounds(self.bounds_pixels));
        }
        if !(1..=6).contains(&self.views.len()) {
            return Err(PackageError::ViewCount(self.views.len()));
        }
        let mut unique = std::collections::HashSet::with_capacity(self.views.len());
        for view in &self.views {
            if !unique.insert(*view) {
                return Err(PackageError::DuplicateView(*view));
            }
        }
        Bounds::new(width, height, depth)
            .map_err(|_| PackageError::InvalidBounds(self.bounds_pixels))
    }
}
