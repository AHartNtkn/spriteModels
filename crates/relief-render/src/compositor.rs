use num_rational::Ratio;
use relief_core::{
    Bounds, Chart, DecodedTexel, FrameInverse, ReliefField, ResolvedCharts, SourcePoint,
};
use std::cmp::Ordering;
use thiserror::Error;

use crate::{
    FragmentKey, FrameBuffer, TargetView,
    framebuffer::commit_fragment,
    presets::{FacingCoefficients, TargetExtents},
};

const ROOT_SCALE: i64 = 1 << 24;
const COORDINATE_EPSILON: f64 = 1.0e-9;
const POLYNOMIAL_EPSILON: f64 = 1.0e-10;

/// Static upper bound on the clipped ray-parameter range length. The ray
/// parameter is the inverse solve's *free* source variable itself
/// (`PreparedInverse` sets `var_slope[free] = 1` and `var_offset[free] = 0`,
/// and those survive to the f64 coefficients exactly), and `solve_preimages`
/// clips the parameter range so every source variable — the free one included —
/// stays inside its box. The clipped range is therefore contained in the free
/// variable's box: source coordinates span at most 63 (`Bounds::new` rejects
/// sides outside `1..=63`) and relief spans at most
/// `CanonicalView::maximum_inward_depth = 4 * opposing_axis <= 4 * 63`. Hence
/// `4 * 63 = 252`, and every root-bearing segment `[start, end]` — a
/// sub-interval of that range — has span at most 252.
const MAX_PARAMETER_SPAN: f64 = 252.0;

/// Worst-case count of quantum-boundary sign probes in
/// [`correctly_rounded_root`]. The root's parameter quantum is located by
/// binary search over the integer quanta covered by the bracket's parameter
/// image: the first two probes test the closed-form root's own quantum
/// boundaries (confirming it outright in the well-conditioned case), and every
/// later probe bisects the remaining candidate range, at least halving it. The
/// range initially holds at most `MAX_PARAMETER_SPAN * ROOT_SCALE + 2 =
/// 252 * 2^24 + 2` candidate quanta (endpoint quanta of a parameter interval
/// of length at most `MAX_PARAMETER_SPAN` differ by at most
/// `MAX_PARAMETER_SPAN * ROOT_SCALE + 1`), which is `<= 2^32` (so 32 halvings
/// always suffice) and `> 2^31` (so 31 do not) — hence `2 + 32 = 34` probes:
/// derived, not chosen.
const MAX_QUANTUM_PROBES: u32 = 34;

/// Upper bound on `|t|` for every ray parameter at which `solve_preimages`
/// evaluates the root-acceptance box checks. The clipped parameter range is a
/// sub-interval of `[0, MAX_PARAMETER_SPAN]` (the range is clipped against the
/// free variable's own box `[0, extent]` with `extent <= 252`, exactly — the
/// free variable has offset `0.0` and slope `1.0`, so `clip_affine_range`
/// computes the box bounds themselves). Segment endpoints are boundaries
/// inside that range; direct partition hits reconstruct `t` inside
/// `[start, end]`, and bisected roots reconstruct the unit preimage of a
/// parameter quantum within half a quantum (`2^-25`) of a bracket endpoint
/// inside `[start, end]`, perturbed by a few ulps of f64 reconstruction noise.
/// The slack of `1` therefore dominates every excursion outside the clipped
/// range by more than seven orders of magnitude.
const MAX_ACCEPTED_PARAMETER: f64 = MAX_PARAMETER_SPAN + 1.0;

/// A fixed-capacity, stack-allocated ordered collection for values whose
/// cardinality is bounded by a small mathematical fact established at each
/// call site (documented where the capacity `N` is chosen). Replaces
/// heap-allocated `Vec`s in the per-pixel hot path with storage that never
/// allocates. `push` beyond capacity is a programming error, not a runtime
/// condition to degrade gracefully from: it means the documented bound does
/// not actually hold, so it panics loudly rather than silently truncating.
#[derive(Clone, Copy)]
struct Bounded<T, const N: usize> {
    items: [T; N],
    len: usize,
}

impl<T: Copy + Default, const N: usize> Bounded<T, N> {
    fn new() -> Self {
        Self {
            items: [T::default(); N],
            len: 0,
        }
    }

    fn push(&mut self, value: T) {
        assert!(
            self.len < N,
            "Bounded<_, {N}> capacity exceeded: the documented bound for this call site does not hold"
        );
        self.items[self.len] = value;
        self.len += 1;
    }

    fn as_slice(&self) -> &[T] {
        &self.items[..self.len]
    }

    fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.items[..self.len]
    }

    fn sort_by(&mut self, compare: impl FnMut(&T, &T) -> std::cmp::Ordering) {
        self.as_mut_slice().sort_by(compare);
    }

    /// Same semantics as `Vec::dedup_by`: scans left to right, keeping the
    /// first element of each run of consecutive elements for which
    /// `same_bucket` returns `true`.
    fn dedup_by(&mut self, mut same_bucket: impl FnMut(&T, &T) -> bool) {
        let mut write = usize::from(self.len > 0);
        for read in 1..self.len {
            if same_bucket(&self.items[read], &self.items[write - 1]) {
                continue;
            }
            self.items[write] = self.items[read];
            write += 1;
        }
        self.len = write;
    }
}

impl<T: Copy + Default, const N: usize> IntoIterator for Bounded<T, N> {
    type Item = T;
    type IntoIter = std::iter::Take<std::array::IntoIter<T, N>>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter().take(self.len)
    }
}

impl<'a, T: Copy + Default, const N: usize> IntoIterator for &'a Bounded<T, N> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderRequest {
    width: u32,
    height: u32,
    target: TargetView,
}

impl RenderRequest {
    pub fn new(width: u32, height: u32, target: TargetView) -> Self {
        Self {
            width,
            height,
            target,
        }
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum RenderError {
    #[error("framebuffer dimensions overflow addressable storage")]
    FrameBufferTooLarge,
}

/// A recovered source-cell hit along the inverse line. `parameter` is the ray
/// parameter quantized to denominator `ROOT_SCALE = 2^24`, stored as the integer
/// numerator `round(t * 2^24)`; the implicit denominator is shared by every
/// preimage, so integer comparison of `parameter` is exact rational comparison
/// and `parameter as f64 / ROOT_SCALE as f64` is the bit-identical `f64` the
/// reduced-`Ratio` predecessor produced (both `|parameter| <= 2^53` and
/// `2^24 <= 2^53`, so both are correctly rounded conversions of the same value).
#[derive(Clone, Copy, Debug)]
struct Preimage {
    parameter: i64,
    source_x: u32,
    source_y: u32,
}

/// One quadrant patch of a foreground cell: the relief surface values at its
/// four half-texel corners, each resolved at prepare time as the f64 quotient
/// `weighted / total` of the exact-rational hat-kernel terms sampled at that
/// corner. The patch surface is the plain bilinear interpolant of these
/// values, so the ray-surface equation is quadratic in the ray parameter.
#[derive(Clone, Copy, Debug)]
struct ReliefPatch {
    corners: [f64; 4],
}

#[derive(Clone, Debug)]
struct PreparedRelief {
    width: u32,
    height: u32,
    cells: Vec<Option<[ReliefPatch; 4]>>,
}

impl PreparedRelief {
    fn new(field: &ReliefField) -> Self {
        let (width, height) = field.dimensions();
        let mut cells = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                cells.push(field.foreground_cell(x, y).map(|cell| {
                    std::array::from_fn(|quadrant| {
                        let right = quadrant % 2 == 1;
                        let bottom = quadrant / 2 == 1;
                        let left = Ratio::new(2 * i64::from(x) + i64::from(right), 2);
                        let top = Ratio::new(2 * i64::from(y) + i64::from(bottom), 2);
                        let corners = [
                            (left, top),
                            (left + Ratio::new(1, 2), top),
                            (left, top + Ratio::new(1, 2)),
                            (left + Ratio::new(1, 2), top + Ratio::new(1, 2)),
                        ]
                        .map(|(sample_x, sample_y)| {
                            let (weighted, total) = cell
                                .sample_terms_closure(SourcePoint::new(sample_x, sample_y))
                                .expect("quadrant corners belong to the foreground cell closure");
                            // The division is always defined: the hat kernel
                            // includes the cell's own texel, whose per-axis
                            // weight is at least 1/2 anywhere in the cell's
                            // closure, so `total >= 1/4 > 0` at every corner.
                            // Both terms are exact multiples of 1/4 far inside
                            // f64's integer range (corners sit on the
                            // half-texel grid), so the conversions are exact
                            // and the quotient is a single correctly rounded
                            // operation.
                            ratio_to_f64(weighted) / ratio_to_f64(total)
                        });
                        ReliefPatch { corners }
                    })
                }));
            }
        }
        Self {
            width,
            height,
            cells,
        }
    }

    fn is_foreground(&self, x: u32, y: u32) -> bool {
        self.cells[(y * self.width + x) as usize].is_some()
    }

    /// Bound: each axis independently matches either exactly one quadrant
    /// half (the local coordinate is strictly on one side of the axis'
    /// midline) or exactly two (the local coordinate is within
    /// `COORDINATE_EPSILON` of the shared midline, touching both halves).
    /// The horizontal and vertical matches combine multiplicatively (every
    /// pair is emitted), so at most 2 * 2 = 4 quadrants are ever produced.
    fn quadrants_at(&self, source_x: u32, source_y: u32, x: f64, y: f64) -> Bounded<usize, 4> {
        let local_x = (x - f64::from(source_x)).clamp(0.0, 1.0);
        let local_y = (y - f64::from(source_y)).clamp(0.0, 1.0);
        let horizontal: &[usize] = if (local_x - 0.5).abs() <= COORDINATE_EPSILON {
            &[0, 1]
        } else if local_x < 0.5 {
            &[0]
        } else {
            &[1]
        };
        let vertical: &[usize] = if (local_y - 0.5).abs() <= COORDINATE_EPSILON {
            &[0, 1]
        } else if local_y < 0.5 {
            &[0]
        } else {
            &[1]
        };
        let mut quadrants = Bounded::new();
        for bottom in vertical {
            for right in horizontal {
                quadrants.push(right + 2 * bottom);
            }
        }
        quadrants
    }

