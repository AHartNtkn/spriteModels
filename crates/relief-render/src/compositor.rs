use num_rational::Ratio;
use relief_core::{
    Bounds, Chart, DecodedTexel, FrameInverse, ReliefField, ResolvedCharts, SourcePoint,
};
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
/// and those survive to the f64 coefficients exactly), and
/// [`cell_intersections`] clips the parameter range so every source variable
/// — the free one included — stays inside the patch's box, itself a sub-box
/// of the free variable's own box. The clipped range is therefore contained
/// in the free variable's box: source coordinates span at most 63
/// (`Bounds::new` rejects sides outside `1..=63`) and relief spans at most
/// `CanonicalView::maximum_inward_depth = 4 * opposing_axis <= 4 * 63`. Hence
/// `4 * 63 = 252`, and every patch's clipped interval `[start, end]` — a
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

/// Upper bound on `|t|` for every ray parameter at which
/// [`cell_intersections`] evaluates the root-acceptance box checks. The
/// clipped parameter range is a sub-interval of `[0, MAX_PARAMETER_SPAN]`
/// (the patch box is a sub-box of the free variable's own box `[0, extent]`
/// with `extent <= 252`, and the free variable has offset `0.0` and slope
/// `1.0`, so clipping against it yields sub-bounds of the box
/// itself). Direct partition hits reconstruct `t` inside `[start, end]`, and
/// bisected roots reconstruct the unit preimage of a parameter quantum within
/// half a quantum (`2^-25`) of a bracket endpoint inside `[start, end]`,
/// perturbed by a few ulps of f64 reconstruction noise. The slack of `1`
/// therefore dominates every excursion outside the clipped range by more than
/// seven orders of magnitude.
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

/// One quadrant patch of a foreground cell: the relief surface values at its
/// four half-texel corners, each resolved at prepare time as the f64 quotient
/// `weighted / total` of the exact-rational hat-kernel terms sampled at that
/// corner. The patch surface is the plain bilinear interpolant of these
/// values, so the ray-surface equation is quadratic in the ray parameter.
#[derive(Clone, Copy, Debug)]
struct ReliefPatch {
    corners: [f64; 4],
}

/// One prepared foreground cell: its four quadrant patches plus the exact
/// range of the cell's surface — the minimum and maximum of the sixteen patch
/// corner values. The bilinear interpolant of each patch attains its extrema
/// at corners, so `[relief_min, relief_max]` contains every surface value of
/// the cell, which is what bounds the relief extent of the cell's swept
/// screen footprint (see [`relief_residual_slack`]).
#[derive(Clone, Copy, Debug)]
struct PreparedCell {
    patches: [ReliefPatch; 4],
    relief_min: f64,
    relief_max: f64,
}

#[derive(Clone, Debug)]
struct PreparedRelief {
    width: u32,
    height: u32,
    cells: Vec<Option<PreparedCell>>,
    /// Chart-wide maximum of `relief_max - relief_min` over foreground cells.
    max_cell_spread: f64,
    /// Chart-wide maximum of `|corner|` over all patch corners.
    max_corner_magnitude: f64,
}

impl PreparedRelief {
    fn new(field: &ReliefField) -> Self {
        let (width, height) = field.dimensions();
        let mut cells = Vec::with_capacity((width * height) as usize);
        let mut max_cell_spread = 0.0_f64;
        let mut max_corner_magnitude = 0.0_f64;
        for y in 0..height {
            for x in 0..width {
                cells.push(field.foreground_cell(x, y).map(|cell| {
                    let patches: [ReliefPatch; 4] = std::array::from_fn(|quadrant| {
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
                    });
                    let mut relief_min = f64::INFINITY;
                    let mut relief_max = f64::NEG_INFINITY;
                    for patch in &patches {
                        for &corner in &patch.corners {
                            relief_min = relief_min.min(corner);
                            relief_max = relief_max.max(corner);
                        }
                    }
                    max_cell_spread = max_cell_spread.max(relief_max - relief_min);
                    max_corner_magnitude = max_corner_magnitude
                        .max(relief_min.abs())
                        .max(relief_max.abs());
                    PreparedCell {
                        patches,
                        relief_min,
                        relief_max,
                    }
                }));
            }
        }
        Self {
            width,
            height,
            cells,
            max_cell_spread,
            max_corner_magnitude,
        }
    }

    fn is_foreground(&self, x: u32, y: u32) -> bool {
        self.cells[(y * self.width + x) as usize].is_some()
    }

    /// The exact range of the cell's surface values: min and max over the
    /// sixteen patch corners (see [`PreparedCell`]).
    fn relief_bounds(&self, x: u32, y: u32) -> (f64, f64) {
        let cell = self.cells[(y * self.width + x) as usize]
            .expect("only foreground cells reach analytic evaluation");
        (cell.relief_min, cell.relief_max)
    }

