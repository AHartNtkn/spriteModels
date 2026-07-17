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

/// The screen-coordinate-independent factoring of the inverse-line solve for a
/// single (chart, camera) pair. Building this performs the pivot selection and
/// the 2×2 inversion once; [`PreparedInverse::inverse_frame`] then fixes the
/// per-frame screen origin and produces a [`FrameInverse`] whose per-pixel work
/// is exact integer affine substitution — no per-pixel pivot search, no
/// per-pixel rational division, and no gcd reduction.
///
/// Each per-pixel field is an affine function of `(screen_x, screen_y)`. The
/// pivot rows' constant offsets `variables[first][0]` and
/// `variables[second][0]` carry the only screen dependence; all slopes and the
/// free row are screen-independent constants computed in
/// [`WarpCoefficients::prepare_inverse`].
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

/// `1/2 <= |value|` representable exactly in `f64` requires `|value| <= 2^53`;
/// integers up to and including `2^53` are exactly representable, `2^53 + 1` is
/// not. Every rational that becomes an `f64` in the hot path is asserted to have
/// numerator and denominator magnitudes within this bound so that the unreduced
/// integer division `(numerator as f64) / (denominator as f64)` is the correctly
/// rounded `f64` of the exact value — bit-identical to converting the reduced
/// pair (whose magnitudes are no larger). See [`FrameInverse`] for the full
/// argument.
const MAX_F64_EXACT_INT: i128 = 1 << 53;

/// A reduced rational over `i128`, used only during per-(chart,frame) setup to
/// combine the frame-constant inverse coefficients with the frame screen origin
/// without the intermediate overflow that `Ratio<i64>` would risk. Every operand
/// entering setup is a reduced `Ratio<i64>` (magnitude `< 2^63`); products and
/// cross-sums therefore stay within `i128`, and each result is reduced back to
/// lowest terms.
#[derive(Clone, Copy)]
struct Rat128 {
    numerator: i128,
    denominator: i128,
}

fn gcd_i128(a: i128, b: i128) -> i128 {
    let mut a = a.unsigned_abs();
    let mut b = b.unsigned_abs();
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a as i128
}

impl Rat128 {
    fn new(numerator: i128, denominator: i128) -> Self {
        assert!(denominator != 0, "Rat128 denominator must be non-zero");
        let (mut numerator, mut denominator) = if denominator < 0 {
            (-numerator, -denominator)
        } else {
            (numerator, denominator)
        };
        let divisor = gcd_i128(numerator, denominator);
        if divisor > 1 {
            numerator /= divisor;
            denominator /= divisor;
        }
        Self {
            numerator,
            denominator,
        }
    }

    fn from_ratio(value: Ratio<i64>) -> Self {
        Self::new(i128::from(*value.numer()), i128::from(*value.denom()))
    }

    fn mul(self, other: Self) -> Self {
        let numerator = self
            .numerator
            .checked_mul(other.numerator)
            .expect("frame-setup rational multiply overflowed i128");
        let denominator = self
            .denominator
            .checked_mul(other.denominator)
            .expect("frame-setup rational multiply overflowed i128");
        Self::new(numerator, denominator)
    }

    fn add(self, other: Self) -> Self {
        let divisor = gcd_i128(self.denominator, other.denominator);
        let lcm = (self.denominator / divisor)
            .checked_mul(other.denominator)
            .expect("frame-setup rational add overflowed i128");
        let left = self
            .numerator
            .checked_mul(other.denominator / divisor)
            .expect("frame-setup rational add overflowed i128");
        let right = other
            .numerator
            .checked_mul(self.denominator / divisor)
            .expect("frame-setup rational add overflowed i128");
        let numerator = left
            .checked_add(right)
            .expect("frame-setup rational add overflowed i128");
        Self::new(numerator, lcm)
    }
}

/// An exact affine function of the integer pixel column `x` and row `y`,
/// `value(x, y) = (a + bx * x + by * y) / d`, with `d > 0`. Frame-constant
/// integer coefficients let the per-pixel evaluation be three integer multiplies
/// and two adds — no rational arithmetic, no reduction.
#[derive(Clone, Copy, Debug)]
struct AffineForm {
    a: i128,
    bx: i128,
    by: i128,
    d: i128,
}

