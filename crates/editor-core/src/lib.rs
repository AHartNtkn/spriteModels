mod camera;
mod document;
mod edit;
mod fallback;
mod history;
mod io;
mod preview;
mod source;

pub use camera::OrbitCamera;
pub use document::{ActiveLayer, EditorDocument, Tool};
pub use edit::{DepthValue, ReliefValue};
pub use fallback::opposite;
pub use preview::{PreviewCache, PreviewFrame};
pub use source::{SourcePixel, SourceSprite};

use depthsprite_format::PackageError;
use relief_core::{CanonicalView, ChartError};
use relief_render::RenderError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EditorError {
    #[error(transparent)]
    Chart(#[from] ChartError),
    #[error(transparent)]
    Package(#[from] PackageError),
    #[error(transparent)]
    Render(#[from] RenderError),
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
    #[error("relief {0} is outside the paintable range 0..=254")]
    InvalidRelief(u8),
    #[error("a stroke is already active")]
    StrokeAlreadyActive,
    #[error("no stroke is active")]
    NoActiveStroke,
    #[error("document has no package path; use save_as first")]
    MissingPath,
    #[error("pixel ({x}, {y}) is outside source {view:?}")]
    PixelOutOfBounds { view: CanonicalView, x: u32, y: u32 },
}