    fn patch_corners(&self, source_x: u32, source_y: u32, quadrant: usize) -> [f64; 4] {
        self.cells[(source_y * self.width + source_x) as usize]
            .expect("only foreground cells reach analytic evaluation")
            .patches[quadrant]
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
/// Preparation costs on the order of a full frame render or more, so callers
/// that re-render the same model under different orientations should build
/// this once per model revision and reuse it across every `render_model`
/// call rather than rebuilding it per frame.
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

/// Frame-wide line magnitude facts, headline among them the per-source-variable
/// upper bound (`deltas`) on the absolute error between the f64 line
/// evaluation `offset + slope * t` performed by [`cell_intersections`] and the
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
fn source_evaluation_error_bounds(
    inverse: &FrameInverse,
    width: u32,
    height: u32,
) -> FrameLineBounds {
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
    FrameLineBounds {
        deltas: std::array::from_fn(|variable| {
            8.0 * f64::EPSILON * (offset_bound[variable] + slope[variable] * MAX_ACCEPTED_PARAMETER)
        }),
        offset_bounds: offset_bound,
        slope_bounds: slope,
    }
}

/// Frame-wide magnitude facts about a chart's inverse lines, all maximized
/// over the pixel rectangle (offsets are affine in the pixel coordinates, so
/// the four corners cover every pixel; slopes are frame constants).
struct FrameLineBounds {
    /// The [`source_evaluation_error_bounds`] deltas per source variable.
    deltas: [f64; 3],
    /// Upper bounds on `|offset|` per source variable.
    offset_bounds: [f64; 3],
    /// `|slope|` per source variable.
    slope_bounds: [f64; 3],
}

/// Upper bound `R` on how far outside a cell's surface range
/// `[relief_min, relief_max]` the f64 relief-line value of an *accepted* root
/// can lie — the quantity that lets the forward splat sweep each cell's
/// footprint over its actual surface band instead of the chart's whole
/// `[0, maximum_relief]` depth prism.
///
/// Every parameter `t` that [`cell_intersections`] accepts came out of
/// [`roots_in_unit_interval`] for some patch quadratic `P` (assembled from
/// the f64 line coefficients and the patch's f64 corners) over a clipped
/// interval inside the patch's box, in one of these ways, with
/// `tol = POLYNOMIAL_EPSILON * scale <= POLYNOMIAL_EPSILON * K`:
///
/// - a direct tolerance hit: `|P(s)| <= tol`;
/// - a truncated-to-constant polynomial: `|P(s)| <= 3 * tol` everywhere (the
///   constant term passed the tolerance test and the two truncated
///   coefficients were each within `tol` of zero);
/// - a sign-verified quantum of a true root `s*` of `P`: `P(s*) = 0` and the
///   accepted parameter is within `2^-24` of `t(s*)` (the proven quantum is
///   the correct rounding of the root's parameter — within a half quantum,
///   `2^-25` — and the unit round trip perturbs it by ulps; `2^-24` covers
///   both, see [`correctly_rounded_root`]).
///
/// Writing `g(t) = relief_line(t) - B(u(t), v(t))` for the *exactly
/// evaluated* residual of the f64 line against the patch bilinear `B`, the
/// accepted parameter therefore satisfies
/// `|g(t)| <= 3*tol + A + G * 2^-24`, where `A` bounds the f64 assembly and
/// Horner evaluation error `|P - g|` on the unit interval and `G` bounds
/// `|dg/dt|`. Acceptance further pins `(x, y)(t)` inside the patch box
/// inflated by `COORDINATE_EPSILON`, so `(u, v)` lie within `4 *
/// COORDINATE_EPSILON` outside `[0, 1]^2`; the bilinear's basis weights still
/// sum to exactly 1 there, with total negative mass at most `~8 *
/// COORDINATE_EPSILON`, so `B` lies within `E = 32 * S * COORDINATE_EPSILON`
/// of the corner range (a generous cover). Hence the accepted root's f64
/// relief-line value lies in `[relief_min - R, relief_max + R]` with
/// `R = 3*tol + A + G * 2^-24 + E`.
///
/// The magnitude bounds, with `S` the chart's largest cell surface spread
/// (`|gu|, |gv| <= S`, `|cross| <= 2S` for every patch), `C` the largest
/// corner magnitude, `T = MAX_ACCEPTED_PARAMETER`, `sx, sy, sr` the absolute
/// line slopes and `Or` the frame-wide `|relief offset|` bound, and using
/// `|u0|, |v0| <= 2` (the clipped interval starts inside the patch slab, so
/// they lie within noise of `[0, 1]`) and `|du|, |dv| <= 2 * sx * T,
/// 2 * sy * T`:
///
/// - `|c0| <= Or + sr*T + C + 12*S =: K0`
/// - `|c1| <= sr*T + 10*S*T*(sx + sy) =: K1`
/// - `|c2| <= 8*S*sx*sy*T^2 =: K2`, `K = max(1, K0, K1, K2)`
/// - `A <= 64 * f64::EPSILON * K`: the assembly performs at most ~20
///   correctly rounded operations on intermediates of magnitude at most
///   `4*K` and the Horner evaluation 3 more, giving `<= 46 * EPSILON * K`;
///   the factor 64 leaves spare that dominates the underestimate of the `K`s
///   by their computed f64 images and the roundings of this expression
///   itself.
/// - `G <= sr + 10*S*(sx + sy) + 16*S*sx*sy*T` (`|dP/dt| = |c1 + 2*c2*s| /
///   span` with the `c1`, `c2` bounds above, whose span factors cancel).
///
/// `R` also absorbs (with orders of magnitude to spare, via the `3*tol >=
/// 3e-10 * C` term against `~2e-16 * C`) the single-rounding gap between the
/// f64 corner values defining `relief_min/relief_max` and their exact
/// rational counterparts, so sweeping the *exact* corner range plus `R` is
/// covered as well. Every term errs toward a larger `R`, i.e. toward
/// enumerating more candidate pixels — never fewer.
fn relief_residual_slack(bounds: &FrameLineBounds, relief: &PreparedRelief) -> f64 {
    let spread = relief.max_cell_spread;
    let corner = relief.max_corner_magnitude;
    let [sx, sy, sr] = bounds.slope_bounds;
    let relief_offset = bounds.offset_bounds[2];
    let t = MAX_ACCEPTED_PARAMETER;
    let k0 = relief_offset + sr * t + corner + 12.0 * spread;
    let k1 = sr * t + 10.0 * spread * t * (sx + sy);
    let k2 = 8.0 * spread * sx * sy * t * t;
    let k = 1.0_f64.max(k0).max(k1).max(k2);
    let tolerance = POLYNOMIAL_EPSILON * k;
    let assembly = 64.0 * f64::EPSILON * k;
    let derivative = sr + 10.0 * spread * (sx + sy) + 16.0 * spread * sx * sy * t;
    let drift = derivative * (0.5 / ROOT_SCALE as f64) * 2.0;
    let extrapolation = 32.0 * spread * COORDINATE_EPSILON;
    3.0 * tolerance + assembly + drift + extrapolation
}

/// One screen axis of the forward splat: converts a foreground cell's swept
/// source box — the cell's unit texel crossed with the cell's own surface
/// band `[relief_min, relief_max]` widened by `relief_residual_slack`, the
/// union of the swept boxes of its four quadrant patches — into a
/// conservative range of pixel indices on this axis.
///
/// The exact screen coordinate of a swept source point `(x, y, relief)` is
/// the affine map `coefficients[0]*x + coefficients[1]*y +
/// coefficients[2]*relief + constant`, so the image of an axis-aligned box is
/// the interval centered on the image of the box center with radius the sum
/// of `|coefficient| * half_extent` — the exact axis-aligned bound of the
/// zonotope image. A pixel `p` can receive a committed fragment from one of
/// the cell's patches only when its exact screen coordinate `p + origin` lies
/// inside the image of that patch's *inflated* swept box (see `render_model`
/// for the inflation argument), which is contained in the image of the
/// cell's inflated swept box, so the integer range `[ceil(low), floor(high)]`,
/// widened by the derived f64-noise `margin`, covers every committing pixel
/// of all four patches. The cell-level range is deliberate: the four patch
/// footprints differ by half a texel per source axis but share the full
/// relief sweep, which dominates them, so enumerating them separately would
/// multiply the candidate set roughly fourfold while the per-patch clip in
/// `cell_intersections` already rejects, exactly, every candidate outside a
/// given patch's own footprint.
#[derive(Clone, Copy, Debug)]
struct SplatAxis {
    coefficients: [f64; 3],
    constant: f64,
    radius_xy: f64,
    relief_pad: f64,
    origin: f64,
    margin: i64,
}

impl SplatAxis {
    /// `screen_row` and `parallax` are this axis's forward warp coefficients,
    /// `origin` is the frame constant added to the integer pixel index
    /// (`sx0`/`sy0`), `deltas` are the [`source_evaluation_error_bounds`],
    /// and `field` supplies the source extents.
    ///
    /// The swept box of a foreground cell — half-extents `1/2` per source
    /// axis, and per cell `spread / 2 + relief_residual_slack` around the
    /// midpoint of the cell's surface range in relief (see
    /// [`relief_residual_slack`] for why every accepted root's relief lies in
    /// that band) — is inflated by `COORDINATE_EPSILON + delta` per source
    /// variable: an accepted root's f64 source coordinates pass its patch's
    /// box check with `COORDINATE_EPSILON` slack — and the patch box is a
    /// sub-box of the cell box — and the exact line point at the same
    /// parameter is within `delta` of the f64 values, so the exact point lies
    /// in the inflated box. The x/y part of the zonotope radius is
    /// frame-constant and precomputed; the relief part is assembled per cell
    /// in [`SplatAxis::pixel_range`] from the cell's own half-extent.
    ///
    /// `margin` bounds the f64 rounding noise of the whole
    /// [`SplatAxis::pixel_range`] chain, in pixels. Every intermediate value
    /// in that chain (center terms, radius terms, the per-cell relief
    /// half-extent assembly, and the sums with the origin) has exact
    /// magnitude at most `magnitude` below, and the chain performs at most
    /// 33 correctly rounded operations/conversions (5 rational-to-f64
    /// conversions, 7 half-extent operations, 5 radius operations, 5 per-cell
    /// relief mid/half operations, and 11 center/radius/interval operations
    /// per endpoint — the cell centers themselves are exact f64 values: an
    /// integer `<= 63` plus a half), each contributing at most
    /// `(f64::EPSILON / 2) * magnitude * (1 + tiny)` of absolute error — at
    /// most `17 * f64::EPSILON * magnitude` in total. The factor `32` leaves
    /// a spare factor of ~1.9, which dominates the underestimate of
    /// `magnitude` by its own f64 evaluation and the roundings of the margin
    /// expression itself. The computed interval endpoint therefore lies
    /// within `eta = 32 * EPSILON * magnitude < 1` of the exact one, and
    /// `ceil(exact) >= ceil(computed - eta) >= ceil(computed) - ceil(eta)`
    /// (from `ceil(a) <= ceil(a - b) + ceil(b)`), so widening the integer
    /// range by `margin = ceil(eta)` covers the exact range — including the
    /// case where the noise crosses an integer boundary, which is exactly
    /// what the `ceil` accounts for. Widening only ever *adds* certainly-safe
    /// candidate pixels (each still runs the full unchanged solve), so every
    /// step of this bound errs in the conservative direction.
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
        let radius_xy = coefficients[0].abs() * (0.5 + COORDINATE_EPSILON + deltas[0])
            + coefficients[1].abs() * (0.5 + COORDINATE_EPSILON + deltas[1]);
        let relief_pad = COORDINATE_EPSILON + deltas[2];
        // The magnitude bound uses the chart-wide relief cap: every per-cell
        // relief midpoint and half-extent is bounded by `maximum_relief`
        // plus the (tiny) residual slack, which `+ 1.0` dominates.
        let magnitude = coefficients[0].abs() * (f64::from(field.width) + 1.0 + deltas[0])
            + coefficients[1].abs() * (f64::from(field.height) + 1.0 + deltas[1])
            + coefficients[2].abs() * (maximum_relief + 1.0 + deltas[2])
            + constant.abs()
            + origin.abs()
            + 1.0;
        let margin = (32.0 * f64::EPSILON * magnitude).ceil() as i64;
        Self {
            coefficients,
            constant,
            radius_xy,
            relief_pad,
            origin,
            margin,
        }
    }

    /// The conservative pixel-index range the inflated swept box of the cell
    /// centered at `(center_x, center_y, relief_mid)`, with relief
    /// half-extent `relief_half` (the cell's surface half-spread plus the
    /// chart's [`relief_residual_slack`]), can touch on this axis, clamped to
    /// `[0, extent)`; `None` when the clamped range is empty.
    fn pixel_range(
        &self,
        center_x: f64,
        center_y: f64,
        relief_mid: f64,
        relief_half: f64,
        extent: u32,
    ) -> Option<(u32, u32)> {
        let center = self.coefficients[0] * center_x
            + self.coefficients[1] * center_y
            + self.coefficients[2] * relief_mid
            + self.constant;
        let radius = self.radius_xy + self.coefficients[2].abs() * (relief_half + self.relief_pad);
        let first = ((center - radius - self.origin).ceil() as i64 - self.margin).max(0);
        let last = ((center + radius - self.origin).floor() as i64 + self.margin)
            .min(i64::from(extent) - 1);
        (first <= last).then_some((first as u32, last as u32))
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
    /// The chart's [`relief_residual_slack`]: added to every cell's surface
    /// half-spread when sweeping its footprint.
    relief_slack: f64,
}

/// Builds the per-chart frame state for one `(model, request)` pair: the
/// frame origin, the fixed-point inverse line factoring, the facing
/// coefficients, and the forward-splat axes. Shared by [`render_model`] and
/// the exhaustive per-pixel test oracle so both traversals feed identical
/// inputs into the identical per-patch solve — the only difference between
/// them is which (patch, pixel) pairs they visit.
fn prepare_frame_charts<'a>(
    prepared: &'a PreparedModel,
    request: &RenderRequest,
) -> Vec<FrameChart<'a>> {
    let bounds = prepared.charts.bounds();
    let Some(TargetExtents {
        min_x,
        max_x,
        min_y,
        max_y,
    }) = projected_extents(bounds, &prepared.charts, &request.target)
    else {
        return Vec::new();
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
    prepared
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
            let line_bounds =
                source_evaluation_error_bounds(&inverse, request.width, request.height);
            let relief_slack = relief_residual_slack(&line_bounds, &entry.relief);
            let screen = warp.screen();
            let parallax = warp.parallax();
            let splat = [
                SplatAxis::new(
                    screen[0],
                    parallax[0],
                    sx0,
                    line_bounds.deltas,
                    entry.maximum_relief,
                    &entry.relief,
                ),
                SplatAxis::new(
                    screen[1],
                    parallax[1],
                    sy0,
                    line_bounds.deltas,
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
                relief_slack,
            })
        })
        .collect()
}