impl AffineForm {
    const ZERO: Self = Self {
        a: 0,
        bx: 0,
        by: 0,
        d: 1,
    };

    /// Builds the affine form of `constant + bx_rat * x + by_rat * y` by placing
    /// the three frame-constant rationals over their common denominator. Asserts
    /// the shared denominator is representable in `f64` (`<= 2^53`) so that the
    /// per-pixel `f64` conversion is exact.
    fn from_rats(constant: Rat128, bx_rat: Rat128, by_rat: Rat128) -> Self {
        let divisor = gcd_i128(constant.denominator, bx_rat.denominator);
        let partial = (constant.denominator / divisor)
            .checked_mul(bx_rat.denominator)
            .expect("frame-setup denominator lcm overflowed i128");
        let divisor = gcd_i128(partial, by_rat.denominator);
        let d = (partial / divisor)
            .checked_mul(by_rat.denominator)
            .expect("frame-setup denominator lcm overflowed i128");
        assert!(
            (1..=MAX_F64_EXACT_INT).contains(&d),
            "inverse-line shared denominator {d} exceeds the 2^53 f64-exact bound; \
             the camera basis or model bounds exceed the documented domain"
        );
        let scale = |value: Rat128| {
            value
                .numerator
                .checked_mul(d / value.denominator)
                .expect("frame-setup numerator scaling overflowed i128")
        };
        Self {
            a: scale(constant),
            bx: scale(bx_rat),
            by: scale(by_rat),
            d,
        }
    }

    fn numerator_at(&self, x: i128, y: i128) -> i128 {
        let bx_term = self
            .bx
            .checked_mul(x)
            .expect("inverse-line x term overflowed i128");
        let by_term = self
            .by
            .checked_mul(y)
            .expect("inverse-line y term overflowed i128");
        self.a
            .checked_add(bx_term)
            .and_then(|partial| partial.checked_add(by_term))
            .expect("inverse-line numerator overflowed i128")
    }
}

/// The per-(chart,frame) fixed-point factoring of the inverse-line solve.
///
/// # Common-denominator scheme
///
/// For integer pixel column `x` and row `y`, the screen coordinates are
/// `screen_x = x + sx0` and `screen_y = y + sy0`, where `sx0 = 1/2 - offset_x`
/// and `sy0 = 1/2 - offset_y` are frame constants. Each of the three source
/// variables (source x, source y, relief) has value
/// `variables[i][0] + variables[i][1] * parameter` along the inverse line; the
/// offset `variables[i][0]` is the only screen-dependent part and is an affine
/// function of `(screen_x, screen_y)`, hence an affine function of `(x, y)`:
/// `(a_i + bx_i * x + by_i * y) / d_i`. These coefficients are computed once at
/// setup (in exact `i128` rational arithmetic) and evaluated per pixel with pure
/// integer arithmetic.
///
/// # `f64` exactness
///
/// The predecessor converted the *reduced* `Ratio<i64>` offset `n/d` to `f64`
/// as `(n as f64) / (d as f64)`. This factoring instead holds the *unreduced*
/// pair `N/D = (n * g)/(d * g)`. `f64` division is correctly rounded, so
/// `(N as f64)/(D as f64) == (n as f64)/(d as f64)` whenever `N` and `D` are both
/// exactly representable in `f64`, i.e. `|N| <= 2^53` and `D <= 2^53`: both sides
/// then equal the correctly rounded `f64` of the identical exact value, and the
/// reduced magnitudes `|n| <= |N|`, `d <= D` are exact a fortiori. Setup asserts
/// `D <= 2^53` ([`AffineForm::from_rats`]) and that every per-pixel numerator
/// `N = a + bx*x + by*y` satisfies `|N| <= 2^53` at the four corners of the pixel
/// rectangle; because `N` is affine in `x, y` its extreme magnitudes occur at a
/// corner, so the interior is covered.
///
/// Informal magnitude estimate, not a proof: camera basis entries are
/// `integer/Dcam` with `|integer| <= 1024` and `Dcam <= 1024` (editor
/// quantization uses denominator 1024; presets use denominators dividing 12
/// with numerators `<= 4`). Model bounds are `<= 63`, so origin offsets are
/// `<= 63`; `source_u/v/inward` are unit vectors; the relief unit is `1/8`.
/// The projected columns therefore have numerators `<= 3*1024*63 < 2^18` over
/// denominators `<= 8*1024 < 2^13`. Composing these through the 2×2
/// determinant, the offsets divided by it, and the screen origin (denominator
/// `2`, numerators `<= 2*(side + bound)`) *suggests* shared denominators and
/// per-pixel numerators stay comfortably below `2^53` for `side <= a few
/// thousand` — but no bound has actually been derived through that
/// composition; this paragraph only motivates why the domain is expected to
/// fit.
///
/// The actual guarantee is the runtime certification, not this estimate:
/// [`AffineForm::from_rats`] asserts the shared denominator `d <= 2^53`, and
/// [`PreparedInverse::inverse_frame`] asserts `|numerator| <= 2^53` at the
/// four corners of the pixel rectangle (the numerator is affine in `x, y`, so
/// its extremes occur at a corner — see above). Together these convert any
/// violation of the expected domain, whether or not the estimate above holds,
/// into a loud panic rather than a silent precision loss.
#[derive(Clone, Copy, Debug)]
pub struct FrameInverse {
    var_offset: [AffineForm; 3],
    var_slope: [Ratio<i64>; 3],
    var_slope_f64: [f64; 3],
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

