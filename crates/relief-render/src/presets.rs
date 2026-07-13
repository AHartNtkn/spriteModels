use num_rational::Ratio;
use relief_core::{Bounds, CanonicalView, WarpCoefficients};

type Vector = [Ratio<i64>; 3];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CameraBasis {
    screen_right: Vector,
    screen_down: Vector,
    depth: Vector,
}

impl CameraBasis {
    pub fn new(screen_right: Vector, screen_down: Vector, depth: Vector) -> Self {
        Self {
            screen_right,
            screen_down,
            depth,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ProjectionSource {
    Camera(CameraBasis),
    IdentityAllCharts,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetView {
    preset_version: u32,
    source: ProjectionSource,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TargetExtents {
    pub(crate) min_x: Ratio<i64>,
    pub(crate) max_x: Ratio<i64>,
    pub(crate) min_y: Ratio<i64>,
    pub(crate) max_y: Ratio<i64>,
}

impl TargetView {
    pub const PRESET_VERSION: u32 = 1;

    pub fn from_camera(camera: CameraBasis) -> Self {
        Self {
            preset_version: Self::PRESET_VERSION,
            source: ProjectionSource::Camera(camera),
        }
    }

    pub fn front_v1() -> Self {
        Self::from_camera(CameraBasis::new(
            integer_vector(1, 0, 0),
            integer_vector(0, 1, 0),
            integer_vector(0, 0, 1),
        ))
    }

    pub fn right_v1() -> Self {
        Self::from_camera(CameraBasis::new(
            integer_vector(0, 0, -1),
            integer_vector(0, 1, 0),
            integer_vector(-1, 0, 0),
        ))
    }

    pub fn top_v1() -> Self {
        Self::from_camera(CameraBasis::new(
            integer_vector(1, 0, 0),
            integer_vector(0, 0, 1),
            integer_vector(0, 1, 0),
        ))
    }

    pub fn isometric_v1() -> Self {
        Self::from_camera(CameraBasis::new(
            [Ratio::new(1, 2), ratio_zero(), Ratio::new(1, 2)],
            [Ratio::new(1, 4), Ratio::new(1, 2), Ratio::new(-1, 4)],
            [Ratio::new(-1, 3), Ratio::new(1, 3), Ratio::new(1, 3)],
        ))
    }

    pub fn bowl_acceptance() -> Self {
        Self::isometric_v1()
    }

    pub fn preset_version(&self) -> u32 {
        self.preset_version
    }

    pub fn is_front_facing(&self, view: CanonicalView) -> bool {
        match &self.source {
            ProjectionSource::Camera(camera) => {
                dot(&camera.depth, &inward_axis(view)) > ratio_zero()
            }
            ProjectionSource::IdentityAllCharts => true,
        }
    }

    pub fn warp_coefficients(
        &self,
        view: CanonicalView,
        bounds: Bounds,
    ) -> Option<WarpCoefficients> {
        match &self.source {
            ProjectionSource::Camera(camera) => {
                let frame = chart_frame(view, bounds);
                if dot(&camera.depth, &frame.inward) <= ratio_zero() {
                    return None;
                }
                Some(compose(camera, &frame))
            }
            ProjectionSource::IdentityAllCharts => Some(WarpCoefficients::new(
                [[1, 0, 0], [0, 1, 0]],
                [0, 0],
                [0, 0, 0],
                0,
            )),
        }
    }

    pub fn front_for_test() -> Self {
        Self {
            preset_version: Self::PRESET_VERSION,
            source: ProjectionSource::IdentityAllCharts,
        }
    }

    pub fn back_of_front_for_test() -> Self {
        Self::from_camera(CameraBasis::new(
            integer_vector(1, 0, 0),
            integer_vector(0, 1, 0),
            integer_vector(0, 0, -1),
        ))
    }

    pub(crate) fn framing_extents(&self, bounds: Bounds) -> TargetExtents {
        match &self.source {
            ProjectionSource::Camera(camera) => camera_extents(camera, bounds),
            ProjectionSource::IdentityAllCharts => TargetExtents {
                min_x: ratio_zero(),
                max_x: Ratio::from_integer(i64::from(bounds.width())),
                min_y: ratio_zero(),
                max_y: Ratio::from_integer(i64::from(bounds.height())),
            },
        }
    }
}

#[derive(Clone, Debug)]
struct ChartFrame {
    origin: Vector,
    source_x: Vector,
    source_y: Vector,
    inward: Vector,
}

fn chart_frame(view: CanonicalView, bounds: Bounds) -> ChartFrame {
    let width = Ratio::from_integer(i64::from(bounds.width()));
    let height = Ratio::from_integer(i64::from(bounds.height()));
    let depth = Ratio::from_integer(i64::from(bounds.depth()));
    let zero = ratio_zero();

    match view {
        CanonicalView::Front => ChartFrame {
            origin: [zero, zero, zero],
            source_x: integer_vector(1, 0, 0),
            source_y: integer_vector(0, 1, 0),
            inward: inward_axis(view),
        },
        CanonicalView::Back => ChartFrame {
            origin: [width, zero, depth],
            source_x: integer_vector(-1, 0, 0),
            source_y: integer_vector(0, 1, 0),
            inward: inward_axis(view),
        },
        CanonicalView::Left => ChartFrame {
            origin: [zero, zero, zero],
            source_x: integer_vector(0, 0, 1),
            source_y: integer_vector(0, 1, 0),
            inward: inward_axis(view),
        },
        CanonicalView::Right => ChartFrame {
            origin: [width, zero, depth],
            source_x: integer_vector(0, 0, -1),
            source_y: integer_vector(0, 1, 0),
            inward: inward_axis(view),
        },
        CanonicalView::Top => ChartFrame {
            origin: [zero, zero, zero],
            source_x: integer_vector(1, 0, 0),
            source_y: integer_vector(0, 0, 1),
            inward: inward_axis(view),
        },
        CanonicalView::Bottom => ChartFrame {
            origin: [zero, height, depth],
            source_x: integer_vector(1, 0, 0),
            source_y: integer_vector(0, 0, -1),
            inward: inward_axis(view),
        },
    }
}

fn inward_axis(view: CanonicalView) -> Vector {
    match view {
        CanonicalView::Front => integer_vector(0, 0, 1),
        CanonicalView::Back => integer_vector(0, 0, -1),
        CanonicalView::Left => integer_vector(1, 0, 0),
        CanonicalView::Right => integer_vector(-1, 0, 0),
        CanonicalView::Top => integer_vector(0, 1, 0),
        CanonicalView::Bottom => integer_vector(0, -1, 0),
    }
}

fn compose(camera: &CameraBasis, frame: &ChartFrame) -> WarpCoefficients {
    let eighth = Ratio::new(1, 8);
    WarpCoefficients::from_rational(
        [
            [
                dot(&camera.screen_right, &frame.source_x),
                dot(&camera.screen_right, &frame.source_y),
                dot(&camera.screen_right, &frame.origin),
            ],
            [
                dot(&camera.screen_down, &frame.source_x),
                dot(&camera.screen_down, &frame.source_y),
                dot(&camera.screen_down, &frame.origin),
            ],
        ],
        [
            dot(&camera.screen_right, &frame.inward) * eighth,
            dot(&camera.screen_down, &frame.inward) * eighth,
        ],
        [
            dot(&camera.depth, &frame.source_x),
            dot(&camera.depth, &frame.source_y),
            dot(&camera.depth, &frame.origin),
        ],
        dot(&camera.depth, &frame.inward) * eighth,
    )
}

fn camera_extents(camera: &CameraBasis, bounds: Bounds) -> TargetExtents {
    let axes = [
        [ratio_zero(), Ratio::from_integer(i64::from(bounds.width()))],
        [
            ratio_zero(),
            Ratio::from_integer(i64::from(bounds.height())),
        ],
        [ratio_zero(), Ratio::from_integer(i64::from(bounds.depth()))],
    ];
    let first = [axes[0][0], axes[1][0], axes[2][0]];
    let first_x = dot(&camera.screen_right, &first);
    let first_y = dot(&camera.screen_down, &first);
    let mut extents = TargetExtents {
        min_x: first_x,
        max_x: first_x,
        min_y: first_y,
        max_y: first_y,
    };

    for x in axes[0] {
        for y in axes[1] {
            for z in axes[2] {
                let corner = [x, y, z];
                let screen_x = dot(&camera.screen_right, &corner);
                let screen_y = dot(&camera.screen_down, &corner);
                extents.min_x = extents.min_x.min(screen_x);
                extents.max_x = extents.max_x.max(screen_x);
                extents.min_y = extents.min_y.min(screen_y);
                extents.max_y = extents.max_y.max(screen_y);
            }
        }
    }

    extents
}

fn dot(first: &Vector, second: &Vector) -> Ratio<i64> {
    first
        .iter()
        .zip(second)
        .fold(ratio_zero(), |sum, (left, right)| sum + *left * *right)
}

fn integer_vector(x: i64, y: i64, z: i64) -> Vector {
    [
        Ratio::from_integer(x),
        Ratio::from_integer(y),
        Ratio::from_integer(z),
    ]
}

fn ratio_zero() -> Ratio<i64> {
    Ratio::from_integer(0)
}