pub fn render_model(
    prepared: &PreparedModel,
    request: &RenderRequest,
) -> Result<FrameBuffer, RenderError> {
    (request.width as usize)
        .checked_mul(request.height as usize)
        .ok_or(RenderError::FrameBufferTooLarge)?;
    let mut frame = FrameBuffer::transparent(request.width, request.height);
    if request.width == 0 || request.height == 0 {
        return Ok(frame);
    }
    let frame_charts = prepare_frame_charts(prepared, request);

    // Per-patch forward splat: for each chart, for each foreground cell,
    // enumerate the pixels of the cell's conservative swept-box screen
    // footprint — the union of its four quadrant patches' footprints (see
    // `SplatAxis` for why the union is enumerated once) — and intersect each
    // pixel's inverse line against each of the four patches individually
    // (see `cell_intersections` for the per-patch semantics).
    //
    // Coverage: a fragment is committed for (pixel, patch) only when a root
    // passes `cell_intersections`' acceptance box check, which pins the
    // root's f64 source coordinates inside the patch's box inflated by
    // `COORDINATE_EPSILON` and its f64 relief inside `[0, maximum_relief]`
    // inflated likewise, at a parameter `|t| <= MAX_ACCEPTED_PARAMETER`. The
    // exact line point at that parameter is within the
    // `source_evaluation_error_bounds` of those f64 values, and the exact
    // line point maps forward to exactly the pixel's screen coordinate (the
    // inverse line is the exact rational solution set of the warp equations).
    // The pixel's screen coordinate therefore lies inside the zonotope bbox
    // of the patch's inflated swept box, which is contained in its cell's,
    // which the `SplatAxis` ranges cover conservatively — so a pixel outside
    // the cell's footprint provably commits nothing for any of the cell's
    // patches, and enumerating the footprint visits every pixel the cell's
    // patches can color (asserted against the exhaustive per-pixel oracle in
    // the tests).
    //
    // Order independence: `commit_fragment` keeps the unique minimum
    // `FragmentKey` under an exact total order, so the frame is the pointwise
    // minimum over the accepted-fragment multiset — independent of the
    // cell-major visit order, and of a patch pair accepting the same
    // boundary intersection twice (see `cell_intersections`' attribution
    // note). Equal keys imply the same chart (ranks are unique per resolved
    // chart) and the same source texel, hence the same color.
    let mut scratch = RenderScratch::default();
    for pass in &frame_charts {
        // Pass A: collect one splat job per foreground cell (its footprint
        // rectangle and texel color) and the union rectangle of all of them.
        scratch.jobs.clear();
        let mut union_rect: Option<[u32; 4]> = None;
        for cell_y in 0..pass.relief.height {
            for cell_x in 0..pass.relief.width {
                if !pass.relief.is_foreground(cell_x, cell_y) {
                    continue;
                }
                let Some(DecodedTexel::Relief { rgb, .. }) = pass.chart.texel_at(cell_x, cell_y)
                else {
                    unreachable!(
                        "foreground cells decode to relief texels (ComponentMap::label only \
                         labels relief texels)"
                    );
                };
                // Cell centers are exact in f64: an integer `<= 63` plus a
                // half.
                let center_x = f64::from(cell_x) + 0.5;
                let center_y = f64::from(cell_y) + 0.5;
                // The cell's footprint sweeps its own surface band — the
                // cell's corner range plus the derived residual slack — not
                // the chart's whole depth prism (see `relief_residual_slack`).
                let (relief_min, relief_max) = pass.relief.relief_bounds(cell_x, cell_y);
                let relief_mid = 0.5 * (relief_min + relief_max);
                let relief_half = 0.5 * (relief_max - relief_min) + pass.relief_slack;
                let Some((first_x, last_x)) = pass.splat[0].pixel_range(
                    center_x,
                    center_y,
                    relief_mid,
                    relief_half,
                    request.width,
                ) else {
                    continue;
                };
                let Some((first_y, last_y)) = pass.splat[1].pixel_range(
                    center_x,
                    center_y,
                    relief_mid,
                    relief_half,
                    request.height,
                ) else {
                    continue;
                };
                union_rect = Some(match union_rect {
                    None => [first_x, last_x, first_y, last_y],
                    Some([ux0, ux1, uy0, uy1]) => [
                        ux0.min(first_x),
                        ux1.max(last_x),
                        uy0.min(first_y),
                        uy1.max(last_y),
                    ],
                });
                scratch.jobs.push(CellJob {
                    cell: Cell {
                        x: cell_x,
                        y: cell_y,
                        rgb,
                    },
                    first_x,
                    last_x,
                    first_y,
                    last_y,
                });
            }
        }
        let Some([rect_x0, rect_x1, rect_y0, rect_y1]) = union_rect else {
            continue;
        };

        // Fetch every covered pixel's inverse-line coefficients exactly once
        // per (pixel, chart): the identical `variable_f64` values the solve
        // would fetch itself, hoisted because each of the many cell
        // footprints covering a pixel would otherwise refetch them.
        let rect_width = (rect_x1 - rect_x0 + 1) as usize;
        scratch.lines.clear();
        for y in rect_y0..=rect_y1 {
            for x in rect_x0..=rect_x1 {
                scratch.lines.push(pass.inverse.variable_f64(x, y));
            }
        }

        // Pass B: splat every job's four patches over its footprint pixels.
        for job in &scratch.jobs {
            for y in job.first_y..=job.last_y {
                let row = (y - rect_y0) as usize * rect_width;
                for x in job.first_x..=job.last_x {
                    let coefficients = &scratch.lines[row + (x - rect_x0) as usize];
                    splat_cell_pixel(&mut frame, pass, coefficients, job.cell, x, y);
                }
            }
        }
    }

    Ok(frame)
}

