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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WarpedSample {
    pub screen_x: Ratio<i64>,
    pub screen_y: Ratio<i64>,
    pub depth: Ratio<i64>,
}