    fn patch_corners(&self, source_x: u32, source_y: u32, quadrant: usize) -> [f64; 4] {
        self.cells[(source_y * self.width + source_x) as usize]
            .expect("only foreground cells reach analytic evaluation")[quadrant]
            .corners
    }

    /// Gradient of the patch's bilinear surface with respect to the source
    /// coordinates at `(x, y)`. A quadrant spans half a texel per axis, so the
    /// patch's unit coordinates are the local source coordinates scaled by 2 —
    /// hence the factor 2 on each partial derivative.
    fn patch_gradient(
        &self,
        source_x: u32,
        source_y: u32,
        quadrant: usize,
        x: f64,
        y: f64,
    ) -> [f64; 2] {
        let corners = self.patch_corners(source_x, source_y, quadrant);
        let local_x = (x - f64::from(source_x)).clamp(0.0, 1.0);
        let local_y = (y - f64::from(source_y)).clamp(0.0, 1.0);
        let right = quadrant % 2 == 1;
        let bottom = quadrant / 2 == 1;
        let u = if right {
            2.0 * local_x - 1.0
        } else {
            2.0 * local_x
        };
        let v = if bottom {
            2.0 * local_y - 1.0
        } else {
            2.0 * local_y
        };
        [
            2.0 * ((corners[1] - corners[0]) * (1.0 - v) + (corners[3] - corners[2]) * v),
            2.0 * ((corners[2] - corners[0]) * (1.0 - u) + (corners[3] - corners[1]) * u),
        ]
    }
}

/// Camera-independent preparation for a resolved model: per-chart relief
/// fields and derived constants that do not depend on `RenderRequest`.
/// Building this is ~40% of a render call, so callers that re-render the
/// same model under different orientations should build it once and pass it
/// to every `render_model` call.
#[derive(Clone, Debug)]
pub struct PreparedModel {
    charts: ResolvedCharts,
    reliefs: Vec<PreparedChart>,
}

#[derive(Clone, Debug)]
struct PreparedChart {
    relief: PreparedRelief,
    maximum_relief: f64,
}

impl PreparedModel {
    pub fn new(charts: &ResolvedCharts) -> Self {
        let bounds = charts.bounds();
        let reliefs = charts
            .charts()
            .iter()
            .map(|chart| {
                let field = ReliefField::new(chart);
                PreparedChart {
                    relief: PreparedRelief::new(&field),
                    maximum_relief: f64::from(chart.view().maximum_inward_depth(bounds)),
                }
            })
            .collect();
        Self {
            charts: charts.clone(),
            reliefs,
        }
    }
}

/// Scratch buffers reused across every pixel x chart segment inside a single
/// `render_model` call. `boundaries` and `preimages` are unbounded in the
/// worst case (their length depends on chart width/height, not a small
/// constant), so they remain heap `Vec`s rather than `Bounded` arrays — but
/// allocated once per `render_model` call and only ever `.clear()`ed, a
/// `Vec`'s backing storage stops reallocating once it reaches its
/// steady-state capacity, unlike a fresh `Vec::new()` per pixel x chart x
/// segment.
#[derive(Default)]
struct RenderScratch {
    boundaries: Vec<f64>,
    preimages: Vec<Preimage>,
}

/// Per-source-variable upper bound on the absolute error between the f64 line
/// evaluation `offset + slope * t` performed by `solve_preimages` and the
/// exact rational line value at the same `t`, over every pixel of the frame
/// and every parameter the solver can test (`|t| <= MAX_ACCEPTED_PARAMETER`).
///
/// Derivation: with `e = 2^-53` (half of `f64::EPSILON`), exact offset `O`,
/// and exact slope `S`, the solver computes `fl(fl(O) + fl(fl(S) * t))`
/// (`fl` denotes one correctly rounded operation or conversion), so
/// `|computed - (O + S*t)| <= |O|*((1+e)^2 - 1) + |S*t|*((1+e)^3 - 1)
///                         <= e*(2.001*|O| + 3.001*|S*t|)
///                         <= 2 * f64::EPSILON * (|O| + |S| * MAX_ACCEPTED_PARAMETER)`.
/// The factor below is `8` instead of `2`: the spare factor `4` dominates
/// (a) the underestimate of `|O|` and `|S|` by their computed f64 images
/// (each within one rounding of exact, factor `<= 1 + 2e`) and (b) the three
/// roundings in evaluating the bound expression itself (factor `<= (1+e)^3`),
/// so the *computed* value is a true upper bound on the exact error. Offsets
/// are affine in the pixel coordinates, so the frame-wide maximum of
/// `|offset|` is attained at a corner of the pixel rectangle; evaluating the
/// four corners covers every pixel.
fn source_evaluation_error_bounds(inverse: &FrameInverse, width: u32, height: u32) -> [f64; 3] {
    let corners = [
        (0, 0),
        (width - 1, 0),
        (0, height - 1),
        (width - 1, height - 1),
    ];
    let mut offset_bound = [0.0_f64; 3];
    let mut slope = [0.0_f64; 3];
    for (x, y) in corners {
        let coefficients = inverse.variable_f64(x, y);
        for variable in 0..3 {
            offset_bound[variable] = offset_bound[variable].max(coefficients[variable][0].abs());
            slope[variable] = coefficients[variable][1].abs();
        }
    }
    std::array::from_fn(|variable| {
        8.0 * f64::EPSILON * (offset_bound[variable] + slope[variable] * MAX_ACCEPTED_PARAMETER)
    })
}

/// One screen axis of the forward splat: converts a foreground cell's swept
/// source box into a conservative range of pixel indices on this axis.
///
/// The exact screen coordinate of a swept source point `(x, y, relief)` is
/// the affine map `coefficients[0]*x + coefficients[1]*y +
/// coefficients[2]*relief + constant`, so the image of an axis-aligned box is
/// the interval centered on the image of the box center with radius the sum
/// of `|coefficient| * half_extent` — the exact axis-aligned bound of the
/// zonotope image. A pixel `p` can receive a committed fragment from a cell
/// only when its exact screen coordinate `p + origin` lies inside the image
/// of the cell's *inflated* swept box (see `render_model` for the inflation
/// argument), so the integer range `[ceil(low), floor(high)]`, widened by the
/// derived f64-noise `margin`, covers every committing pixel.
#[derive(Clone, Copy, Debug)]
struct SplatAxis {
    coefficients: [f64; 3],
    constant: f64,
    radius: f64,
    origin: f64,
    margin: i64,
}

impl SplatAxis {
    /// `screen_row` and `parallax` are this axis's forward warp coefficients,
    /// `origin` is the frame constant added to the integer pixel index
    /// (`sx0`/`sy0`), `deltas` are the [`source_evaluation_error_bounds`],
    /// and `field` supplies the source extents.
    ///
    /// The swept box of cell `(cx, cy)` is inflated by
    /// `COORDINATE_EPSILON + delta` per source variable: an accepted root's
    /// f64 coordinates pass the box check with `COORDINATE_EPSILON` slack,
    /// and the exact line point at the same parameter is within `delta` of
    /// those f64 coordinates, so the exact point lies in the inflated box.
    ///
    /// `margin` bounds the f64 rounding noise of the whole
    /// [`SplatAxis::pixel_range`] chain, in pixels. Every intermediate value
    /// in that chain (center terms, radius terms, and their sums with the
    /// origin) has exact magnitude at most `magnitude` below, and the chain
    /// performs at most 26 correctly rounded operations/conversions (5
    /// rational-to-f64 conversions, 7 half-extent operations, 5 radius
    /// operations, and 9 center/interval operations per endpoint), each
    /// contributing at most `(f64::EPSILON / 2) * magnitude * (1 + tiny)` of
    /// absolute error — at most `13 * f64::EPSILON * magnitude` in total. The
    /// factor `32` leaves a spare factor of ~2.4, which dominates the
    /// underestimate of
    /// `magnitude` by its own f64 evaluation and the roundings of the margin
    /// expression itself; the trailing `+ 1` covers the integer
    /// `ceil`/`floor` boundary semantics (a computed endpoint within the
    /// noise bound of an exact integer boundary can shift the rounded index
    /// by one). Widening only ever *adds* certainly-safe candidate pixels
    /// (each still runs the full unchanged solve), so every step of this
    /// bound errs in the conservative direction.
    fn new(
        screen_row: [Ratio<i64>; 3],
        parallax: Ratio<i64>,
        origin: Ratio<i64>,
        deltas: [f64; 3],
        maximum_relief: f64,
        field: &PreparedRelief,
    ) -> Self {
        let coefficients = [
            ratio_to_f64(screen_row[0]),
            ratio_to_f64(screen_row[1]),
            ratio_to_f64(parallax),
        ];
        let constant = ratio_to_f64(screen_row[2]);
        let origin = ratio_to_f64(origin);
        let half_extents = [
            0.5 + COORDINATE_EPSILON + deltas[0],
            0.5 + COORDINATE_EPSILON + deltas[1],
            0.5 * maximum_relief + COORDINATE_EPSILON + deltas[2],
        ];
        let radius = coefficients[0].abs() * half_extents[0]
            + coefficients[1].abs() * half_extents[1]
            + coefficients[2].abs() * half_extents[2];
        let magnitude = coefficients[0].abs() * (f64::from(field.width) + 1.0 + deltas[0])
            + coefficients[1].abs() * (f64::from(field.height) + 1.0 + deltas[1])
            + coefficients[2].abs() * (maximum_relief + 1.0 + deltas[2])
            + constant.abs()
            + origin.abs()
            + 1.0;
        let margin = (32.0 * f64::EPSILON * magnitude).ceil() as i64 + 1;
        Self {
            coefficients,
            constant,
            radius,
            origin,
            margin,
        }
    }

    /// The conservative pixel-index range cell `(cell_x, cell_y)`'s inflated
    /// swept box can touch on this axis, clamped to `[0, extent)`; `None`
    /// when the clamped range is empty.
    fn pixel_range(
        &self,
        cell_x: f64,
        cell_y: f64,
        middle_relief: f64,
        extent: u32,
    ) -> Option<(u32, u32)> {
        let center = self.coefficients[0] * (cell_x + 0.5)
            + self.coefficients[1] * (cell_y + 0.5)
            + self.coefficients[2] * middle_relief
            + self.constant;
        let first = ((center - self.radius - self.origin).ceil() as i64 - self.margin).max(0);
        let last = ((center + self.radius - self.origin).floor() as i64 + self.margin)
            .min(i64::from(extent) - 1);
        (first <= last).then_some((first as u32, last as u32))
    }
}

