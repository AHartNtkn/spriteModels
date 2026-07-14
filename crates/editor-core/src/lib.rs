mod camera;
mod document;
mod edit;
mod history;
mod io;
mod preview;

pub use camera::OrbitCamera;
pub use document::{ActiveLayer, EditorDocument, Tool};
pub use edit::{DepthValue, ReliefValue};
pub use preview::{PreviewCache, PreviewFrame};

use depthsprite_format::PackageError;
use relief_core::{CanonicalView, ModelError};
use relief_render::RenderError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EditorError {
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error(transparent)]
    Package(#[from] PackageError),
    #[error(transparent)]
    Render(#[from] RenderError),
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