/// One foreground cell scheduled for splatting, with its conservative
/// footprint pixel rectangle (`first_x..=last_x` x `first_y..=last_y`,
/// already clamped to the frame).
struct CellJob {
    cell: Cell,
    first_x: u32,
    last_x: u32,
    first_y: u32,
    last_y: u32,
}

/// Scratch reused across the charts of one `render_model` call. `jobs` holds
/// the current chart's cell jobs; `lines` holds, for every pixel of the
/// union rectangle of the jobs' footprints, the pixel's inverse-line f64
/// coefficients (`FrameInverse::variable_f64`), row-major from the
/// rectangle's origin. Both are unbounded (they scale with chart and frame
/// size), so they stay heap `Vec`s — allocated once and `.clear()`ed per
/// chart, their backing storage stops reallocating at steady state.
#[derive(Default)]
struct RenderScratch {
    jobs: Vec<CellJob>,
    lines: Vec<[[f64; 2]; 3]>,
}

/// One foreground cell being splatted: its source coordinates and texel
/// color. Its four quadrant patches are solved individually by
/// [`cell_intersections`].
#[derive(Clone, Copy)]
struct Cell {
    x: u32,
    y: u32,
    rgb: [u8; 3],
}

/// Intersects pixel `(x, y)`'s inverse line — whose f64 coefficients the
/// caller fetched once per (pixel, chart) via `FrameInverse::variable_f64` —
/// with each of the four quadrant patches of one foreground cell and commits
/// every accepted intersection through the exact depth test.
fn splat_cell_pixel(
    frame: &mut FrameBuffer,
    pass: &FrameChart,
    coefficients: &[[f64; 2]; 3],
    cell: Cell,
    x: u32,
    y: u32,
) {
    for parameter in cell_intersections(
        pass.relief,
        coefficients,
        pass.facing,
        pass.maximum_relief,
        cell.x,
        cell.y,
    ) {
        commit_fragment(
            frame,
            x,
            y,
            FragmentKey {
                depth: pass
                    .inverse
                    .depth_at(x, y, Ratio::new(parameter, ROOT_SCALE)),
                chart_rank: pass.chart.view().rank(),
                source_y: cell.y,
                source_x: cell.x,
            },
            cell.rgb,
        );
    }
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

/// Intersects one pixel's inverse line with the four quadrant patches of a
/// single foreground cell, returning the quantized ray parameters of every
/// accepted, camera-facing intersection. Capacity 12: each of the 4 patches
/// contributes at most the 3 candidates [`roots_in_unit_interval`] can return
/// (bound documented there).
///
/// # Per-patch semantics
///
/// - **Clipping.** Each patch's ray-parameter interval is the intersection of
///   the parameter intervals in which the line's source x and y lie in the
///   patch's half-texel slabs ([`half_slab_ranges`], computed once per cell —
///   adjacent patches share boundary planes) and its relief lies in the
///   chart's full range `[0, maximum_relief]` (computed once, shared by all
///   four patches: the relief planes bound the whole chart). A patch interval
///   without interior (`end - start <= 0`) means the line meets the patch's
///   closed box in at most a single parameter: it grazes the box boundary
///   without passing through the patch, so there is no interval to solve
///   over. A genuine surface intersection lying exactly on a shared box face
///   also lies in the neighboring patch's interval, which has interior there;
///   only a tangential contact at the chart's outer silhouette corner is
///   dropped — a measure-zero grazing contact with an empty solve interval,
///   which cannot own an interior fragment.
/// - **Acceptance.** A root is accepted when its f64 line coordinates lie in
///   its patch's box inflated by `COORDINATE_EPSILON` per variable — an
///   f64-noise tolerance scoped to the patch. Rejecting on the exact box would drop genuine
///   boundary intersections whose computed image lands a few ulps outside;
///   the inflation errs toward acceptance, and the forward-splat footprint is
///   inflated to match (see [`SplatAxis`]).
/// - **Attribution.** Patch domains are *closed*, so an intersection exactly
///   on a shared patch edge is recovered by both neighbors — deliberately.
///   Within a chart component the piecewise-bilinear surface is continuous
///   across patch edges (adjacent patches sample identical corner values at
///   shared half-texel grid points: the hat-kernel closure sample depends
///   only on the point and the component), so both patches recover the same
///   geometric point, each attributing it to its own source texel. Both
///   fragments commit, and `commit_fragment` keeps the unique minimum
///   `FragmentKey` under an exact total order — the frame is a well-defined
///   pointwise minimum over the accepted-fragment multiset, so duplicate
///   acceptance can never change it non-deterministically, and within-cell
///   duplicates (same texel) cannot change its color at all. Half-open patch
///   domains (`[lo, hi)` on interior edges) were rejected from first
///   principles: the two neighbors reconstruct a shared-edge root through
///   *different* quadratics, so each would test its own noise-perturbed f64
///   coordinate against a strict boundary, and both tests can fail (one
///   computes `hi + ulp`, the other `lo - ulp`), losing the intersection
///   entirely and turning patch seams into pinholes. Closed domains can only
///   duplicate, never lose, and the exact minimum absorbs duplicates by
///   construction — no cross-patch epsilon coordination exists or is needed.
fn cell_intersections(
    field: &PreparedRelief,
    coefficients: &[[f64; 2]; 3],
    facing: FacingCoefficients,
    maximum_relief: f64,
    cell_x: u32,
    cell_y: u32,
) -> Bounded<i64, 12> {
    let mut intersections = Bounded::new();
    let [
        [x_offset, x_slope],
        [y_offset, y_slope],
        [relief_offset, relief_slope],
    ] = *coefficients;
    // Exact in f64: integers `<= 63`.
    let cell_left = f64::from(cell_x);
    let cell_top = f64::from(cell_y);

    // The relief planes bound the whole chart, so their parameter interval is
    // shared by all four patches. A (near-)constant relief line (slope below
    // `COORDINATE_EPSILON`: over `|t| <= MAX_ACCEPTED_PARAMETER` such a
    // coordinate varies below the noise tolerance) constrains no parameter
    // — it either lies in the
    // inflated range (no constraint) or misses the chart entirely.
    let relief_range = if relief_slope.abs() <= COORDINATE_EPSILON {
        if relief_offset < -COORDINATE_EPSILON
            || relief_offset > maximum_relief + COORDINATE_EPSILON
        {
            return intersections;
        }
        [f64::NEG_INFINITY, f64::INFINITY]
    } else {
        let first = (0.0 - relief_offset) / relief_slope;
        let second = (maximum_relief - relief_offset) / relief_slope;
        [first.min(second), first.max(second)]
    };
    let x_slabs = half_slab_ranges(x_offset, x_slope, cell_left);
    let y_slabs = half_slab_ranges(y_offset, y_slope, cell_top);

    for quadrant in 0..4 {
        let right = quadrant % 2 == 1;
        let bottom = quadrant / 2 == 1;
        let Some(x_range) = x_slabs[usize::from(right)] else {
            continue;
        };
        let Some(y_range) = y_slabs[usize::from(bottom)] else {
            continue;
        };
        let start = x_range[0].max(y_range[0]).max(relief_range[0]);
        let end = x_range[1].min(y_range[1]).min(relief_range[1]);
        // The free source variable has offset exactly `0.0` and slope exactly
        // `1.0` (`PreparedInverse`), so at least one of the three axes above
        // produced finite bounds; a non-finite endpoint would mean that
        // invariant is broken and must be loud.
        assert!(
            start.is_finite() && end.is_finite(),
            "patch-clipped parameter range [{start}, {end}] is not finite: the free-variable \
             clip invariant does not hold"
        );
        let span = end - start;
        if span <= 0.0 {
            continue;
        }
        let x_lo = cell_left + if right { 0.5 } else { 0.0 };
        let y_lo = cell_top + if bottom { 0.5 } else { 0.0 };
        let x_hi = x_lo + 0.5;
        let y_hi = y_lo + 0.5;

        // Residual of the ray against the patch surface in the clipped
        // interval's unit variable `s` (`parameter = start + span * s`):
        // `g(s) = relief_line(s) - bilinear(u(s), v(s))`. The bilinear
        // expands to `c00 + gu*u + gv*v + cross*u*v` and `u`, `v` are affine
        // in `s` (the quadrant's unit coordinates are the source coordinates
        // scaled by 2 and shifted by the quadrant half), so `g` is quadratic
        // in `s` with the closed-form coefficients assembled below.
        let corners = field.patch_corners(cell_x, cell_y, quadrant);
        let u0 = 2.0 * (x_offset + x_slope * start - cell_left) - if right { 1.0 } else { 0.0 };
        let v0 = 2.0 * (y_offset + y_slope * start - cell_top) - if bottom { 1.0 } else { 0.0 };
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
            let parameter = start + span * unit;
            let x = x_offset + x_slope * parameter;
            let y = y_offset + y_slope * parameter;
            let relief = relief_offset + relief_slope * parameter;
            if x < x_lo - COORDINATE_EPSILON
                || x > x_hi + COORDINATE_EPSILON
                || y < y_lo - COORDINATE_EPSILON
                || y > y_hi + COORDINATE_EPSILON
                || relief < -COORDINATE_EPSILON
                || relief > maximum_relief + COORDINATE_EPSILON
            {
                continue;
            }
            if !branch_faces_camera(field, cell_x, cell_y, quadrant, facing, x, y) {
                continue;
            }
            intersections.push(quantized_parameter(parameter));
        }
    }
    intersections
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

/// One source axis's contribution to the per-patch parameter clips of a
/// single (cell, pixel): the closed parameter intervals in which the line's
/// coordinate on this axis lies inside the axis's two half-texel slabs
/// (`[lo, lo + 1/2]` and `[lo + 1/2, lo + 1]`, all bounds exact in f64:
/// integers `<= 63` plus dyadic halves). `None` means the slab is missed
/// entirely: a (near-)constant coordinate outside it, under the
/// `COORDINATE_EPSILON` slope rule and inflated-bounds test.
/// A constant coordinate inside a slab constrains no
/// parameter, yielding the unbounded interval.
///
/// The three boundary-plane parameters are computed once and both slab
/// intervals assembled from them. Adjacent quadrants share their middle
/// boundary plane, whose parameter is the same division either way, so this
/// is common-subexpression elimination across the per-patch clips — not a
/// change of semantics: each slab's interval is exactly `[min, max]` of its
/// own two boundary parameters.
fn half_slab_ranges(offset: f64, slope: f64, lo: f64) -> [Option<[f64; 2]>; 2] {
    if slope.abs() <= COORDINATE_EPSILON {
        let inside = |low: f64, high: f64| {
            offset >= low - COORDINATE_EPSILON && offset <= high + COORDINATE_EPSILON
        };
        [
            inside(lo, lo + 0.5).then_some([f64::NEG_INFINITY, f64::INFINITY]),
            inside(lo + 0.5, lo + 1.0).then_some([f64::NEG_INFINITY, f64::INFINITY]),
        ]
    } else {
        let first = (lo - offset) / slope;
        let middle = (lo + 0.5 - offset) / slope;
        let last = (lo + 1.0 - offset) / slope;
        [
            Some([first.min(middle), first.max(middle)]),
            Some([middle.min(last), middle.max(last)]),
        ]
    }
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

#[cfg(test)]
mod tests {
    use num_rational::Ratio;
    use relief_core::{
        AuthoredModel, Bounds, CanonicalView, Chart, DecodedTexel, ReliefField, ResolvedCharts,
        SourcePoint, WarpCoefficients,
    };

    use crate::{
        CameraBasis, FrameBuffer, PreparedModel, RenderRequest, TargetView,
        presets::{FacingCoefficients, TargetExtents},
    };

    use super::{
        Bounded, Cell, MAX_PARAMETER_SPAN, MAX_QUANTUM_PROBES, PreparedRelief, ROOT_SCALE,
        SplatAxis, cell_intersections, correctly_rounded_root, prepare_frame_charts,
        projected_extents, quantized_parameter, ratio_to_f64, relief_residual_slack, render_model,
        roots_in_unit_interval, source_evaluation_error_bounds, splat_cell_pixel,
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

    /// The distinct geometric intersections of one pixel's line against every
    /// foreground patch of `field` — the per-pixel union the per-patch
    /// traversal commits from. Distinctness is exact (integer quanta and
    /// integer source coordinates): adjacent patches recovering the same
    /// shared-edge intersection produce identical `(source_x, parameter)`
    /// pairs here (the surface is continuous across patch edges within a
    /// component), and collapsing them yields the geometric hit set the
    /// assertions speak about.
    fn distinct_intersections(
        field: &PreparedRelief,
        coefficients: &[[f64; 2]; 3],
        facing: FacingCoefficients,
        maximum_relief: f64,
    ) -> Vec<(u32, i64)> {
        let mut hits = Vec::new();
        for cell_y in 0..field.height {
            for cell_x in 0..field.width {
                if !field.is_foreground(cell_x, cell_y) {
                    continue;
                }
                for parameter in
                    cell_intersections(field, coefficients, facing, maximum_relief, cell_x, cell_y)
                {
                    hits.push((cell_x, parameter));
                }
            }
        }
        hits.sort_unstable();
        hits.dedup();
        hits
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
        let locations: Vec<_> = distinct_intersections(&prepared, &coefficients, facing, 16.0)
            .into_iter()
            .map(|(source_x, parameter)| {
                (
                    source_x,
                    (ratio_to_f64(relief_at(&variables, parameter)) * 1_000.0).round() / 1_000.0,
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

        let locations: Vec<_> = distinct_intersections(&prepared, &coefficients, facing, 16.0)
            .into_iter()
            .map(|(source_x, parameter)| {
                (
                    source_x,
                    (ratio_to_f64(relief_at(&variables, parameter)) * 1_000.0).round() / 1_000.0,
                )
            })
            .collect();

        assert_eq!(locations, vec![(1, 4.0), (2, 16.0)]);
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

    /// Exhaustive per-pixel reference for the per-patch semantics: for every
    /// pixel, iterate ALL foreground patches of ALL charts — no footprint
    /// culling — and apply the identical per-patch solve
    /// ([`splat_cell_pixel`], shared with `render_model`). Equality against
    /// `render_model` proves the load-bearing property of the forward splat:
    /// footprint enumeration visits every (patch, pixel) pair that commits.
    /// The traversal order also differs (pixel-major here, patch-major in
    /// `render_model`), so equality re-verifies commit-order independence.
    fn render_patch_reference(prepared: &PreparedModel, request: &RenderRequest) -> FrameBuffer {
        let mut frame = FrameBuffer::transparent(request.width, request.height);
        let frame_charts = prepare_frame_charts(prepared, request);
        for y in 0..request.height {
            for x in 0..request.width {
                for pass in &frame_charts {
                    let coefficients = pass.inverse.variable_f64(x, y);
                    for cell_y in 0..pass.relief.height {
                        for cell_x in 0..pass.relief.width {
                            if !pass.relief.is_foreground(cell_x, cell_y) {
                                continue;
                            }
                            let Some(DecodedTexel::Relief { rgb, .. }) =
                                pass.chart.texel_at(cell_x, cell_y)
                            else {
                                unreachable!("foreground cells decode to relief texels");
                            };
                            let cell = Cell {
                                x: cell_x,
                                y: cell_y,
                                rgb,
                            };
                            splat_cell_pixel(&mut frame, pass, &coefficients, cell, x, y);
                        }
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
    fn per_patch_splat_matches_the_exhaustive_patch_scan() {
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
                let reference = render_patch_reference(&prepared, &request);
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
    /// image of every foreground cell's swept surface band — the cell's unit
    /// texel crossed with the *exact rational* range of its surface samples:
    /// for each cell and screen axis, every integer pixel index whose exact
    /// screen coordinate lies inside the exact zonotope bbox (computed from
    /// the eight exact corner images via `WarpCoefficients::apply`) must be
    /// inside the range `render_model` enumerates candidates from.
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
            let line_bounds = source_evaluation_error_bounds(&inverse, width, height);
            let relief_slack = relief_residual_slack(&line_bounds, &entry.relief);
            let screen = warp.screen();
            let parallax = warp.parallax();
            let axes = [
                SplatAxis::new(
                    screen[0],
                    parallax[0],
                    sx0,
                    line_bounds.deltas,
                    entry.maximum_relief,
                    &entry.relief,
                ),
                SplatAxis::new(
                    screen[1],
                    parallax[1],
                    sy0,
                    line_bounds.deltas,
                    entry.maximum_relief,
                    &entry.relief,
                ),
            ];
            let origins = [sx0, sy0];
            let extents = [width, height];
            let relief_field = ReliefField::new(chart);

            for source_y in 0..entry.relief.height {
                for source_x in 0..entry.relief.width {
                    if !entry.relief.is_foreground(source_x, source_y) {
                        continue;
                    }
                    // The exact rational surface range of the cell: min/max
                    // over the nine half-texel closure samples (the corner
                    // set of all four quadrant patches), recomputed here
                    // independently of the prepared f64 corners.
                    let foreground = relief_field
                        .foreground_cell(source_x, source_y)
                        .expect("prepared foreground cell must exist in the field");
                    let mut exact_corners = Vec::new();
                    for half_x in 0..=2_i64 {
                        for half_y in 0..=2_i64 {
                            let point = SourcePoint::new(
                                Ratio::new(2 * i64::from(source_x) + half_x, 2),
                                Ratio::new(2 * i64::from(source_y) + half_y, 2),
                            );
                            let (weighted, total) = foreground
                                .sample_terms_closure(point)
                                .expect("cell closure sample must exist");
                            exact_corners.push(weighted / total);
                        }
                    }
                    let exact_min = *exact_corners.iter().min().unwrap();
                    let exact_max = *exact_corners.iter().max().unwrap();

                    let mut corners = Vec::new();
                    for corner_x in [i64::from(source_x), i64::from(source_x) + 1] {
                        for corner_y in [i64::from(source_y), i64::from(source_y) + 1] {
                            for relief in [exact_min, exact_max] {
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
                    let center_x = f64::from(source_x) + 0.5;
                    let center_y = f64::from(source_y) + 0.5;
                    let (relief_min, relief_max) = entry.relief.relief_bounds(source_x, source_y);
                    let relief_mid = 0.5 * (relief_min + relief_max);
                    let relief_half = 0.5 * (relief_max - relief_min) + relief_slack;
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
                            .pixel_range(center_x, center_y, relief_mid, relief_half, *extent)
                            .expect("in-frame swept box must yield a pixel range");
                        assert!(
                            i64::from(first) <= exact_first.max(0),
                            "cell ({source_x}, {source_y}): range start {first} misses exact \
                             {exact_first}"
                        );
                        assert!(
                            i64::from(last) >= exact_last.min(i64::from(*extent) - 1),
                            "cell ({source_x}, {source_y}): range end {last} misses exact \
                             {exact_last}"
                        );
                    }
                }
            }
        }
    }
}
