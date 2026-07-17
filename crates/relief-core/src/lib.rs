mod alpha;
mod chart;
mod component;
mod frame;
mod model;
mod rational;
mod relief;
mod resize;
mod warp;

pub use alpha::{DecodedTexel, decode_rgba};
pub use chart::{Bounds, CanonicalView, Chart, ChartError};
pub use component::{ComponentId, ComponentMap};
pub use frame::CanonicalFrame;
pub use model::{AuthoredModel, EMPTY_RGBA, ModelError, ResolvedCharts};
pub use relief::{ForegroundCell, ReliefField};
pub use resize::{
    AxisSide, ChartEdge, DiscardPolicy, ImageEdge, ReassignMode, ResizeDelta, ResizeRequest,
    WorldAxis, WorldEdge,
};
pub use warp::{FrameInverse, PreparedInverse, SourcePoint, WarpCoefficients, WarpedSample};

/// Number of inverted-alpha relief units represented by one model-space pixel.
pub const RELIEF_UNITS_PER_PIXEL: i64 = 8;
