mod error;
mod load;
mod manifest;
mod save;

pub use error::PackageError;
pub use load::{MAX_ARCHIVE_SIZE, MAX_COMPRESSED_SIZE, load_path, load_reader};
pub use manifest::{CanonicalViewName, ManifestV1};
pub use save::{save_path_atomic, save_writer};

use std::collections::HashSet;

use relief_core::{Bounds, Chart};

const MAX_BOUND: u32 = 512;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DepthSpriteModel {
    bounds: Bounds,
    charts: Vec<Chart>,
}

impl DepthSpriteModel {
    pub fn new(bounds: Bounds, mut charts: Vec<Chart>) -> Result<Self, PackageError> {
        validate_bounds(bounds)?;
        if charts.is_empty() {
            return Err(PackageError::EmptyModel);
        }
        if charts.len() > 6 {
            return Err(PackageError::ViewCount(charts.len()));
        }

        let mut views = HashSet::with_capacity(charts.len());
        for chart in &charts {
            if chart.bounds() != bounds {
                return Err(PackageError::MixedBounds {
                    expected: bounds,
                    actual: chart.bounds(),
                });
            }
            if !views.insert(chart.view()) {
                return Err(PackageError::DuplicateView(chart.view().into()));
            }
        }
        charts.sort_by_key(|chart| chart.view().rank());
        Ok(Self { bounds, charts })
    }

    pub fn bounds(&self) -> Bounds {
        self.bounds
    }

    pub fn charts(&self) -> &[Chart] {
        &self.charts
    }
}

pub(crate) fn validate_bounds(bounds: Bounds) -> Result<(), PackageError> {
    let dimensions = [bounds.width(), bounds.height(), bounds.depth()];
    if dimensions.into_iter().any(|value| value > MAX_BOUND) {
        return Err(PackageError::InvalidBounds(dimensions));
    }
    Ok(())
}