/// A per-chart bitmask over the frame's pixels: the union of the conservative
/// pixel rectangles of the chart's foreground cells. Guarantees each
/// (pixel, chart) pair is solved at most once even though neighboring cells'
/// rectangles overlap heavily. Reused across charts via [`PixelMask::reset`].
struct PixelMask {
    words_per_row: usize,
    words: Vec<u64>,
}

impl PixelMask {
    fn new() -> Self {
        Self {
            words_per_row: 0,
            words: Vec::new(),
        }
    }

    fn reset(&mut self, width: u32, height: u32) {
        self.words_per_row = (width as usize).div_ceil(64);
        self.words.clear();
        self.words.resize(self.words_per_row * height as usize, 0);
    }

    /// Marks pixels `first_x..=last_x` of row `y`. Callers guarantee
    /// `first_x <= last_x < width` and `y < height` (the [`SplatAxis`] ranges
    /// are clamped to the frame).
    fn mark_row(&mut self, y: u32, first_x: u32, last_x: u32) {
        let row = y as usize * self.words_per_row;
        let first_word = first_x as usize / 64;
        let last_word = last_x as usize / 64;
        let first_mask = !0_u64 << (first_x as usize % 64);
        let last_mask = !0_u64 >> (63 - last_x as usize % 64);
        if first_word == last_word {
            self.words[row + first_word] |= first_mask & last_mask;
        } else {
            self.words[row + first_word] |= first_mask;
            for word in &mut self.words[row + first_word + 1..row + last_word] {
                *word = !0;
            }
            self.words[row + last_word] |= last_mask;
        }
    }

    fn row(&self, y: u32) -> &[u64] {
        let start = y as usize * self.words_per_row;
        &self.words[start..start + self.words_per_row]
    }
}

/// Everything `render_model` needs to composite one chart into the frame:
/// the chart, its prepared relief, the fixed-point inverse line factoring,
/// and the forward-splat screen bounds of its foreground cells.
struct FrameChart<'a> {
    chart: &'a Chart,
    relief: &'a PreparedRelief,
    inverse: FrameInverse,
    facing: FacingCoefficients,
    maximum_relief: f64,
    splat: [SplatAxis; 2],
}

pub fn render_model(
    prepared: &PreparedModel,
    request: &RenderRequest,
) -> Result<FrameBuffer, RenderError> {
    let bounds = prepared.charts.bounds();
    (request.width as usize)
        .checked_mul(request.height as usize)
        .ok_or(RenderError::FrameBufferTooLarge)?;
    let mut frame = FrameBuffer::transparent(request.width, request.height);
    if request.width == 0 || request.height == 0 {
        return Ok(frame);
    }

    let Some(TargetExtents {
        min_x,
        max_x,
        min_y,
        max_y,
    }) = projected_extents(bounds, &prepared.charts, &request.target)
    else {
        return Ok(frame);
    };
    let offset_x = Ratio::new(i64::from(request.width), 2) - (min_x + max_x) / 2;
    let offset_y = Ratio::new(i64::from(request.height), 2) - (min_y + max_y) / 2;
    // `screen_x = (2x+1)/2 - offset_x = x + (1/2 - offset_x) = x + sx0`, so the
    // integer pixel column `x` enters the inverse line as an integer offset and
    // `sx0` is a frame constant folded into the fixed-point affine forms.
    let sx0 = Ratio::new(1, 2) - offset_x;
    let sy0 = Ratio::new(1, 2) - offset_y;
    // `prepare_inverse` returns `None` exactly when the per-pixel inverse solve
    // would have for every pixel (the singular condition depends only on the
    // matrix, not the screen coordinates), so a chart that fails it contributes
    // nothing to any pixel and is dropped from the frame entirely.
    let frame_charts: Vec<FrameChart> = prepared
        .charts
        .charts()
        .iter()
        .zip(prepared.reliefs.iter())
        .filter_map(|(chart, entry)| {
            let warp = request.target.warp_coefficients(chart.view(), bounds);
            let inverse =
                warp.prepare_inverse()?
                    .inverse_frame(sx0, sy0, request.width, request.height);
            let facing = request.target.facing_coefficients(chart.view(), bounds);
            let deltas = source_evaluation_error_bounds(&inverse, request.width, request.height);
            let screen = warp.screen();
            let parallax = warp.parallax();
            let splat = [
                SplatAxis::new(
                    screen[0],
                    parallax[0],
                    sx0,
                    deltas,
                    entry.maximum_relief,
                    &entry.relief,
                ),
                SplatAxis::new(
                    screen[1],
                    parallax[1],
                    sy0,
                    deltas,
                    entry.maximum_relief,
                    &entry.relief,
                ),
            ];
            Some(FrameChart {
                chart,
                relief: &entry.relief,
                inverse,
                facing,
                maximum_relief: entry.maximum_relief,
                splat,
            })
        })
        .collect();

    let mut scratch = RenderScratch::default();
    let mut mask = PixelMask::new();

    // Forward splat: for each chart, mark the conservative screen footprint
    // of every foreground cell's swept box, then run the unchanged per-pixel
    // inverse solve only on the marked pixels.
    //
    // Exactness: a fragment is committed for (pixel, chart) only when a root
    // passes `solve_preimages`' acceptance box check, which pins the root's
    // f64 source coordinates inside a foreground cell's box inflated by
    // `COORDINATE_EPSILON` and its f64 relief inside `[0, maximum_relief]`
    // inflated likewise. The exact line point at that parameter is within the
    // `source_evaluation_error_bounds` of those f64 values, and the exact
    // line point maps forward to exactly the pixel's screen coordinate (the
    // inverse line is the exact solution set of the warp equations). The
    // pixel's screen coordinate therefore lies inside the zonotope bbox of
    // the inflated swept box, which the `SplatAxis` ranges cover
    // conservatively — so every pixel outside the mask provably commits
    // nothing, and every pixel inside runs the identical whole-ray solve the
    // exhaustive scan ran, producing identical fragments. Chart-major commit
    // order cannot change the frame: `commit_fragment` keeps the strictly
    // smallest `FragmentKey` and equal keys imply the same chart (ranks are
    // unique per resolved chart) and the same source texel, hence the same
    // color.
    for pass in &frame_charts {
        mask.reset(request.width, request.height);
        let middle_relief = 0.5 * pass.maximum_relief;
        for source_y in 0..pass.relief.height {
            for source_x in 0..pass.relief.width {
                if !pass.relief.is_foreground(source_x, source_y) {
                    continue;
                }
                let cell_x = f64::from(source_x);
                let cell_y = f64::from(source_y);
                let Some((first_x, last_x)) =
                    pass.splat[0].pixel_range(cell_x, cell_y, middle_relief, request.width)
                else {
                    continue;
                };
                let Some((first_y, last_y)) =
                    pass.splat[1].pixel_range(cell_x, cell_y, middle_relief, request.height)
                else {
                    continue;
                };
                for y in first_y..=last_y {
                    mask.mark_row(y, first_x, last_x);
                }
            }
        }

        for y in 0..request.height {
            for (word_index, &word) in mask.row(y).iter().enumerate() {
                let mut bits = word;
                while bits != 0 {
                    let x = word_index as u32 * 64 + bits.trailing_zeros();
                    bits &= bits - 1;
                    let coefficients = pass.inverse.variable_f64(x, y);

                    solve_preimages(
                        &mut scratch,
                        pass.relief,
                        &coefficients,
                        pass.facing,
                        pass.maximum_relief,
                    );
                    for preimage in &scratch.preimages {
                        let Some(DecodedTexel::Relief { rgb, .. }) =
                            pass.chart.texel_at(preimage.source_x, preimage.source_y)
                        else {
                            continue;
                        };
                        commit_fragment(
                            &mut frame,
                            x,
                            y,
                            FragmentKey {
                                depth: pass.inverse.depth_at(
                                    x,
                                    y,
                                    Ratio::new(preimage.parameter, ROOT_SCALE),
                                ),
                                chart_rank: pass.chart.view().rank(),
                                source_y: preimage.source_y,
                                source_x: preimage.source_x,
                            },
                            rgb,
                        );
                    }
                }
            }
        }
    }

    Ok(frame)
}

fn projected_extents(
    bounds: Bounds,
    charts: &ResolvedCharts,
    target: &TargetView,
) -> Option<TargetExtents> {
    charts
        .charts()
        .first()
        .map(|_| target.framing_extents(bounds))
}

