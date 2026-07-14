mod alpha;
mod chart;
mod component;
mod rational;
mod relief;
mod warp;

pub use alpha::{DecodedTexel, decode_rgba};
pub use chart::{Bounds, CanonicalView, Chart, ChartError};
pub use component::{ComponentId, ComponentMap};
pub use relief::{ForegroundCell, ReliefField};
pub use warp::{InverseWarpLine, SourcePoint, WarpCoefficients, WarpedSample};

/// Number of inverted-alpha relief units represented by one model-space pixel.
pub const RELIEF_UNITS_PER_PIXEL: i64 = 8;
