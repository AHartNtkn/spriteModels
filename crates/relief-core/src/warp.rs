use num_rational::Ratio;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourcePoint {
    pub x: Ratio<i64>,
    pub y: Ratio<i64>,
}

impl SourcePoint {
    pub fn new(x: Ratio<i64>, y: Ratio<i64>) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WarpCoefficients {
    screen: [[Ratio<i64>; 3]; 2],
    parallax: [Ratio<i64>; 2],
    depth_plane: [Ratio<i64>; 3],
    depth_relief: Ratio<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InverseWarpLine {
    variables: [[Ratio<i64>; 2]; 3],
    depth: [Ratio<i64>; 2],
}

/// The screen-coordinate-independent factoring of the inverse-line solve for a
/// single (chart, camera) pair. Building this performs the pivot selection and
/// the 2×2 inversion once; [`PreparedInverse::inverse_line`] then reconstructs
/// the per-pixel [`InverseWarpLine`] with only affine substitution of the
/// screen coordinates — no per-pixel pivot search and no per-pixel rational
/// division.
///
/// Each per-pixel field is an affine function of `(screen_x, screen_y)`. The
/// pivot rows' constant offsets `variables[first][0]` and
/// `variables[second][0]` carry the only screen dependence; all slopes and the
/// free row are screen-independent constants computed in
/// [`WarpCoefficients::prepare_inverse`]. Because `num_rational::Ratio` is kept
/// in reduced canonical form, evaluating those affine forms yields the exact
/// same reduced rationals the per-pixel solve produced, bit for bit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparedInverse {
    first: usize,
    second: usize,
    free: usize,
    /// `[base, d/dscreen_x, d/dscreen_y]` for `variables[first][0]`.
    first_offset: [Ratio<i64>; 3],
    /// `[base, d/dscreen_x, d/dscreen_y]` for `variables[second][0]`.
    second_offset: [Ratio<i64>; 3],
    first_slope: Ratio<i64>,
    second_slope: Ratio<i64>,
    depth_plane: [Ratio<i64>; 3],
    depth_relief: Ratio<i64>,
    depth_slope: Ratio<i64>,
}

impl WarpCoefficients {
    pub fn new(
        screen: [[i64; 3]; 2],
        parallax: [i64; 2],
        depth_plane: [i64; 3],
        depth_relief: i64,
    ) -> Self {
        Self::from_rational(
            screen.map(|row| row.map(Ratio::from_integer)),
            parallax.map(Ratio::from_integer),
            depth_plane.map(Ratio::from_integer),
            Ratio::from_integer(depth_relief),
        )
    }

    pub fn from_rational(
        screen: [[Ratio<i64>; 3]; 2],
        parallax: [Ratio<i64>; 2],
        depth_plane: [Ratio<i64>; 3],
        depth_relief: Ratio<i64>,
    ) -> Self {
        Self {
            screen,
            parallax,
            depth_plane,
            depth_relief,
        }
    }

    pub fn apply(&self, point: SourcePoint, relief: Ratio<i64>) -> WarpedSample {
        let source = [point.x, point.y, Ratio::from_integer(1)];
        let dot = |row: &[Ratio<i64>; 3]| {
            source
                .iter()
                .zip(row)
                .fold(Ratio::from_integer(0), |sum, (value, coefficient)| {
                    sum + *value * *coefficient
                })
        };

        WarpedSample {
            screen_x: dot(&self.screen[0]) + relief * self.parallax[0],
            screen_y: dot(&self.screen[1]) + relief * self.parallax[1],
            depth: dot(&self.depth_plane) + relief * self.depth_relief,
        }
    }

