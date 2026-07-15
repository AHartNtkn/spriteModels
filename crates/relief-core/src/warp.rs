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

    pub fn inverse_line(
        &self,
        screen_x: Ratio<i64>,
        screen_y: Ratio<i64>,
    ) -> Option<InverseWarpLine> {
        let columns = [
            [self.screen[0][0], self.screen[1][0]],
            [self.screen[0][1], self.screen[1][1]],
            self.parallax,
        ];
        let target = [screen_x - self.screen[0][2], screen_y - self.screen[1][2]];
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

        let mut variables = [[zero; 2]; 3];
        variables[free][1] = Ratio::from_integer(1);
        variables[first] = [
            (target[0] * columns[second][1] - columns[second][0] * target[1]) / determinant,
            (-columns[free][0] * columns[second][1] + columns[second][0] * columns[free][1])
                / determinant,
        ];
        variables[second] = [
            (columns[first][0] * target[1] - target[0] * columns[first][1]) / determinant,
            (-columns[first][0] * columns[free][1] + columns[free][0] * columns[first][1])
                / determinant,
        ];
        let depth = [
            self.depth_plane[0] * variables[0][0]
                + self.depth_plane[1] * variables[1][0]
                + self.depth_plane[2]
                + self.depth_relief * variables[2][0],
            self.depth_plane[0] * variables[0][1]
                + self.depth_plane[1] * variables[1][1]
                + self.depth_relief * variables[2][1],
        ];

        Some(InverseWarpLine { variables, depth })
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