/// Fills `scratch.preimages` with the preimages of `line` under `field`,
/// using `scratch.boundaries` as working storage for the segment-boundary
/// parameter set. Both buffers are cleared on entry and, on every return
/// path, hold the complete result for this call (never a stale mix with a
/// previous call) — the caller reads `scratch.preimages` after this returns.
fn solve_preimages(
    scratch: &mut RenderScratch,
    field: &PreparedRelief,
    coefficients: &[[f64; 2]; 3],
    facing: FacingCoefficients,
    maximum_relief: f64,
) {
    let RenderScratch {
        boundaries,
        preimages,
    } = scratch;
    boundaries.clear();
    preimages.clear();

    let [
        [x_offset, x_slope],
        [y_offset, y_slope],
        [relief_offset, relief_slope],
    ] = *coefficients;
    let (width, height) = (field.width, field.height);
    let mut parameter_range = [f64::NEG_INFINITY, f64::INFINITY];
    if !clip_affine_range(
        &mut parameter_range,
        x_offset,
        x_slope,
        0.0,
        f64::from(width),
    ) || !clip_affine_range(
        &mut parameter_range,
        y_offset,
        y_slope,
        0.0,
        f64::from(height),
    ) || !clip_affine_range(
        &mut parameter_range,
        relief_offset,
        relief_slope,
        0.0,
        maximum_relief,
    ) {
        return;
    }
    let [range_start, range_end] = parameter_range;
    if !range_start.is_finite() || !range_end.is_finite() {
        return;
    }

    merge_boundaries(
        boundaries,
        range_start,
        range_end,
        GridCrossings::new(x_offset, x_slope, width, range_start, range_end),
        GridCrossings::new(y_offset, y_slope, height, range_start, range_end),
    );
    boundaries.dedup_by(|left, right| (*left - *right).abs() <= COORDINATE_EPSILON);

    for window in boundaries.windows(2) {
        let start = window[0];
        let end = window[1];
        if end - start <= COORDINATE_EPSILON {
            continue;
        }
        let middle = (start + end) * 0.5;
        let middle_x = x_offset + x_slope * middle;
        let middle_y = y_offset + y_slope * middle;

        for source_y in containing_cells(middle_y, height) {
            for source_x in containing_cells(middle_x, width) {
                if !field.is_foreground(source_x, source_y) {
                    continue;
                }
                for quadrant in field.quadrants_at(source_x, source_y, middle_x, middle_y) {
                    // Residual of the ray against the patch surface in the
                    // segment's unit variable `s` (`parameter = start + span * s`):
                    // `g(s) = relief_line(s) - bilinear(u(s), v(s))`. The
                    // bilinear expands to `c00 + gu*u + gv*v + cross*u*v` and
                    // `u`, `v` are affine in `s` (the quadrant's unit
                    // coordinates are the source coordinates scaled by 2 and
                    // shifted by the quadrant half), so `g` is quadratic in
                    // `s` with the closed-form coefficients assembled below.
                    let corners = field.patch_corners(source_x, source_y, quadrant);
                    let span = end - start;
                    let right = quadrant % 2 == 1;
                    let bottom = quadrant / 2 == 1;
                    let u0 = 2.0 * (x_offset + x_slope * start - f64::from(source_x))
                        - if right { 1.0 } else { 0.0 };
                    let v0 = 2.0 * (y_offset + y_slope * start - f64::from(source_y))
                        - if bottom { 1.0 } else { 0.0 };
                    let du = 2.0 * x_slope * span;
                    let dv = 2.0 * y_slope * span;
                    let gu = corners[1] - corners[0];
                    let gv = corners[2] - corners[0];
                    let cross = corners[0] - corners[1] - corners[2] + corners[3];
                    let polynomial = [
                        relief_offset + relief_slope * start
                            - (corners[0] + gu * u0 + gv * v0 + cross * u0 * v0),
                        relief_slope * span - (gu * du + gv * dv + cross * (u0 * dv + du * v0)),
                        -(cross * du * dv),
                    ];

                    for unit in roots_in_unit_interval(polynomial, start, end) {
                        let parameter = start + (end - start) * unit;
                        let x = x_offset + x_slope * parameter;
                        let y = y_offset + y_slope * parameter;
                        let relief = relief_offset + relief_slope * parameter;
                        if x < f64::from(source_x) - COORDINATE_EPSILON
                            || x > f64::from(source_x) + 1.0 + COORDINATE_EPSILON
                            || y < f64::from(source_y) - COORDINATE_EPSILON
                            || y > f64::from(source_y) + 1.0 + COORDINATE_EPSILON
                            || relief < -COORDINATE_EPSILON
                            || relief > maximum_relief + COORDINATE_EPSILON
                        {
                            continue;
                        }
                        if !branch_faces_camera(field, source_x, source_y, quadrant, facing, x, y) {
                            continue;
                        }
                        preimages.push(Preimage {
                            parameter: quantized_parameter(parameter),
                            source_x,
                            source_y,
                        });
                    }
                }
            }
        }
    }

    preimages.sort_by(|left, right| {
        (left.source_y, left.source_x)
            .cmp(&(right.source_y, right.source_x))
            .then_with(|| left.parameter.cmp(&right.parameter))
    });
    preimages.dedup_by(|left, right| {
        left.source_x == right.source_x
            && left.source_y == right.source_y
            && (parameter_f64(left.parameter) - parameter_f64(right.parameter)).abs() <= 1.0e-7
    });
}

fn branch_faces_camera(
    field: &PreparedRelief,
    source_x: u32,
    source_y: u32,
    quadrant: usize,
    facing: FacingCoefficients,
    x: f64,
    y: f64,
) -> bool {
    let evaluate = |x: f64, y: f64| {
        let [relief_x, relief_y] = field.patch_gradient(source_x, source_y, quadrant, x, y);
        facing.evaluate(relief_x, relief_y)
    };
    let value = evaluate(x, y);
    if value > POLYNOMIAL_EPSILON {
        return true;
    }
    if value < -POLYNOMIAL_EPSILON {
        return false;
    }

    let right = quadrant % 2 == 1;
    let bottom = quadrant / 2 == 1;
    let x_min = f64::from(source_x) + if right { 0.5 } else { 0.0 };
    let x_max = x_min + 0.5;
    let y_min = f64::from(source_y) + if bottom { 0.5 } else { 0.0 };
    let y_max = y_min + 0.5;
    let step = 1.0e-7;
    [
        ((x - step).max(x_min), y),
        ((x + step).min(x_max), y),
        (x, (y - step).max(y_min)),
        (x, (y + step).min(y_max)),
    ]
    .into_iter()
    .any(|(probe_x, probe_y)| {
        (probe_x != x || probe_y != y) && evaluate(probe_x, probe_y) > POLYNOMIAL_EPSILON
    })
}

fn clip_affine_range(
    range: &mut [f64; 2],
    offset: f64,
    slope: f64,
    minimum: f64,
    maximum: f64,
) -> bool {
    if slope.abs() <= COORDINATE_EPSILON {
        return offset >= minimum - COORDINATE_EPSILON && offset <= maximum + COORDINATE_EPSILON;
    }
    let first = (minimum - offset) / slope;
    let second = (maximum - offset) / slope;
    range[0] = range[0].max(first.min(second));
    range[1] = range[1].min(first.max(second));
    range[0] <= range[1] + COORDINATE_EPSILON
}

/// Enumerates one source axis's half-texel grid-line crossings of the ray as
/// ray parameters, in ascending `f64::total_cmp` order, without allocating.
///
/// Each crossing is `(coordinate - offset) / slope` with
/// `coordinate = f64::from(half_step) * 0.5` for `half_step in 0..=extent*2` —
/// the identical `f64` expression the collect-and-sort predecessor used, on the
/// identical operand values, so every emitted crossing is bit-identical.
///
/// The parameter is a monotone (rounding-preserving) function of `coordinate`:
/// increasing when `slope > 0`, decreasing when `slope < 0`. Iterating the
/// half-step index in the matching direction (ascending for positive slope,
/// descending for negative) therefore yields crossings already in
/// `total_cmp` order — including the `-0.0`/`+0.0` boundary, where the negative
/// side (smaller `total_cmp`) is always reached first. Only crossings strictly
/// inside `(range_start, range_end)` are emitted, matching the predecessor's
/// strict interior filter; a negligible slope yields no crossings.
struct GridCrossings {
    offset: f64,
    slope: f64,
    range_start: f64,
    range_end: f64,
    lo: u32,
    hi: u32,
    forward: bool,
    active: bool,
    exhausted: bool,
}

impl GridCrossings {
    fn new(offset: f64, slope: f64, extent: u32, range_start: f64, range_end: f64) -> Self {
        Self {
            offset,
            slope,
            range_start,
            range_end,
            lo: 0,
            hi: extent * 2,
            forward: slope > 0.0,
            active: slope.abs() > COORDINATE_EPSILON,
            exhausted: false,
        }
    }

    fn next_half_step(&mut self) -> Option<u32> {
        if !self.active || self.exhausted {
            return None;
        }
        let index = if self.forward { self.lo } else { self.hi };
        if self.lo == self.hi {
            self.exhausted = true;
        } else if self.forward {
            self.lo += 1;
        } else {
            self.hi -= 1;
        }
        Some(index)
    }
}

impl Iterator for GridCrossings {
    type Item = f64;

    fn next(&mut self) -> Option<f64> {
        loop {
            let half_step = self.next_half_step()?;
            let coordinate = f64::from(half_step) * 0.5;
            let parameter = (coordinate - self.offset) / self.slope;
            if parameter > self.range_start && parameter < self.range_end {
                return Some(parameter);
            }
        }
    }
}

/// Merges the two per-axis crossing streams (each already in `total_cmp` order)
/// with the two range endpoints into `boundaries`, in `total_cmp` order. The
/// output multiset is exactly `{range_start, range_end}` plus every interior
/// crossing — the same set the predecessor collected — and `total_cmp` order is
/// the same order its `sort_by(f64::total_cmp)` produced, so the subsequent
/// epsilon dedup makes bit-identical decisions. Streams supply identical values
/// only when their `f64` bit patterns are identical, so the choice of source on
/// a tie cannot change which bit pattern survives.
fn merge_boundaries(
    boundaries: &mut Vec<f64>,
    range_start: f64,
    range_end: f64,
    mut x_axis: GridCrossings,
    mut y_axis: GridCrossings,
) {
    boundaries.clear();
    let mut heads = [
        x_axis.next(),
        y_axis.next(),
        Some(range_start),
        Some(range_end),
    ];
    loop {
        let mut best: Option<usize> = None;
        for (index, head) in heads.iter().enumerate() {
            if let Some(value) = *head {
                match best {
                    Some(current)
                        if heads[current].unwrap().total_cmp(&value) != Ordering::Greater => {}
                    _ => best = Some(index),
                }
            }
        }
        let Some(index) = best else { break };
        boundaries.push(heads[index].unwrap());
        heads[index] = match index {
            0 => x_axis.next(),
            1 => y_axis.next(),
            _ => None,
        };
    }
}

/// Bound: `coordinate` addresses a 1-D grid of `extent` unit cells.
/// It is either strictly interior to exactly one cell (1 result), or within
/// `COORDINATE_EPSILON` of an integer boundary shared by exactly two
/// adjacent cells (2 results) — except at the two extreme boundaries (`0`
/// or `extent`), which border only one cell each. Hence at most 2 cells are
/// ever produced by a single call. (`solve_preimages` calls this once for
/// each of the two source axes and takes their Cartesian product, so a
/// texel position touches at most 2 * 2 = 4 source cells overall.)
fn containing_cells(coordinate: f64, extent: u32) -> Bounded<u32, 2> {
    let mut cells = Bounded::new();
    if extent == 0
        || coordinate < -COORDINATE_EPSILON
        || coordinate > f64::from(extent) + COORDINATE_EPSILON
    {
        return cells;
    }

    let rounded = coordinate.round();
    if (coordinate - rounded).abs() <= COORDINATE_EPSILON {
        let boundary = rounded.clamp(0.0, f64::from(extent)) as u32;
        match boundary {
            0 => cells.push(0),
            value if value == extent => cells.push(extent - 1),
            value => {
                cells.push(value - 1);
                cells.push(value);
            }
        }
    } else {
        cells.push((coordinate.floor() as u32).min(extent - 1));
    }
    cells
}

