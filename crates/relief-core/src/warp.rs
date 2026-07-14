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
    source_x: [Ratio<i64>; 2],
    source_y: [Ratio<i64>; 2],
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
        let [[a, b, translate_x], [c, d, translate_y]] = &self.screen;
        let determinant = *a * *d - *b * *c;
        if determinant == Ratio::from_integer(0) {
            return None;
        }

        let target_x = screen_x - *translate_x;
        let target_y = screen_y - *translate_y;
        let source_x = [
            (*d * target_x - *b * target_y) / determinant,
            (-*d * self.parallax[0] + *b * self.parallax[1]) / determinant,
        ];
        let source_y = [
            (-*c * target_x + *a * target_y) / determinant,
            (*c * self.parallax[0] - *a * self.parallax[1]) / determinant,
        ];
        let depth = [
            self.depth_plane[0] * source_x[0]
                + self.depth_plane[1] * source_y[0]
                + self.depth_plane[2],
            self.depth_plane[0] * source_x[1]
                + self.depth_plane[1] * source_y[1]
                + self.depth_relief,
        ];

        Some(InverseWarpLine {
            source_x,
            source_y,
            depth,
        })
    }
}

impl InverseWarpLine {
    pub fn source_at(&self, relief: Ratio<i64>) -> SourcePoint {
        SourcePoint::new(
            self.source_x[0] + self.source_x[1] * relief,
            self.source_y[0] + self.source_y[1] * relief,
        )
    }

    pub fn depth_at(&self, relief: Ratio<i64>) -> Ratio<i64> {
        self.depth[0] + self.depth[1] * relief
    }

    pub fn source_coefficients(&self) -> [[Ratio<i64>; 2]; 2] {
        [self.source_x, self.source_y]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WarpedSample {
    pub screen_x: Ratio<i64>,
    pub screen_y: Ratio<i64>,
    pub depth: Ratio<i64>,
}
