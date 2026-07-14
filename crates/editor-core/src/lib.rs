mod document;
mod fallback;
mod source;

pub use document::{ActiveLayer, EditorDocument, Tool};
pub use fallback::opposite;
pub use source::SourceSprite;

use depthsprite_format::PackageError;
use relief_core::{CanonicalView, ChartError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EditorError {
    #[error(transparent)]
    Chart(#[from] ChartError),
    #[error(transparent)]
    Package(#[from] PackageError),
    #[error("document already contains a source for {0:?}")]
    SourceAlreadyExists(CanonicalView),
    #[error("document already contains all six canonical sources")]
    SourceLimit,
    #[error("document has no authored source for {0:?}")]
    SourceNotFound(CanonicalView),
    #[error("document must retain at least one authored source")]
    LastSource,
    #[error("source {view:?} dimensions {actual:?} do not match model bounds {expected:?}")]
    DimensionMismatch {
        view: CanonicalView,
        expected: (u32, u32),
        actual: (u32, u32),
    },
}