/// Finds the quadratic's roots in its unit variable `[0, 1]`. The segment
/// `[start, end]` — the ray-parameter interval the unit variable is an affine
/// reparameterization of (`parameter = start + (end - start) * unit`) — is
/// passed through to [`correctly_rounded_root`] so each sign-change root is
/// returned as the unit preimage of its provably correctly rounded *parameter*
/// quantum. Direct tolerance hits at partition points are returned as the raw
/// partition values.
///
/// Bound on `partitions`: it holds the interval endpoints `{0, 1}` plus the
/// root of the polynomial's derivative when it falls strictly inside `(0, 1)`
/// — the polynomial's critical point, which splits `[0, 1]` into monotonic
/// pieces. A quadratic's derivative is linear, with at most 1 root, so
/// `partitions` holds at most 2 + 1 = 3 values.
///
/// Bound on the returned roots: consider the `partitions.len() - 1` unit
/// intervals `(partitions[i], partitions[i+1])` in order. A sign-change root
/// is only pushed for interval `i` when its left endpoint `partitions[i]`
/// independently failed the direct zero-hit test (`left_value.abs() >
/// tolerance` is required to proceed) — so charge that push to
/// `partitions[i]`. A direct zero-hit push is charged to the point itself.
/// Every push is thus charged to a distinct partition point (a point is
/// charged at most once: either it is a direct hit, or — having failed
/// that test — it anchors at most one sign-change push for the interval
/// starting there), so the number of entries pushed before the final
/// `dedup_by` can never exceed `partitions.len()`, i.e. at most 3. This is
/// a tighter, algorithm-specific bound than "a quadratic has at most 2
/// roots": that fact constrains the polynomial's true zero set, not the
/// number of *candidate* entries this tolerance-based procedure can push
/// before `dedup_by` collapses coincident candidates.
fn roots_in_unit_interval(mut polynomial: [f64; 3], start: f64, end: f64) -> Bounded<f64, 3> {
    let scale = polynomial
        .iter()
        .fold(1.0_f64, |largest, value| largest.max(value.abs()));
    let tolerance = POLYNOMIAL_EPSILON * scale;
    let mut degree = 2;
    while degree > 0 && polynomial[degree].abs() <= tolerance {
        polynomial[degree] = 0.0;
        degree -= 1;
    }
    if degree == 0 {
        let mut roots = Bounded::new();
        if polynomial[0].abs() <= tolerance {
            roots.push(0.0);
            roots.push(1.0);
        }
        return roots;
    }

    let mut partitions = Bounded::<f64, 3>::new();
    partitions.push(0.0);
    partitions.push(1.0);
    if degree == 2 {
        let critical = -polynomial[1] / (2.0 * polynomial[2]);
        if critical > 0.0 && critical < 1.0 {
            partitions.push(critical);
        }
    }
    partitions.sort_by(f64::total_cmp);
    partitions.dedup_by(|left, right| (*left - *right).abs() <= COORDINATE_EPSILON);

    let evaluate = |value: f64| polynomial[0] + value * (polynomial[1] + value * polynomial[2]);
    let mut roots = Bounded::<f64, 3>::new();
    for &point in &partitions {
        if evaluate(point).abs() <= tolerance {
            roots.push(point);
        }
    }
    for interval in partitions.as_slice().windows(2) {
        let left = interval[0];
        let right = interval[1];
        let left_value = evaluate(left);
        let right_value = evaluate(right);
        if left_value.abs() <= tolerance
            || right_value.abs() <= tolerance
            || left_value.signum() == right_value.signum()
        {
            continue;
        }
        roots.push(
            correctly_rounded_root(
                &polynomial,
                left,
                right,
                left_value,
                right_value,
                start,
                end,
            )
            .0,
        );
    }
    roots.sort_by(f64::total_cmp);
    roots.dedup_by(|left, right| (*left - *right).abs() <= 1.0e-7);
    roots
}

/// Solves for the correctly-rounded ray-parameter quantum of the root of
/// `polynomial` inside the strictly sign-changing unit bracket `[a, b]`.
///
/// The quadratic lives in the segment's unit variable, but the quantity the
/// pipeline stores and renders is the ray parameter
/// `t = start + (end - start) * unit`, quantized to `q = round(t * ROOT_SCALE)`
/// by [`quantized_parameter`]. The root itself is computed closed-form (the
/// cancellation-free q-form), but a closed-form f64 root is merely *near* the
/// true root, not provably rounded to the right quantum — near a tangency the
/// discriminant's cancellation can push it many quanta off. The quantum is
/// therefore *proven* separately (relative to the same f64 Horner evaluation
/// the rest of the solve uses), by polynomial sign tests at parameter-quantum
/// boundaries mapped into the unit variable: `polynomial` is strictly monotone
/// on `[a, b]`, so the sign at a boundary preimage decides exactly whether the
/// root lies below or above that boundary, and a binary search over the
/// bracket's candidate quanta — its first two probes testing the closed-form
/// root's own two boundaries, which confirm it outright in the
/// well-conditioned case — pins the unique correct quantum. The seed only
/// chooses which boundaries are probed first; correctness rests entirely on
/// the sign tests, so any conditioning error in the seed costs probes, never
/// the answer.
///
/// Returns `(unit, probes)` where `unit` is the unit-variable preimage
/// `(q / ROOT_SCALE - start) / span` of the proven quantum `q` — chosen so the
/// caller's parameter reconstruction followed by [`quantized_parameter`]
/// recovers exactly `q` (asserted below) — and `probes` is the number of
/// boundary sign tests performed (exposed only so the tests can certify the
/// [`MAX_QUANTUM_PROBES`] bound).
///
/// Preconditions the caller establishes: `fa = polynomial(a)` and `fb =
/// polynomial(b)` carry strictly opposite signs (a unique root lies in
/// `(a, b)`), and `[a, b]` is a partition interval between consecutive critical
/// points, so `polynomial` is strictly monotone on `[a, b]`. Both are relied on
/// below.
fn correctly_rounded_root(
    polynomial: &[f64; 3],
    a: f64,
    b: f64,
    fa: f64,
    fb: f64,
    start: f64,
    end: f64,
) -> (f64, u32) {
    // `span` must match the caller's `end - start` bit-for-bit so the
    // round-trip assert below exercises the identical expression; the upper
    // bound is the clip-derived fact MAX_QUANTUM_PROBES rests on.
    let span = end - start;
    assert!(
        span > 0.0 && span <= MAX_PARAMETER_SPAN,
        "segment span {span} outside (0, {MAX_PARAMETER_SPAN}]: the clipped parameter-range \
         bound behind MAX_QUANTUM_PROBES does not hold"
    );
    debug_assert!(
        fa.signum() != fb.signum(),
        "correctly_rounded_root requires a strict sign change on [a, b]"
    );
    let evaluate = |x: f64| polynomial[0] + x * (polynomial[1] + x * polynomial[2]);
    let scale = ROOT_SCALE as f64;
    // The caller's exact parameter expression for a unit value.
    let parameter_of = |unit: f64| start + span * unit;
    // Finishes with a proven quantum `q`: the returned unit value is `q`'s
    // unit preimage. The assert certifies the caller's reconstruction recovers
    // `q`: the round trip perturbs `t` from the exact `q / 2^24` by a few ulps
    // of magnitude ~max(|start|, span, |t|) <= ~2^8, i.e. by ~2^-45 — about
    // 2^-21 of a quantum — so `round(t * 2^24)` cannot move off the integer
    // `q`. A violation would mean the f64 domain assumptions are broken, and
    // must be loud, not a silently shifted root.
    let finish = |q: i64, probes: u32| {
        let unit = (q as f64 / scale - start) / span;
        assert!(
            quantized_parameter(parameter_of(unit)) == q,
            "proven quantum {q} does not survive the caller's parameter reconstruction"
        );
        (unit, probes)
    };

    // The root's parameter lies in `[t(a), t(b)]` and rounding is monotone, so
    // its quantum lies in `[q_lo, q_hi]`. Every probe below shrinks this
    // candidate range while preserving that invariant.
    let mut q_lo = quantized_parameter(parameter_of(a));
    let mut q_hi = quantized_parameter(parameter_of(b));
    if q_lo == q_hi {
        // Every parameter in `[t(a), t(b)]` — the root's among them — rounds
        // to the same quantum.
        return finish(q_lo, 0);
    }

    // Sign probe at the rounding boundary between quanta `k` and `k + 1`,
    // `s_k = (k + 1/2) / ROOT_SCALE` (exact: `2k + 1 < 2^33` is f64-exact and
    // the division is by a power of two). `true` means the root's parameter
    // rounds to a quantum `<= k`; `false` means `>= k + 1`. By monotonicity
    // the root lies below `s_k` exactly when the polynomial at `s_k`'s unit
    // preimage carries `fb`'s sign (the boundaries probed all lie inside
    // `[t(a), t(b)]`, so the preimage stays inside the monotone bracket up to
    // f64 mapping noise of ~2^-21 quanta). A zero value is the rounding tie —
    // the root's parameter *is* the boundary, so both neighbours are correct
    // roundings — resolved half away from zero to match
    // `quantized_parameter`'s own tie rule (`f64::round`; parameters are
    // nonnegative here, see MAX_PARAMETER_SPAN), i.e. upward: `false`.
    let fb_sign = fb.signum();
    let below = |k: i64| {
        let boundary = (2 * k + 1) as f64 / (2.0 * scale);
        let value = evaluate((boundary - start) / span);
        value != 0.0 && value.signum() == fb_sign
    };

    // Closed-form root: the numerically stable q-form when the polynomial is
    // genuinely quadratic — `q = -(b + sign(b) * sqrt(disc)) / 2`, roots `q/a`
    // and `c/q`, which never subtracts like-signed quantities — and the exact
    // linear solution otherwise (the caller zeroes a truncated quadratic
    // coefficient, so this case split is structural, not a tolerance of its
    // own). The bracket contains exactly one of the two quadratic roots; the
    // candidate nearer the bracket midpoint is it, up to conditioning noise,
    // and the clamp keeps even a noise-displaced seed inside the candidate
    // range.
    let closed_form = if polynomial[2] == 0.0 {
        -polynomial[0] / polynomial[1]
    } else {
        let discriminant = polynomial[1] * polynomial[1] - 4.0 * polynomial[2] * polynomial[0];
        // A strict sign change guarantees a real root, so a negative computed
        // discriminant can only be cancellation noise near a tangency: clamp
        // to the tangent case.
        let q = -0.5 * (polynomial[1] + polynomial[1].signum() * discriminant.max(0.0).sqrt());
        let midpoint = 0.5 * (a + b);
        if q == 0.0 {
            // `b` and the discriminant both vanished: the double root at the
            // vertex `-b / (2a) = 0`.
            0.0
        } else if (q / polynomial[2] - midpoint).abs() <= (polynomial[0] / q - midpoint).abs() {
            q / polynomial[2]
        } else {
            polynomial[0] / q
        }
    };
    let seed = quantized_parameter(parameter_of(closed_form.clamp(a, b))).clamp(q_lo, q_hi);

    let mut probes = 0u32;
    while q_lo < q_hi {
        assert!(
            probes < MAX_QUANTUM_PROBES,
            "quantum search exceeded {MAX_QUANTUM_PROBES} probes: two seeded probes plus one \
             halving probe per bisection of at most {MAX_PARAMETER_SPAN} * 2^24 + 2 candidate \
             quanta bound the count"
        );
        probes += 1;
        // Probe order: the first probe tests the seed quantum's upper boundary
        // and the second its lower boundary — when the seed is right, those
        // two tests shrink the range to exactly the seed and the loop exits.
        // Every later probe bisects the remaining range. All probe indices are
        // clamped into `[q_lo, q_hi - 1]`, the boundaries that still separate
        // candidates.
        let k = match probes {
            1 => seed.min(q_hi - 1),
            2 => (seed - 1).clamp(q_lo, q_hi - 1),
            _ => q_lo + (q_hi - q_lo) / 2,
        };
        if below(k) {
            q_hi = k;
        } else {
            q_lo = k + 1;
        }
    }
    finish(q_lo, probes)
}

