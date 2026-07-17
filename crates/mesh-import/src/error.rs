use relief_core::{CanonicalView, ModelError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ImportError {
    #[error("could not load glTF: {0}")]
    Gltf(#[from] gltf::Error),
    #[error("the scene contains no triangle geometry")]
    NoTriangles,
    #[error("no side is set to Capture")]
    NoCaptureSides,
    #[error("{side:?} is supplied by its opposite, but {opposite:?} is not captured")]
    UnsatisfiedOpposite {
        side: CanonicalView,
        opposite: CanonicalView,
    },
    #[error("longest axis {0} is outside 1..=63")]
    LongestAxisRange(u32),
    #[error(transparent)]
    Chart(#[from] relief_core::ChartError),
    #[error(transparent)]
    Model(#[from] ModelError),
}
