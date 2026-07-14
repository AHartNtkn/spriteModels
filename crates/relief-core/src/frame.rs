use crate::{Bounds, CanonicalView};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CanonicalFrame {
    pub origin: [i64; 3],
    pub source_u: [i64; 3],
    pub source_v: [i64; 3],
    pub inward: [i64; 3],
}

impl CanonicalView {
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