fn ratio_to_f64(value: Ratio<i64>) -> f64 {
    *value.numer() as f64 / *value.denom() as f64
}

/// Quantizes a ray parameter to denominator `ROOT_SCALE = 2^24`, returning the
/// integer numerator `round(t * 2^24)`. The value `t` lies within the clipped
/// parameter range — a sub-interval of `[0, MAX_PARAMETER_SPAN]` (see that
/// constant's derivation) — extended by at most a half-quantum plus the box
/// check epsilons, so `|round(t * 2^24)| <= 253 * 2^24 < 2^32`, well inside
/// `i64` and the `2^53` f64-exact range.
fn quantized_parameter(value: f64) -> i64 {
    (value * ROOT_SCALE as f64).round() as i64
}

/// Converts a quantized parameter numerator back to `f64` as
/// `numerator / ROOT_SCALE`. Both operands are `f64`-exact (see
/// [`quantized_parameter`]), so this is the bit-identical `f64` of the reduced
/// `Ratio::new(numerator, ROOT_SCALE)` the predecessor compared against.
fn parameter_f64(parameter: i64) -> f64 {
    parameter as f64 / ROOT_SCALE as f64
}

#[cfg(test)]
mod tests {
    use num_rational::Ratio;
    use relief_core::{
        AuthoredModel, Bounds, CanonicalView, Chart, DecodedTexel, ReliefField, ResolvedCharts,
        SourcePoint, WarpCoefficients,
    };

    use crate::{
        CameraBasis, FragmentKey, FrameBuffer, PreparedModel, RenderRequest, TargetView,
        framebuffer::commit_fragment, presets::TargetExtents,
    };

    use super::{
        Bounded, MAX_PARAMETER_SPAN, MAX_QUANTUM_PROBES, PixelMask, PreparedRelief, ROOT_SCALE,
        RenderScratch, SplatAxis, correctly_rounded_root, projected_extents, quantized_parameter,
        ratio_to_f64, render_model, roots_in_unit_interval, solve_preimages,
        source_evaluation_error_bounds,
    };

    /// Half of one ray-parameter quantum, exactly `2^-25`: the tolerance the
    /// correctly-rounded-quantum property is asserted against — a proven
    /// quantum's parameter lies within half a quantum of the true root's.
    const HALF_QUANTUM: f64 = 0.5 / ROOT_SCALE as f64;

    /// Exact relief along the inverse line at the quantized ray parameter,
    /// reconstructed from the fixed-point frame's exact per-variable
    /// coefficients: `relief = relief_offset + relief_slope * parameter`.
    fn relief_at(variables: &[[Ratio<i64>; 2]; 3], parameter: i64) -> Ratio<i64> {
        variables[2][0] + variables[2][1] * Ratio::new(parameter, ROOT_SCALE)
    }

    #[test]
    #[should_panic(expected = "Bounded<_, 2> capacity exceeded")]
    fn bounded_push_beyond_capacity_panics_loudly() {
        let mut bounded = Bounded::<u32, 2>::new();
        bounded.push(1);
        bounded.push(2);
        bounded.push(3);
    }

    #[test]
    fn scalar_solver_retains_both_preimages_of_a_fold() {
        // (x - 0.2) * (x - 0.8) = 0.16 - x + x^2: both roots inside (0, 1).
        let roots = roots_in_unit_interval([0.16, -1.0, 1.0], 0.0, 1.0);

        assert_eq!(roots.as_slice().len(), 2);
        assert!((roots.as_slice()[0] - 0.2).abs() < 1.0e-6);
        assert!((roots.as_slice()[1] - 0.8).abs() < 1.0e-6);
    }

    #[test]
    fn scalar_solver_keeps_a_tangent_preimage() {
        // (x - 0.5)^2 = 0.25 - x + x^2: a tangency at the vertex, which is a
        // partition point and therefore a direct tolerance hit.
        let roots = roots_in_unit_interval([0.25, -1.0, 1.0], 0.0, 1.0);

        assert_eq!(roots.as_slice(), [0.5]);
    }

    /// Evaluates `c0 + c1 x + c2 x^2` — a standalone oracle for the
    /// half-quantum sign property, independent of the solver's own `evaluate`.
    fn poly(coefficients: &[f64; 3], x: f64) -> f64 {
        coefficients[0] + x * (coefficients[1] + x * coefficients[2])
    }

    /// The proven parameter quantum for a solver result: the caller's own
    /// reconstruction `start + (end - start) * unit` fed through
    /// [`quantized_parameter`] — the exact pipeline the stored preimage uses.
    fn quantum_of(unit: f64, start: f64, end: f64) -> i64 {
        quantized_parameter(start + (end - start) * unit)
    }

    /// The returned quantum is *proven* to be the correct rounding of the true
    /// root's parameter: the polynomial changes sign across the unit preimages
    /// of the two half-quantum parameter boundaries `q / 2^24 +- 2^-25`, so the
    /// root's parameter lies strictly inside `q`'s quantum. Checked directly
    /// here on `x^2 - 1/2`, whose only root in `(0, 1)` is the irrational
    /// `2^(-1/2)`, over the unit segment where parameter and unit coincide.
    #[test]
    fn correctly_rounded_root_certifies_irrational_root_by_half_quantum_sign_change() {
        let coefficients = [-0.5, 0.0, 1.0];
        let true_root = 2.0_f64.powf(-0.5);
        let (root, probes) = correctly_rounded_root(
            &coefficients,
            0.0,
            1.0,
            poly(&coefficients, 0.0),
            poly(&coefficients, 1.0),
            0.0,
            1.0,
        );
        let parameter =
            f64::from(i32::try_from(quantum_of(root, 0.0, 1.0)).unwrap()) / ROOT_SCALE as f64;

        // Correctly rounded: within half a quantum of the true root's parameter.
        assert!(
            (parameter - true_root).abs() <= HALF_QUANTUM,
            "quantum {parameter} is farther than a half-quantum from {true_root}"
        );
        // Half-quantum sign property: strict sign change across q +- 2^-25.
        let lower = poly(&coefficients, parameter - HALF_QUANTUM);
        let upper = poly(&coefficients, parameter + HALF_QUANTUM);
        assert!(
            lower < 0.0 && upper > 0.0,
            "root not bracketed by its quantum: {lower}, {upper}"
        );
        assert!(probes <= MAX_QUANTUM_PROBES, "took {probes} probes");
    }

    /// The span mismatch this solver exists to handle: over a segment of span
    /// 192 (the largest observed in the fixture set — the globe's relief
    /// range), a unit-variable half-quantum is 96 parameter quanta wide, so
    /// correctness must be certified on the parameter grid. The same
    /// irrational-root quadratic is solved over `[start, end] = [10, 202]` and
    /// the half-quantum sign property is asserted in parameter space.
    #[test]
    fn correctly_rounded_root_certifies_the_parameter_quantum_across_a_wide_span() {
        let (start, end) = (10.0, 202.0);
        let coefficients = [-0.5, 0.0, 1.0];
        let true_unit_root = 2.0_f64.powf(-0.5);
        let true_parameter = start + (end - start) * true_unit_root;
        let (root, probes) = correctly_rounded_root(
            &coefficients,
            0.0,
            1.0,
            poly(&coefficients, 0.0),
            poly(&coefficients, 1.0),
            start,
            end,
        );
        let quantum = quantum_of(root, start, end);
        let parameter = quantum as f64 / ROOT_SCALE as f64;

        // Correctly rounded on the PARAMETER grid: within half a parameter
        // quantum of the true root's parameter.
        assert!(
            (parameter - true_parameter).abs() <= HALF_QUANTUM,
            "quantum {parameter} is farther than a parameter half-quantum from {true_parameter}"
        );
        // Half-quantum sign property, evaluated at the unit preimages of the
        // parameter boundaries q/2^24 +- 2^-25.
        let lower = poly(
            &coefficients,
            (parameter - HALF_QUANTUM - start) / (end - start),
        );
        let upper = poly(
            &coefficients,
            (parameter + HALF_QUANTUM - start) / (end - start),
        );
        assert!(
            lower < 0.0 && upper > 0.0,
            "root's parameter not bracketed by its quantum: {lower}, {upper}"
        );
        assert!(probes <= MAX_QUANTUM_PROBES, "took {probes} probes");
    }