    /// Solves the camera-only part of the inverse-line system once for this
    /// (chart, camera) pair, returning the affine factoring evaluated per pixel
    /// by [`PreparedInverse::inverse_line`]. Returns `None` exactly when the
    /// per-pixel solve would have — when all three projected 2×2 minors are
    /// singular — because that condition depends only on the matrix, not on the
    /// screen coordinates.
    pub fn prepare_inverse(&self) -> Option<PreparedInverse> {
        let columns = [
            [self.screen[0][0], self.screen[1][0]],
            [self.screen[0][1], self.screen[1][1]],
            self.parallax,
        ];
        let zero = Ratio::from_integer(0);
        let (first, second, free, determinant, _) = [(0, 1, 2), (0, 2, 1), (1, 2, 0)]
            .into_iter()
            .filter_map(|(first, second, free)| {
                let determinant =
                    columns[first][0] * columns[second][1] - columns[second][0] * columns[first][1];
                let magnitude = if determinant < zero {
                    -determinant
                } else {
                    determinant
                };
                (magnitude != zero).then_some((first, second, free, determinant, magnitude))
            })
            .reduce(|best, candidate| {
                if candidate.4 > best.4 {
                    candidate
                } else {
                    best
                }
            })?;

        let s0 = self.screen[0][2];
        let s1 = self.screen[1][2];
        let cf0 = columns[first][0];
        let cf1 = columns[first][1];
        let cs0 = columns[second][0];
        let cs1 = columns[second][1];

        // variables[first][0] = (target[0]*cs1 - cs0*target[1]) / det, with
        // target = [screen_x - s0, screen_y - s1]. Expanding in screen_x,
        // screen_y gives the affine coefficients below; because the reduced
        // rational is unique, this yields the same value the direct formula
        // does per pixel.
        let first_offset = [
            (cs0 * s1 - s0 * cs1) / determinant,
            cs1 / determinant,
            -cs0 / determinant,
        ];
        // variables[second][0] = (cf0*target[1] - target[0]*cf1) / det.
        let second_offset = [
            (s0 * cf1 - cf0 * s1) / determinant,
            -cf1 / determinant,
            cf0 / determinant,
        ];
        let first_slope = (-columns[free][0] * cs1 + cs0 * columns[free][1]) / determinant;
        let second_slope = (-cf0 * columns[free][1] + columns[free][0] * cf1) / determinant;

        // depth[1] uses only the (screen-independent) slopes; the free row's
        // slope is 1, the other two are computed above.
        let mut slopes = [zero; 3];
        slopes[free] = Ratio::from_integer(1);
        slopes[first] = first_slope;
        slopes[second] = second_slope;
        let depth_slope = self.depth_plane[0] * slopes[0]
            + self.depth_plane[1] * slopes[1]
            + self.depth_relief * slopes[2];

        Some(PreparedInverse {
            first,
            second,
            free,
            first_offset,
            second_offset,
            first_slope,
            second_slope,
            depth_plane: self.depth_plane,
            depth_relief: self.depth_relief,
            depth_slope,
        })
    }
}

impl PreparedInverse {
    /// Reconstructs the per-pixel [`InverseWarpLine`] by substituting the screen
    /// coordinates into the precomputed affine forms. Every produced rational
    /// equals the one the per-pixel 2×2 solve produced (reduced canonical form
    /// is unique), so the downstream `f64` conversions are bit-identical.
    pub fn inverse_line(&self, screen_x: Ratio<i64>, screen_y: Ratio<i64>) -> InverseWarpLine {
        let zero = Ratio::from_integer(0);
        let eval =
            |coeffs: [Ratio<i64>; 3]| coeffs[0] + coeffs[1] * screen_x + coeffs[2] * screen_y;

        let mut variables = [[zero; 2]; 3];
        variables[self.free][1] = Ratio::from_integer(1);
        variables[self.first] = [eval(self.first_offset), self.first_slope];
        variables[self.second] = [eval(self.second_offset), self.second_slope];

        let depth = [
            self.depth_plane[0] * variables[0][0]
                + self.depth_plane[1] * variables[1][0]
                + self.depth_plane[2]
                + self.depth_relief * variables[2][0],
            self.depth_slope,
        ];

        InverseWarpLine { variables, depth }
    }
}

impl InverseWarpLine {
    /// Evaluates the source coordinates at the shared affine-line parameter.
    pub fn source_at(&self, parameter: Ratio<i64>) -> SourcePoint {
        SourcePoint::new(
            self.variables[0][0] + self.variables[0][1] * parameter,
            self.variables[1][0] + self.variables[1][1] * parameter,
        )
    }

    pub fn depth_at(&self, parameter: Ratio<i64>) -> Ratio<i64> {
        self.depth[0] + self.depth[1] * parameter
    }

    pub fn relief_at(&self, parameter: Ratio<i64>) -> Ratio<i64> {
        self.variables[2][0] + self.variables[2][1] * parameter
    }

    /// Returns `[constant, slope]` for source x, source y, and relief, in that order.
    pub fn variable_coefficients(&self) -> [[Ratio<i64>; 2]; 3] {
        self.variables
    }

    /// Returns `[constant, slope]` for transient camera depth.
    pub fn depth_coefficients(&self) -> [Ratio<i64>; 2] {
        self.depth
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WarpedSample {
    pub screen_x: Ratio<i64>,
    pub screen_y: Ratio<i64>,
    pub depth: Ratio<i64>,
}
