use num_rational::Ratio;
use relief_core::{
    Bounds, CanonicalFrame, CanonicalView, RELIEF_UNITS_PER_PIXEL, WarpCoefficients,
};

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
pub struct TargetView {
    camera: CameraBasis,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TargetExtents {
    pub(crate) min_x: Ratio<i64>,
    pub(crate) max_x: Ratio<i64>,
    pub(crate) min_y: Ratio<i64>,
    pub(crate) max_y: Ratio<i64>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FacingCoefficients {
    constant: f64,
    relief_x: f64,
    relief_y: f64,
}

impl FacingCoefficients {
    pub(crate) fn evaluate(self, relief_x: f64, relief_y: f64) -> f64 {
        self.constant + self.relief_x * relief_x + self.relief_y * relief_y
    }
}

impl TargetView {
    pub fn from_camera(camera: CameraBasis) -> Self {
        Self { camera }
    }

    pub fn front() -> Self {
        Self::from_camera(CameraBasis::new(
            integer_vector(1, 0, 0),
            integer_vector(0, 1, 0),
            integer_vector(0, 0, 1),
        ))
    }

    pub fn right() -> Self {
        Self::from_camera(CameraBasis::new(
            integer_vector(0, 0, -1),
            integer_vector(0, 1, 0),
            integer_vector(-1, 0, 0),
        ))
    }

    pub fn back() -> Self {
        Self::from_camera(CameraBasis::new(
            integer_vector(-1, 0, 0),
            integer_vector(0, 1, 0),
            integer_vector(0, 0, -1),
        ))
    }

    pub fn left() -> Self {
        Self::from_camera(CameraBasis::new(
            integer_vector(0, 0, 1),
            integer_vector(0, 1, 0),
            integer_vector(1, 0, 0),
        ))
    }

    pub fn top() -> Self {
        Self::from_camera(CameraBasis::new(
            integer_vector(1, 0, 0),
            integer_vector(0, 0, 1),
            integer_vector(0, 1, 0),
        ))
    }

    pub fn bottom() -> Self {
        Self::from_camera(CameraBasis::new(
            integer_vector(1, 0, 0),
            integer_vector(0, 0, -1),
            integer_vector(0, -1, 0),
        ))
    }

    pub fn isometric() -> Self {
        Self::from_camera(CameraBasis::new(
            [Ratio::new(1, 2), ratio_zero(), Ratio::new(1, 2)],
            [Ratio::new(1, 4), Ratio::new(1, 2), Ratio::new(-1, 4)],
            [Ratio::new(-1, 3), Ratio::new(1, 3), Ratio::new(1, 3)],
        ))
    }

    pub fn bowl_acceptance() -> Self {
        Self::from_camera(CameraBasis::new(
            [Ratio::new(1, 2), ratio_zero(), Ratio::new(1, 2)],
            [
                Ratio::from_integer(1),
                Ratio::new(1, 2),
                Ratio::from_integer(-1),
            ],
            integer_vector(-1, 4, 1),
        ))
    }

    pub fn warp_coefficients(&self, view: CanonicalView, bounds: Bounds) -> WarpCoefficients {
        let frame = view.frame(bounds);
        compose(&self.camera, frame)
    }

    pub(crate) fn framing_extents(&self, bounds: Bounds) -> TargetExtents {
        camera_extents(&self.camera, bounds)
    }

    pub(crate) fn facing_coefficients(
        &self,
        view: CanonicalView,
        bounds: Bounds,
    ) -> FacingCoefficients {
        let frame = view.frame(bounds);
        let source_x = rational_vector(frame.source_u);
        let source_y = rational_vector(frame.source_v);
        let inward = rational_vector(frame.inward);
        FacingCoefficients {
            constant: ratio_to_f64(dot(&self.camera.depth, &inward)),
            relief_x: -ratio_to_f64(dot(&self.camera.depth, &source_x))
                / RELIEF_UNITS_PER_PIXEL as f64,
            relief_y: -ratio_to_f64(dot(&self.camera.depth, &source_y))
                / RELIEF_UNITS_PER_PIXEL as f64,
        }
    }
}

fn compose(camera: &CameraBasis, frame: CanonicalFrame) -> WarpCoefficients {
    let origin = rational_vector(frame.origin);
    let source_x = rational_vector(frame.source_u);
    let source_y = rational_vector(frame.source_v);
    let inward = rational_vector(frame.inward);
    let relief_unit = Ratio::new(1, RELIEF_UNITS_PER_PIXEL);
    WarpCoefficients::from_rational(
        [
            [
                dot(&camera.screen_right, &source_x),
                dot(&camera.screen_right, &source_y),
                dot(&camera.screen_right, &origin),
            ],
            [
                dot(&camera.screen_down, &source_x),
                dot(&camera.screen_down, &source_y),
                dot(&camera.screen_down, &origin),
            ],
        ],
        [
            dot(&camera.screen_right, &inward) * relief_unit,
            dot(&camera.screen_down, &inward) * relief_unit,
        ],
        [
            dot(&camera.depth, &source_x),
            dot(&camera.depth, &source_y),
            dot(&camera.depth, &origin),
        ],
        dot(&camera.depth, &inward) * relief_unit,
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

fn rational_vector(vector: [i64; 3]) -> Vector {
    vector.map(Ratio::from_integer)
}

fn ratio_zero() -> Ratio<i64> {
    Ratio::from_integer(0)
}

fn ratio_to_f64(value: Ratio<i64>) -> f64 {
    *value.numer() as f64 / *value.denom() as f64
}
