use crate::CanonicalView;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageEdge {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorldAxis {
    X,
    Y,
    Z,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AxisSide {
    Min,
    Max,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorldEdge {
    pub axis: WorldAxis,
    pub side: AxisSide,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResizeDelta {
    Add,
    Remove,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResizeRequest {
    pub view: CanonicalView,
    pub edge: ImageEdge,
    pub delta: ResizeDelta,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscardPolicy {
    Reject,
    Allow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChartEdge {
    pub view: CanonicalView,
    pub edge: ImageEdge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReassignMode {
    Preserve,
    RecreateEmpty,
}