    /// The forward screen-projection rows: `screen[axis]` dotted with
    /// `(source_x, source_y, 1)` is screen axis `axis` before the relief
    /// parallax term. Exposed so the renderer can bound the exact screen
    /// image of a source-space box (the warp of a box is a zonotope whose
    /// axis-aligned bbox is the sum of per-column contributions).
    pub fn screen(&self) -> [[Ratio<i64>; 3]; 2] {
        self.screen
    }

    /// The relief parallax columns: screen axis `axis` gains
    /// `relief * parallax[axis]`. See [`WarpCoefficients::screen`].
    pub fn parallax(&self) -> [Ratio<i64>; 2] {
        self.parallax
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
    /// by [`FrameInverse`]. Returns `None` exactly when the per-pixel solve would
    /// have — when all three projected 2×2 minors are singular — because that
    /// condition depends only on the matrix, not on the screen coordinates.
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
    /// Fixes the frame screen origin — `screen_x = x + sx0`, `screen_y = y + sy0`
    /// for integer pixel column `x`, row `y` — and returns the per-pixel
    /// fixed-point [`FrameInverse`]. `width` and `height` bound the pixel
    /// rectangle so the setup asserts can certify that every per-pixel numerator
    /// stays within the `f64`-exact range.
    pub fn inverse_frame(
        &self,
        sx0: Ratio<i64>,
        sy0: Ratio<i64>,
        width: u32,
        height: u32,
    ) -> FrameInverse {
        let sx0 = Rat128::from_ratio(sx0);
        let sy0 = Rat128::from_ratio(sy0);

        // variables[i][0] = offset[0] + offset[1]*screen_x + offset[2]*screen_y
        //                 = (offset[0] + offset[1]*sx0 + offset[2]*sy0)
        //                   + offset[1]*x + offset[2]*y.
        let affine_of = |offset: [Ratio<i64>; 3]| {
            let o0 = Rat128::from_ratio(offset[0]);
            let o1 = Rat128::from_ratio(offset[1]);
            let o2 = Rat128::from_ratio(offset[2]);
            let constant = o0.add(o1.mul(sx0)).add(o2.mul(sy0));
            AffineForm::from_rats(constant, o1, o2)
        };

        let mut var_offset = [AffineForm::ZERO; 3];
        var_offset[self.first] = affine_of(self.first_offset);
        var_offset[self.second] = affine_of(self.second_offset);
        // var_offset[self.free] stays ZERO: the free variable's offset is 0.

        let mut var_slope = [Ratio::from_integer(0); 3];
        var_slope[self.free] = Ratio::from_integer(1);
        var_slope[self.first] = self.first_slope;
        var_slope[self.second] = self.second_slope;

        let frame = FrameInverse {
            var_offset,
            var_slope,
            var_slope_f64: var_slope.map(ratio_to_f64),
            depth_plane: self.depth_plane,
            depth_relief: self.depth_relief,
            depth_slope: self.depth_slope,
        };

        // Certify the per-pixel f64 numerators stay f64-exact. Each numerator is
        // affine in (x, y), so its extreme magnitudes lie at a corner of the
        // pixel rectangle [0, width-1] x [0, height-1]; checking the corners
        // covers the interior. width/height are >= 1 here (the caller returns
        // early for an empty frame).
        let last_x = i128::from(width.saturating_sub(1));
        let last_y = i128::from(height.saturating_sub(1));
        for form in &frame.var_offset {
            for &x in &[0, last_x] {
                for &y in &[0, last_y] {
                    let numerator = form.numerator_at(x, y);
                    assert!(
                        numerator.unsigned_abs() <= MAX_F64_EXACT_INT as u128,
                        "inverse-line numerator {numerator} at pixel ({x}, {y}) exceeds the \
                         2^53 f64-exact bound; the camera basis, model bounds, or frame size \
                         exceed the documented domain"
                    );
                }
            }
        }

        frame
    }
}

impl FrameInverse {
    fn numerator_denominator(&self, variable: usize, x: u32, y: u32) -> (i128, i128) {
        let form = &self.var_offset[variable];
        (form.numerator_at(i128::from(x), i128::from(y)), form.d)
    }