    /// A root whose parameter lands exactly on a representable quantum is
    /// returned exactly. `x^2 - 1/4` has the root `1/2 = 2^23 / 2^24` over the
    /// unit segment, and every quantity in the q-form closed solution is a
    /// power of two, so the seed is exact and its two boundary probes confirm
    /// it with no further search.
    #[test]
    fn correctly_rounded_root_returns_an_exactly_representable_quantum() {
        let coefficients = [-0.25, 0.0, 1.0];
        let (root, probes) = correctly_rounded_root(
            &coefficients,
            0.0,
            1.0,
            poly(&coefficients, 0.0),
            poly(&coefficients, 1.0),
            0.0,
            1.0,
        );

        assert_eq!(
            root, 0.5,
            "exact representable root must be returned exactly"
        );
        assert!(
            probes <= 2,
            "an exact seed must be confirmed by its two boundary probes, took {probes}"
        );
    }

    /// Splits `x * y` into `(product, error)` with `product + error` exact —
    /// the classic two-product via fused multiply-add. Used to build a
    /// reference discriminant accurate far beyond plain f64, so the test can
    /// locate the true root independently of the solver's arithmetic.
    fn two_product(x: f64, y: f64) -> (f64, f64) {
        let product = x * y;
        (product, x.mul_add(y, -product))
    }

    /// Discriminant cancellation: for `(x - 0.65)^2 - 3e-16` (as f64
    /// coefficients), the plain-f64 discriminant `b*b - 4ac` loses the true
    /// value's low bits to the rounding of `b*b`, and the closed-form seed
    /// lands a provably wrong quantum (verified below with a compensated
    /// reference) — the case the boundary-sign binary search exists for. The
    /// widest permitted span magnifies the miss. Near so flat a crossing the
    /// solver's f64 Horner signs are themselves noise-limited, so the proven
    /// quantum is pinned not to a single quantum but to the analytic noise
    /// window `|g| <= noise` around the true root — the strongest claim any
    /// procedure sharing the solver's evaluation can make.
    #[test]
    fn quantum_search_recovers_from_a_cancellation_displaced_seed() {
        let span = MAX_PARAMETER_SPAN;
        let vertex = 0.65_f64;
        let linear = -1.3_f64;
        let constant = vertex * vertex - 3.0e-16;
        let coefficients = [constant, linear, 1.0];
        let fa = poly(&coefficients, vertex);
        let fb = poly(&coefficients, 1.0);
        assert!(
            fa < 0.0 && fb > 0.0,
            "test construction: strict sign change required, got {fa}, {fb}"
        );

        // Reference root of the exact real polynomial with these f64
        // coefficients: T = 0.65 + sqrt(disc)/2, discriminant compensated so
        // the reference error (~1e-16 in the unit variable) is negligible
        // against a quantum.
        let (b_squared, b_squared_error) = two_product(linear, linear);
        let reference_disc = (b_squared - 4.0 * constant) + b_squared_error;
        let reference_root = vertex + 0.5 * reference_disc.sqrt();
        let reference_quantum = quantized_parameter(span * reference_root);

        // The seed the solver computes, reproduced with the identical f64
        // expressions: its quantum misses the reference — this input reaches
        // the bisection probes.
        let seed_disc = linear * linear - 4.0 * constant;
        let seed_q = -0.5 * (linear + linear.signum() * seed_disc.max(0.0).sqrt());
        let seed_root = seed_q; // q / a with a = 1; nearer the bracket midpoint than c/q
        let seed_quantum = quantized_parameter(span * seed_root);
        assert!(
            seed_quantum != reference_quantum,
            "test construction: seed must miss the true quantum, got seed {seed_quantum} vs \
             reference {reference_quantum}"
        );

        let (root, probes) = correctly_rounded_root(&coefficients, vertex, 1.0, fa, fb, 0.0, span);
        assert!(
            probes <= MAX_QUANTUM_PROBES,
            "took {probes} probes, exceeding {MAX_QUANTUM_PROBES}"
        );
        assert!(
            probes > 2,
            "a wrong seed cannot be confirmed by its own two boundary probes; took {probes}"
        );

        // The returned parameter lies inside the noise window: sign probes
        // share the solver's Horner evaluation, whose absolute error for these
        // O(1) coefficients is below `noise = 4 * f64::EPSILON` (three
        // roundings of intermediates of magnitude <= 1.3), so a probe can only
        // misclassify a boundary within `|g| <= noise` of the root, a unit
        // interval of half-width `noise / |g'(root)|` with
        // `g'(root) = sqrt(disc)`.
        let parameter = quantum_of(root, 0.0, span) as f64 / ROOT_SCALE as f64;
        let window = span * (4.0 * f64::EPSILON) / reference_disc.sqrt() + HALF_QUANTUM;
        assert!(
            (parameter - span * reference_root).abs() <= window,
            "returned parameter {parameter} outside the noise window {window} around \
             {}",
            span * reference_root
        );
    }

    /// The `MAX_QUANTUM_PROBES` bound is derived, not tuned: a bracket's
    /// parameter image spans at most `MAX_PARAMETER_SPAN`, so its endpoint
    /// quanta differ by at most `MAX_PARAMETER_SPAN * 2^24 + 1` and the
    /// candidate range holds at most one more than that. Two seeded probes
    /// precede pure bisection, and each bisection probe at least halves the
    /// candidate count, so `MAX_QUANTUM_PROBES - 2` halvings must cover the
    /// range — and one fewer must not, or the bound would be slack.
    #[test]
    fn quantum_probe_bound_is_the_minimal_sufficient_count() {
        let candidates = MAX_PARAMETER_SPAN * ROOT_SCALE as f64 + 2.0;
        assert!(2.0_f64.powi(MAX_QUANTUM_PROBES as i32 - 2) >= candidates);
        assert!(2.0_f64.powi(MAX_QUANTUM_PROBES as i32 - 3) < candidates);
    }

    #[test]
    fn normalized_tent_fold_retains_all_three_source_preimages() {
        let chart = Chart::from_rgba(
            CanonicalView::Front,
            3,
            1,
            vec![[1, 0, 0, 239], [2, 0, 0, 255], [3, 0, 0, 239]],
        )
        .unwrap();
        let field = ReliefField::new(&chart);
        let prepared = PreparedRelief::new(&field);
        let zero = Ratio::from_integer(0);
        let warp = WarpCoefficients::from_rational(
            [
                [Ratio::from_integer(1), zero, zero],
                [zero, Ratio::from_integer(1), zero],
            ],
            [Ratio::new(-1, 8), zero],
            [zero, zero, zero],
            Ratio::from_integer(1),
        );
        let frame =
            warp.prepare_inverse()
                .unwrap()
                .inverse_frame(Ratio::new(3, 4), Ratio::new(1, 2), 1, 1);
        let variables = frame.variable_coefficients_exact(0, 0);
        let coefficients = frame.variable_f64(0, 0);

        let facing = TargetView::front()
            .facing_coefficients(CanonicalView::Front, Bounds::new(3, 1, 4).unwrap());
        let mut scratch = RenderScratch::default();
        solve_preimages(&mut scratch, &prepared, &coefficients, facing, 16.0);
        let locations: Vec<_> = scratch
            .preimages
            .iter()
            .map(|preimage| {
                (
                    preimage.source_x,
                    (ratio_to_f64(relief_at(&variables, preimage.parameter)) * 1_000.0).round()
                        / 1_000.0,
                )
            })
            .collect();

        assert_eq!(locations, vec![(1, 4.0), (2, 12.0), (2, 16.0)]);
    }

    #[test]
    fn locally_reversed_fold_branch_does_not_supply_color() {
        let chart = Chart::from_rgba(
            CanonicalView::Front,
            3,
            1,
            vec![[1, 0, 0, 239], [2, 0, 0, 255], [3, 0, 0, 239]],
        )
        .unwrap();
        let prepared = PreparedRelief::new(&ReliefField::new(&chart));
        let zero = Ratio::from_integer(0);
        let target = TargetView::from_camera(CameraBasis::new(
            [Ratio::from_integer(1), zero, Ratio::from_integer(-1)],
            [zero, Ratio::from_integer(1), zero],
            [Ratio::from_integer(1), zero, Ratio::from_integer(1)],
        ));
        let bounds = Bounds::new(3, 1, 4).unwrap();
        let warp = target.warp_coefficients(CanonicalView::Front, bounds);
        let frame =
            warp.prepare_inverse()
                .unwrap()
                .inverse_frame(Ratio::new(3, 4), Ratio::new(1, 2), 1, 1);
        let variables = frame.variable_coefficients_exact(0, 0);
        let coefficients = frame.variable_f64(0, 0);
        let facing = target.facing_coefficients(CanonicalView::Front, bounds);

        let mut scratch = RenderScratch::default();
        solve_preimages(&mut scratch, &prepared, &coefficients, facing, 16.0);
        let locations: Vec<_> = scratch
            .preimages
            .iter()
            .map(|preimage| {
                (
                    preimage.source_x,
                    (ratio_to_f64(relief_at(&variables, preimage.parameter)) * 1_000.0).round()
                        / 1_000.0,
                )
            })
            .collect();

        assert_eq!(locations, vec![(1, 4.0), (2, 16.0)]);
    }

    #[test]
    fn pixel_mask_marks_ranges_within_a_single_word() {
        let mut mask = PixelMask::new();
        mask.reset(10, 2);
        mask.mark_row(1, 3, 5);

        assert_eq!(mask.row(0), [0]);
        assert_eq!(mask.row(1), [0b111000]);
    }

    #[test]
    fn pixel_mask_marks_ranges_spanning_multiple_words() {
        let mut mask = PixelMask::new();
        mask.reset(200, 1);
        mask.mark_row(0, 60, 130);

        assert_eq!(
            mask.row(0),
            [!0_u64 << 60, !0, !0 >> (63 - 130 % 64), 0],
            "bits 60..=130 must be set across words 0..=2"
        );
    }

    #[test]
    fn pixel_mask_marking_is_idempotent_and_reset_clears() {
        let mut mask = PixelMask::new();
        mask.reset(70, 1);
        mask.mark_row(0, 0, 69);
        let marked: Vec<u64> = mask.row(0).to_vec();
        mask.mark_row(0, 10, 63);

        assert_eq!(mask.row(0), marked, "overlapping marks must be idempotent");

        mask.reset(70, 1);
        assert_eq!(mask.row(0), [0, 0], "reset must clear every word");
    }

