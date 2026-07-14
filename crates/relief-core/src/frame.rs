use crate::{AxisSide, Bounds, CanonicalView, ImageEdge, WorldAxis, WorldEdge};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CanonicalFrame {
    pub origin: [i64; 3],
    pub source_u: [i64; 3],
    pub source_v: [i64; 3],
    pub inward: [i64; 3],
}

impl CanonicalView {
    pub const fn world_edge(self, edge: ImageEdge) -> WorldEdge {
        match (self, edge) {
            (Self::Front, ImageEdge::Left)
            | (Self::Top, ImageEdge::Left)
            | (Self::Bottom, ImageEdge::Left) => WorldEdge {
                axis: WorldAxis::X,
                side: AxisSide::Min,
            },
            (Self::Front, ImageEdge::Right)
            | (Self::Top, ImageEdge::Right)
            | (Self::Bottom, ImageEdge::Right) => WorldEdge {
                axis: WorldAxis::X,
                side: AxisSide::Max,
            },
            (Self::Back, ImageEdge::Left) => WorldEdge {
                axis: WorldAxis::X,
                side: AxisSide::Max,
            },
            (Self::Back, ImageEdge::Right) => WorldEdge {
                axis: WorldAxis::X,
                side: AxisSide::Min,
            },
            (Self::Left, ImageEdge::Left) => WorldEdge {
                axis: WorldAxis::Z,
                side: AxisSide::Min,
            },
            (Self::Left, ImageEdge::Right) => WorldEdge {
                axis: WorldAxis::Z,
                side: AxisSide::Max,
            },
            (Self::Right, ImageEdge::Left) => WorldEdge {
                axis: WorldAxis::Z,
                side: AxisSide::Max,
            },
            (Self::Right, ImageEdge::Right) => WorldEdge {
                axis: WorldAxis::Z,
                side: AxisSide::Min,
            },
            (Self::Top, ImageEdge::Top) => WorldEdge {
                axis: WorldAxis::Z,
                side: AxisSide::Min,
            },
            (Self::Top, ImageEdge::Bottom) => WorldEdge {
                axis: WorldAxis::Z,
                side: AxisSide::Max,
            },
            (Self::Bottom, ImageEdge::Top) => WorldEdge {
                axis: WorldAxis::Z,
                side: AxisSide::Max,
            },
            (Self::Bottom, ImageEdge::Bottom) => WorldEdge {
                axis: WorldAxis::Z,
                side: AxisSide::Min,
            },
            (Self::Front | Self::Back | Self::Left | Self::Right, ImageEdge::Top) => WorldEdge {
                axis: WorldAxis::Y,
                side: AxisSide::Min,
            },
            (Self::Front | Self::Back | Self::Left | Self::Right, ImageEdge::Bottom) => WorldEdge {
                axis: WorldAxis::Y,
                side: AxisSide::Max,
            },
        }
    }

    pub const fn image_edge(self, edge: WorldEdge) -> Option<ImageEdge> {
        match (self, edge.axis, edge.side) {
            (Self::Front | Self::Top | Self::Bottom, WorldAxis::X, AxisSide::Min) => {
                Some(ImageEdge::Left)
            }
            (Self::Front | Self::Top | Self::Bottom, WorldAxis::X, AxisSide::Max) => {
                Some(ImageEdge::Right)
            }
            (Self::Back, WorldAxis::X, AxisSide::Min) => Some(ImageEdge::Right),
            (Self::Back, WorldAxis::X, AxisSide::Max) => Some(ImageEdge::Left),
            (Self::Left, WorldAxis::Z, AxisSide::Min) => Some(ImageEdge::Left),
            (Self::Left, WorldAxis::Z, AxisSide::Max) => Some(ImageEdge::Right),
            (Self::Right, WorldAxis::Z, AxisSide::Min) => Some(ImageEdge::Right),
            (Self::Right, WorldAxis::Z, AxisSide::Max) => Some(ImageEdge::Left),
            (Self::Top, WorldAxis::Z, AxisSide::Min) => Some(ImageEdge::Top),
            (Self::Top, WorldAxis::Z, AxisSide::Max) => Some(ImageEdge::Bottom),
            (Self::Bottom, WorldAxis::Z, AxisSide::Min) => Some(ImageEdge::Bottom),
            (Self::Bottom, WorldAxis::Z, AxisSide::Max) => Some(ImageEdge::Top),
            (Self::Front | Self::Back | Self::Left | Self::Right, WorldAxis::Y, AxisSide::Min) => {
                Some(ImageEdge::Top)
            }
            (Self::Front | Self::Back | Self::Left | Self::Right, WorldAxis::Y, AxisSide::Max) => {
                Some(ImageEdge::Bottom)
            }
            _ => None,
        }
    }

    pub const fn opposite(self) -> Self {
        match self {
            Self::Front => Self::Back,
            Self::Back => Self::Front,
            Self::Left => Self::Right,
            Self::Right => Self::Left,
            Self::Top => Self::Bottom,
            Self::Bottom => Self::Top,
        }
    }

    pub fn maximum_inward_depth(self, bounds: Bounds) -> u8 {
        let opposing_axis = match self {
            Self::Front | Self::Back => bounds.depth(),
            Self::Left | Self::Right => bounds.width(),
            Self::Top | Self::Bottom => bounds.height(),
        };
        (opposing_axis * 4) as u8
    }

    pub fn frame(self, bounds: Bounds) -> CanonicalFrame {
        let width = i64::from(bounds.width());
        let height = i64::from(bounds.height());
        let depth = i64::from(bounds.depth());

        match self {
            Self::Front => CanonicalFrame {
                origin: [0, 0, 0],
                source_u: [1, 0, 0],
                source_v: [0, 1, 0],
                inward: [0, 0, 1],
            },
            Self::Back => CanonicalFrame {
                origin: [width, 0, depth],
                source_u: [-1, 0, 0],
                source_v: [0, 1, 0],
                inward: [0, 0, -1],
            },
            Self::Left => CanonicalFrame {
                origin: [0, 0, 0],
                source_u: [0, 0, 1],
                source_v: [0, 1, 0],
                inward: [1, 0, 0],
            },
            Self::Right => CanonicalFrame {
                origin: [width, 0, depth],
                source_u: [0, 0, -1],
                source_v: [0, 1, 0],
                inward: [-1, 0, 0],
            },
            Self::Top => CanonicalFrame {
                origin: [0, 0, 0],
                source_u: [1, 0, 0],
                source_v: [0, 0, 1],
                inward: [0, 1, 0],
            },
            Self::Bottom => CanonicalFrame {
                origin: [0, height, depth],
                source_u: [1, 0, 0],
                source_v: [0, 0, -1],
                inward: [0, -1, 0],
            },
        }
    }
}