    /// Per-pixel `[offset, slope]` for source x, source y, and relief as `f64`,
    /// the exact bit pattern the reduced-`Ratio` predecessor produced (see the
    /// type-level `f64`-exactness argument). The offsets are converted from the
    /// unreduced integer affine numerators; the slopes are frame constants
    /// converted once at setup.
    pub fn variable_f64(&self, x: u32, y: u32) -> [[f64; 2]; 3] {
        std::array::from_fn(|variable| {
            let (numerator, denominator) = self.numerator_denominator(variable, x, y);
            debug_assert!(
                numerator.unsigned_abs() <= MAX_F64_EXACT_INT as u128
                    && denominator <= MAX_F64_EXACT_INT,
                "inverse-line f64 conversion outside the 2^53-exact range"
            );
            [
                numerator as f64 / denominator as f64,
                self.var_slope_f64[variable],
            ]
        })
    }

    /// Exact `[offset, slope]` per source variable as reduced `Ratio<i64>`. Used
    /// by depth reconstruction and by exactness tests. The numerator and
    /// denominator fit `i64` because they are within the `2^53` bound certified
    /// at setup.
    pub fn variable_coefficients_exact(&self, x: u32, y: u32) -> [[Ratio<i64>; 2]; 3] {
        std::array::from_fn(|variable| {
            let (numerator, denominator) = self.numerator_denominator(variable, x, y);
            debug_assert!(
                numerator.unsigned_abs() <= MAX_F64_EXACT_INT as u128
                    && denominator <= MAX_F64_EXACT_INT,
                "inverse-line i64 cast outside the 2^53-exact range"
            );
            [
                Ratio::new(numerator as i64, denominator as i64),
                self.var_slope[variable],
            ]
        })
    }

    fn depth_offset(&self, offsets: &[[Ratio<i64>; 2]; 3]) -> Ratio<i64> {
        self.depth_plane[0] * offsets[0][0]
            + self.depth_plane[1] * offsets[1][0]
            + self.depth_plane[2]
            + self.depth_relief * offsets[2][0]
    }

    /// Exact `[constant, slope]` for transient camera depth at this pixel, as
    /// reduced `Ratio<i64>`: the depth along the inverse line is
    /// `constant + slope * parameter`.
    pub fn depth_coefficients_exact(&self, x: u32, y: u32) -> [Ratio<i64>; 2] {
        let offsets = self.variable_coefficients_exact(x, y);
        [self.depth_offset(&offsets), self.depth_slope]
    }

    /// Exact camera depth `constant + slope * parameter` at this pixel. The
    /// result is the reduced canonical `Ratio<i64>` of the same rational the
    /// predecessor's `depth_at` produced, so the golden fragment-owner depths are
    /// bit-identical.
    pub fn depth_at(&self, x: u32, y: u32, parameter: Ratio<i64>) -> Ratio<i64> {
        let offsets = self.variable_coefficients_exact(x, y);
        self.depth_offset(&offsets) + self.depth_slope * parameter
    }
}

fn ratio_to_f64(value: Ratio<i64>) -> f64 {
    *value.numer() as f64 / *value.denom() as f64
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WarpedSample {
    pub screen_x: Ratio<i64>,
    pub screen_y: Ratio<i64>,
    pub depth: Ratio<i64>,
}