    /// A model with background holes and two charts: enough structure that
    /// forward splatting must cull (holes, off-footprint pixels) and must
    /// composite across charts, while staying small enough for the
    /// exhaustive-scan oracle below.
    fn perforated_two_chart_model() -> ResolvedCharts {
        let bounds = Bounds::new(3, 2, 4).unwrap();
        let hole = [0, 0, 0, 0];
        let front = Chart::from_rgba(
            CanonicalView::Front,
            3,
            2,
            vec![
                [10, 0, 0, 239],
                [20, 0, 0, 255],
                [30, 0, 0, 239],
                [40, 0, 0, 255],
                hole,
                [50, 0, 0, 247],
            ],
        )
        .unwrap();
        let top = Chart::from_rgba(
            CanonicalView::Top,
            3,
            4,
            vec![
                [60, 0, 0, 251],
                hole,
                [70, 0, 0, 247],
                [80, 0, 0, 255],
                [90, 0, 0, 249],
                hole,
                hole,
                [100, 0, 0, 253],
                [110, 0, 0, 255],
                [120, 0, 0, 247],
                [130, 0, 0, 251],
                [140, 0, 0, 255],
            ],
        )
        .unwrap();
        AuthoredModel::new(bounds, vec![front, top])
            .expect("test model must validate")
            .resolve()
    }

    /// The pre-splat renderer — every pixel x every chart, no culling — used
    /// as the oracle that forward splatting is output-identical, fragment
    /// keys (exact rational depths) included.
    fn render_reference(prepared: &PreparedModel, request: &RenderRequest) -> FrameBuffer {
        let bounds = prepared.charts.bounds();
        let mut frame = FrameBuffer::transparent(request.width, request.height);
        let Some(TargetExtents {
            min_x,
            max_x,
            min_y,
            max_y,
        }) = projected_extents(bounds, &prepared.charts, &request.target)
        else {
            return frame;
        };
        let offset_x = Ratio::new(i64::from(request.width), 2) - (min_x + max_x) / 2;
        let offset_y = Ratio::new(i64::from(request.height), 2) - (min_y + max_y) / 2;
        let sx0 = Ratio::new(1, 2) - offset_x;
        let sy0 = Ratio::new(1, 2) - offset_y;
        let frame_charts: Vec<_> = prepared
            .charts
            .charts()
            .iter()
            .zip(prepared.reliefs.iter())
            .filter_map(|(chart, entry)| {
                let warp = request.target.warp_coefficients(chart.view(), bounds);
                let inverse =
                    warp.prepare_inverse()?
                        .inverse_frame(sx0, sy0, request.width, request.height);
                let facing = request.target.facing_coefficients(chart.view(), bounds);
                Some((chart, &entry.relief, inverse, facing, entry.maximum_relief))
            })
            .collect();

        let mut scratch = RenderScratch::default();
        for y in 0..request.height {
            for x in 0..request.width {
                for (chart, relief, inverse, facing, maximum_relief) in &frame_charts {
                    let coefficients = inverse.variable_f64(x, y);
                    solve_preimages(
                        &mut scratch,
                        relief,
                        &coefficients,
                        *facing,
                        *maximum_relief,
                    );
                    for preimage in &scratch.preimages {
                        let Some(DecodedTexel::Relief { rgb, .. }) =
                            chart.texel_at(preimage.source_x, preimage.source_y)
                        else {
                            continue;
                        };
                        commit_fragment(
                            &mut frame,
                            x,
                            y,
                            FragmentKey {
                                depth: inverse.depth_at(
                                    x,
                                    y,
                                    Ratio::new(preimage.parameter, ROOT_SCALE),
                                ),
                                chart_rank: chart.view().rank(),
                                source_y: preimage.source_y,
                                source_x: preimage.source_x,
                            },
                            rgb,
                        );
                    }
                }
            }
        }
        frame
    }

    fn assert_frames_identical(rendered: &FrameBuffer, reference: &FrameBuffer, context: &str) {
        assert_eq!(rendered.width(), reference.width(), "{context}: width");
        assert_eq!(rendered.height(), reference.height(), "{context}: height");
        for y in 0..reference.height() {
            for x in 0..reference.width() {
                assert_eq!(
                    rendered.rgba_at(x, y),
                    reference.rgba_at(x, y),
                    "{context}: rgba at ({x}, {y})"
                );
                assert_eq!(
                    rendered.owner_at(x, y),
                    reference.owner_at(x, y),
                    "{context}: fragment owner at ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn forward_splat_matches_the_exhaustive_pixel_scan() {
        let charts = perforated_two_chart_model();
        let prepared = PreparedModel::new(&charts);
        let zero = Ratio::from_integer(0);
        let targets = [
            ("front", TargetView::front()),
            ("isometric", TargetView::isometric()),
            ("bowl_acceptance", TargetView::bowl_acceptance()),
            (
                "reversed_fold",
                TargetView::from_camera(CameraBasis::new(
                    [Ratio::from_integer(1), zero, Ratio::from_integer(-1)],
                    [zero, Ratio::from_integer(1), zero],
                    [Ratio::from_integer(1), zero, Ratio::from_integer(1)],
                )),
            ),
        ];
        for (name, target) in targets {
            for (width, height) in [(40, 32), (9, 7)] {
                let request = RenderRequest::new(width, height, target.clone());
                let rendered = render_model(&prepared, &request).expect("render must succeed");
                let reference = render_reference(&prepared, &request);
                assert_frames_identical(
                    &rendered,
                    &reference,
                    &format!("target {name}, frame {width}x{height}"),
                );
            }
        }
    }

    #[test]
    fn forward_splat_leaves_an_all_background_model_transparent() {
        let bounds = Bounds::new(3, 2, 4).unwrap();
        let charts = AuthoredModel::with_empty_chart(bounds, CanonicalView::Front)
            .expect("empty model must validate")
            .resolve();
        let prepared = PreparedModel::new(&charts);
        let request = RenderRequest::new(16, 16, TargetView::isometric());
        let rendered = render_model(&prepared, &request).expect("render must succeed");

        for y in 0..rendered.height() {
            for x in 0..rendered.width() {
                assert_eq!(rendered.rgba_at(x, y), [0, 0, 0, 0], "pixel ({x}, {y})");
                assert!(rendered.owner_at(x, y).is_none(), "owner at ({x}, {y})");
            }
        }
    }

    /// The conservative pixel ranges must cover the exact rational forward
    /// image of every foreground cell's swept box: for each cell and screen
    /// axis, every integer pixel index whose exact screen coordinate lies
    /// inside the exact zonotope bbox (computed from the eight exact corner
    /// images via `WarpCoefficients::apply`) must be inside the range.
    #[test]
    fn splat_axis_ranges_cover_the_exact_swept_box_image() {
        let charts = perforated_two_chart_model();
        let prepared = PreparedModel::new(&charts);
        let bounds = charts.bounds();
        let target = TargetView::isometric();
        let (width, height) = (64_u32, 64_u32);
        let request = RenderRequest::new(width, height, target.clone());
        let TargetExtents {
            min_x,
            max_x,
            min_y,
            max_y,
        } = projected_extents(bounds, &charts, &request.target).expect("model has charts");
        let offset_x = Ratio::new(i64::from(width), 2) - (min_x + max_x) / 2;
        let offset_y = Ratio::new(i64::from(height), 2) - (min_y + max_y) / 2;
        let sx0 = Ratio::new(1, 2) - offset_x;
        let sy0 = Ratio::new(1, 2) - offset_y;

        for (chart, entry) in prepared.charts.charts().iter().zip(prepared.reliefs.iter()) {
            let warp = target.warp_coefficients(chart.view(), bounds);
            let inverse = warp
                .prepare_inverse()
                .expect("isometric warp is invertible")
                .inverse_frame(sx0, sy0, width, height);
            let deltas = source_evaluation_error_bounds(&inverse, width, height);
            let screen = warp.screen();
            let parallax = warp.parallax();
            let axes = [
                SplatAxis::new(
                    screen[0],
                    parallax[0],
                    sx0,
                    deltas,
                    entry.maximum_relief,
                    &entry.relief,
                ),
                SplatAxis::new(
                    screen[1],
                    parallax[1],
                    sy0,
                    deltas,
                    entry.maximum_relief,
                    &entry.relief,
                ),
            ];
            let origins = [sx0, sy0];
            let extents = [width, height];
            let maximum_relief =
                Ratio::from_integer(i64::from(chart.view().maximum_inward_depth(bounds)));
            let middle_relief = 0.5 * entry.maximum_relief;

            for source_y in 0..entry.relief.height {
                for source_x in 0..entry.relief.width {
                    if !entry.relief.is_foreground(source_x, source_y) {
                        continue;
                    }
                    let mut corners = Vec::new();
                    for corner_x in [i64::from(source_x), i64::from(source_x) + 1] {
                        for corner_y in [i64::from(source_y), i64::from(source_y) + 1] {
                            for relief in [Ratio::from_integer(0), maximum_relief] {
                                corners.push(warp.apply(
                                    SourcePoint::new(
                                        Ratio::from_integer(corner_x),
                                        Ratio::from_integer(corner_y),
                                    ),
                                    relief,
                                ));
                            }
                        }
                    }
                    for (index, axis) in axes.iter().enumerate() {
                        let origin = origins[index];
                        let extent = &extents[index];
                        let screen_values: Vec<Ratio<i64>> = corners
                            .iter()
                            .map(|sample| {
                                if index == 0 {
                                    sample.screen_x
                                } else {
                                    sample.screen_y
                                }
                            })
                            .collect();
                        let minimum = *screen_values.iter().min().unwrap() - origin;
                        let maximum = *screen_values.iter().max().unwrap() - origin;
                        let exact_first = minimum.ceil().to_integer();
                        let exact_last = maximum.floor().to_integer();
                        if exact_last < 0 || exact_first >= i64::from(*extent) {
                            continue;
                        }
                        let (first, last) = axis
                            .pixel_range(
                                f64::from(source_x),
                                f64::from(source_y),
                                middle_relief,
                                *extent,
                            )
                            .expect("in-frame swept box must yield a pixel range");
                        assert!(
                            i64::from(first) <= exact_first.max(0),
                            "cell ({source_x}, {source_y}): range start {first} misses exact {exact_first}"
                        );
                        assert!(
                            i64::from(last) >= exact_last.min(i64::from(*extent) - 1),
                            "cell ({source_x}, {source_y}): range end {last} misses exact {exact_last}"
                        );
                    }
                }
            }
        }
    }
}
