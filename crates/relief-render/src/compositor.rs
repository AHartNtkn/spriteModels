use num_rational::Ratio;
use relief_core::{Bounds, DecodedTexel, ReliefField, ResolvedCharts, SourcePoint};
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

/// Half of one ray-parameter quantum: `0.5 / ROOT_SCALE`. `ROOT_SCALE = 2^24`,
/// so this is exactly `2^-25`. Two parameters closer together than this
/// straddle at most one rounding boundary `(q + 0.5) / ROOT_SCALE`, so their
/// nearest quanta differ by at most one — the fact the correctly-rounded root
/// solve relies on to terminate. This is a *parameter-space* quantity: the
/// downstream quantization ([`quantized_parameter`]) rounds the ray parameter,
/// not the cubic's per-segment unit variable, and the two differ by the
/// segment span.
const HALF_QUANTUM: f64 = 0.5 / ROOT_SCALE as f64;

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

/// Worst-case count of safeguarded bracket-shrinking steps in
/// [`correctly_rounded_root`]. Each step at least halves the bracket (a
/// bisection substep is unconditional; the Newton substep only tightens
/// further). The initial bracket, measured in parameter space where the
/// convergence criterion lives, has width at most `span <= MAX_PARAMETER_SPAN
/// = 252 < 2^8`, and the loop stops once that width is at most `HALF_QUANTUM =
/// 2^-25`. Halving `2^8` down to `2^-25` takes `8 + 25 = 33` steps, and 32 do
/// not suffice (`252 / 2^32 > 2^-25`), so 33 is derived, not chosen:
/// `MAX_PARAMETER_SPAN / 2^33 <= HALF_QUANTUM < MAX_PARAMETER_SPAN / 2^32`.
const MAX_SAFEGUARDED_STEPS: u32 = 33;

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

    fn extend(&mut self, values: impl Iterator<Item = T>) {
        for value in values {
            self.push(value);
        }
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

#[derive(Clone, Copy, Debug)]
struct ReliefTerms {
    weighted: f64,
    total: f64,
}

#[derive(Clone, Copy, Debug)]
struct ReliefPatch {
    corners: [ReliefTerms; 4],
}

#[derive(Clone, Copy, Debug)]
struct ReliefSample {
    terms: ReliefTerms,
    relief_x: f64,
    relief_y: f64,
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
                            ReliefTerms {
                                weighted: ratio_to_f64(weighted),
                                total: ratio_to_f64(total),
                            }
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

    fn sample_patch(
        &self,
        source_x: u32,
        source_y: u32,
        quadrant: usize,
        x: f64,
        y: f64,
    ) -> ReliefSample {
        let patches = self.cells[(source_y * self.width + source_x) as usize]
            .expect("only foreground cells reach analytic evaluation");
        let local_x = (x - f64::from(source_x)).clamp(0.0, 1.0);
        let local_y = (y - f64::from(source_y)).clamp(0.0, 1.0);
        let right = quadrant % 2 == 1;
        let bottom = quadrant / 2 == 1;
        let patch = patches[quadrant];
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
        let interpolate = |values: [f64; 4]| {
            values[0] * (1.0 - u) * (1.0 - v)
                + values[1] * u * (1.0 - v)
                + values[2] * (1.0 - u) * v
                + values[3] * u * v
        };
        let derivative_u =
            |values: [f64; 4]| (values[1] - values[0]) * (1.0 - v) + (values[3] - values[2]) * v;
        let derivative_v =
            |values: [f64; 4]| (values[2] - values[0]) * (1.0 - u) + (values[3] - values[1]) * u;
        let weighted_values = patch.corners.map(|terms| terms.weighted);
        let total_values = patch.corners.map(|terms| terms.total);
        let weighted = interpolate(weighted_values);
        let total = interpolate(total_values);
        let weighted_x = 2.0 * derivative_u(weighted_values);
        let weighted_y = 2.0 * derivative_v(weighted_values);
        let total_x = 2.0 * derivative_u(total_values);
        let total_y = 2.0 * derivative_v(total_values);
        let denominator = total * total;
        ReliefSample {
            terms: ReliefTerms { weighted, total },
            relief_x: (weighted_x * total - weighted * total_x) / denominator,
            relief_y: (weighted_y * total - weighted * total_y) / denominator,
        }
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
                    let values = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0].map(|unit| {
                        let parameter = start + (end - start) * unit;
                        let x = x_offset + x_slope * parameter;
                        let y = y_offset + y_slope * parameter;
                        let relief = relief_offset + relief_slope * parameter;
                        let sample = field.sample_patch(source_x, source_y, quadrant, x, y);
                        relief * sample.terms.total - sample.terms.weighted
                    });
                    let polynomial = interpolate_unit_cubic(values);

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
        let sample = field.sample_patch(source_x, source_y, quadrant, x, y);
        facing.evaluate(sample.relief_x, sample.relief_y)
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

fn interpolate_unit_cubic(values: [f64; 4]) -> [f64; 4] {
    const NODES: [f64; 4] = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];
    let mut result = [0.0; 4];
    for index in 0..4 {
        let mut basis = [0.0; 4];
        basis[0] = 1.0;
        let mut degree = 0;
        let mut denominator = 1.0;
        for (other, &node) in NODES.iter().enumerate() {
            if other == index {
                continue;
            }
            denominator *= NODES[index] - node;
            for coefficient in (0..=degree).rev() {
                basis[coefficient + 1] += basis[coefficient];
                basis[coefficient] *= -node;
            }
            degree += 1;
        }
        for coefficient in 0..4 {
            result[coefficient] += values[index] * basis[coefficient] / denominator;
        }
    }
    result
}

/// Finds the cubic's roots in its unit variable `[0, 1]`. The segment
/// `[start, end]` — the ray-parameter interval the unit variable is an affine
/// reparameterization of (`parameter = start + (end - start) * unit`) — is
/// passed through to [`correctly_rounded_root`] so each sign-change root is
/// returned as the unit preimage of its provably correctly rounded *parameter*
/// quantum. Direct tolerance hits at partition points are returned as the raw
/// partition values, exactly as before.
///
/// Bound on `partitions`: it holds the interval endpoints `{0, 1}` plus the
/// roots of the polynomial's derivative that fall strictly inside `(0, 1)`
/// — the polynomial's critical points, which split `[0, 1]` into monotonic
/// pieces. A cubic's derivative is quadratic, which has at most 2 real
/// roots (see `quadratic_roots`), so `partitions` holds at most 2 + 2 = 4
/// values.
///
/// Bound on the returned roots: consider the `partitions.len() - 1` unit
/// intervals `(partitions[i], partitions[i+1])` in order. A bisected root
/// is only pushed for interval `i` when its left endpoint `partitions[i]`
/// independently failed the direct zero-hit test (`left_value.abs() >
/// tolerance` is required to proceed) — so charge that push to
/// `partitions[i]`. A direct zero-hit push is charged to the point itself.
/// Every push is thus charged to a distinct partition point (a point is
/// charged at most once: either it is a direct hit, or — having failed
/// that test — it anchors at most one bisected push for the interval
/// starting there), so the number of entries pushed before the final
/// `dedup_by` can never exceed `partitions.len()`, i.e. at most 4. This is
/// a tighter, algorithm-specific bound than "a cubic has at most 3 roots":
/// that fact constrains the polynomial's true zero set, not the number of
/// *candidate* entries this tolerance-based procedure can push before
/// `dedup_by` collapses coincident candidates.
fn roots_in_unit_interval(mut polynomial: [f64; 4], start: f64, end: f64) -> Bounded<f64, 4> {
    let scale = polynomial
        .iter()
        .fold(1.0_f64, |largest, value| largest.max(value.abs()));
    let tolerance = POLYNOMIAL_EPSILON * scale;
    let mut degree = 3;
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

    let mut partitions = Bounded::<f64, 4>::new();
    partitions.push(0.0);
    partitions.push(1.0);
    match degree {
        2 => {
            let critical = -polynomial[1] / (2.0 * polynomial[2]);
            if critical > 0.0 && critical < 1.0 {
                partitions.push(critical);
            }
        }
        3 => {
            partitions.extend(
                quadratic_roots(polynomial[1], 2.0 * polynomial[2], 3.0 * polynomial[3])
                    .into_iter()
                    .filter(|root| *root > 0.0 && *root < 1.0),
            );
        }
        _ => {}
    }
    partitions.sort_by(f64::total_cmp);
    partitions.dedup_by(|left, right| (*left - *right).abs() <= COORDINATE_EPSILON);

    let evaluate = |value: f64| {
        polynomial[0] + value * (polynomial[1] + value * (polynomial[2] + value * polynomial[3]))
    };
    let mut roots = Bounded::<f64, 4>::new();
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
/// The cubic lives in the segment's unit variable, but the quantity the
/// pipeline stores and renders is the ray parameter
/// `t = start + (end - start) * unit`, quantized to `q = round(t * ROOT_SCALE)`
/// by [`quantized_parameter`]. This solver therefore converges the bracket
/// until its *image in parameter space* fits within a half-quantum window and
/// decides the quantum by a polynomial sign test at the parameter-quantum
/// boundary mapped back into the unit variable, so the returned quantum
/// provably contains the root's parameter.
///
/// Returns `(unit, steps)` where `unit` is the unit-variable preimage
/// `(q / ROOT_SCALE - start) / span` of the proven quantum `q` — chosen so the
/// caller's parameter reconstruction followed by [`quantized_parameter`]
/// recovers exactly `q` (asserted below) — and `steps` is the number of
/// safeguarded shrinking steps taken (exposed only so the tests can certify
/// the [`MAX_SAFEGUARDED_STEPS`] bound).
///
/// Preconditions the caller establishes: `fa = polynomial(a)` and `fb =
/// polynomial(b)` carry strictly opposite signs (a unique root lies in
/// `(a, b)`), and `[a, b]` is a partition interval between consecutive critical
/// points, so `polynomial` is strictly monotone on `[a, b]`. Both are relied on
/// below.
///
/// This replaces the previous "bisect a fixed 56 times, then round the
/// midpoint" step. That produced the correct quantum almost always but never
/// proved it. Here the returned `q` is *proven* (relative to the same f64
/// Horner evaluation the rest of the solve uses) to be the correct rounding of
/// the root's parameter, by the sign test at the quantum boundary — see the
/// comments at the return.
fn correctly_rounded_root(
    polynomial: &[f64; 4],
    a: f64,
    b: f64,
    fa: f64,
    fb: f64,
    start: f64,
    end: f64,
) -> (f64, u32) {
    // `span` must match the caller's `end - start` bit-for-bit so the
    // round-trip assert below exercises the identical expression; the upper
    // bound is the clip-derived fact MAX_SAFEGUARDED_STEPS rests on.
    let span = end - start;
    assert!(
        span > 0.0 && span <= MAX_PARAMETER_SPAN,
        "segment span {span} outside (0, {MAX_PARAMETER_SPAN}]: the clipped parameter-range \
         bound behind MAX_SAFEGUARDED_STEPS does not hold"
    );
    debug_assert!(
        fa.signum() != fb.signum(),
        "correctly_rounded_root requires a strict sign change on [a, b]"
    );
    let evaluate =
        |x: f64| polynomial[0] + x * (polynomial[1] + x * (polynomial[2] + x * polynomial[3]));
    // Derivative of the cubic `c0 + c1 x + c2 x^2 + c3 x^3`.
    let derivative = |x: f64| polynomial[1] + x * (2.0 * polynomial[2] + x * (3.0 * polynomial[3]));
    let scale = ROOT_SCALE as f64;
    // The caller's exact parameter expression for a unit value.
    let parameter_of = |unit: f64| start + span * unit;
    // Finishes with a proven quantum `q` (an integer-valued f64): the returned
    // unit value is `q`'s unit preimage. The assert certifies the caller's
    // reconstruction recovers `q`: the round trip perturbs `t` from the exact
    // `q / 2^24` by a few ulps of magnitude ~max(|start|, span, |t|) <= ~2^8,
    // i.e. by ~2^-45 — about 2^-21 of a quantum — so `round(t * 2^24)` cannot
    // move off the integer `q`. A violation would mean the f64 domain
    // assumptions are broken, and must be loud, not a silently shifted root.
    let finish = |q: f64, steps: u32| {
        let unit = (q / scale - start) / span;
        assert!(
            quantized_parameter(parameter_of(unit)) == q as i64,
            "proven quantum {q} does not survive the caller's parameter reconstruction"
        );
        (unit, steps)
    };

    let mut lo = a;
    let mut hi = b;
    // Only the sign of `f(lo)` is needed to steer the bracket; `f(hi)` always
    // holds the opposite sign by the invariant, so it need not be tracked.
    let mut f_lo = fa;
    let mut steps = 0u32;
    // Converge until the bracket's parameter-space image fits within a
    // half-quantum window: past that point the bracket pins the parameter
    // quantum to at most two candidates (below).
    while span * (hi - lo) > HALF_QUANTUM {
        assert!(
            steps < MAX_SAFEGUARDED_STEPS,
            "safeguarded root solve exceeded {MAX_SAFEGUARDED_STEPS} halvings: each step at \
             least halves a parameter-space bracket of initial width <= {MAX_PARAMETER_SPAN} \
             toward the {HALF_QUANTUM:e} half-quantum, so the bound must hold"
        );
        steps += 1;

        // Bisection substep: unconditional, so the bracket is at least halved
        // every step regardless of what Newton does — this is what makes the
        // `MAX_SAFEGUARDED_STEPS` bound hold. `evaluate`/`derivative` at `mid`
        // double as the data for the Newton substep, so the substep is free.
        let mid = 0.5 * (lo + hi);
        let f_mid = evaluate(mid);
        if f_mid == 0.0 {
            // Representable exact root: its parameter's rounding is the quantum.
            return finish((parameter_of(mid) * scale).round(), steps);
        }
        if f_mid.signum() == f_lo.signum() {
            lo = mid;
            f_lo = f_mid;
        } else {
            hi = mid;
        }

        // Newton substep: refine strictly inside the just-halved bracket. The
        // tangent from `mid` is the classic Newton step; it is taken only when
        // it lands strictly inside the bracket (otherwise it is discarded, the
        // "bisection fallback"), so it can only tighten `[lo, hi]`, never widen
        // it, and the halving bound is preserved. Near a simple root it
        // collapses the bracket far below a half-quantum in a single step.
        let slope = derivative(mid);
        if slope != 0.0 {
            let newton = mid - f_mid / slope;
            if newton > lo && newton < hi {
                let f_newton = evaluate(newton);
                if f_newton == 0.0 {
                    return finish((parameter_of(newton) * scale).round(), steps);
                }
                if f_newton.signum() == f_lo.signum() {
                    lo = newton;
                    f_lo = f_newton;
                } else {
                    hi = newton;
                }
            }
        }
    }

    // The bracket's parameter image `[t_lo, t_hi]` is now at most one
    // half-quantum wide — half the spacing of the quantum-grid rounding
    // boundaries `(k + 1/2) / ROOT_SCALE` — so it contains at most one such
    // boundary, and its endpoints round to quanta `q_lo <= q_hi` differing by
    // at most one.
    let t_lo = parameter_of(lo);
    let t_hi = parameter_of(hi);
    let q_lo = (t_lo * scale).round();
    let q_hi = (t_hi * scale).round();
    if q_lo == q_hi {
        // Every parameter in `[t_lo, t_hi]` — the root's among them — rounds
        // to the same quantum, so `q_lo` is unambiguously the correctly-rounded
        // parameter. Equivalently, the required boundary sign check succeeds by
        // monotonicity without further evaluation: `q_lo`'s rounding boundaries
        // `(q_lo -+ 1/2) / ROOT_SCALE` lie outside `[t_lo, t_hi]` on opposite
        // sides, `f` maps through the increasing `t(u)` and changes sign across
        // the bracket, so its signs at the two boundary preimages are the
        // bracket-endpoint signs — opposite — and the root's parameter lies
        // strictly inside `q_lo`'s quantum.
        return finish(q_lo, steps);
    }
    debug_assert_eq!(q_hi, q_lo + 1.0);

    // The bracket straddles the single rounding boundary
    // `s = (q_lo + 1/2) / ROOT_SCALE` between the two candidate quanta. The
    // correctly-rounded parameter is `q_lo` if the root's parameter is below
    // `s` and `q_hi` if above. Decide with one polynomial sign test at `s`
    // mapped back into the unit variable. `2 * q_lo + 1 < 2^33` is f64-exact
    // and the division is by a power of two, so `s` itself is exact; its unit
    // preimage lies in `[lo, hi] subset [a, b]` (up to f64 mapping noise of
    // ~2^-21 quanta), inside the monotone interval, where `f` shares `f(lo)`'s
    // sign exactly when `s` is still below the root.
    //
    // Degenerate tie (`f == 0` at the boundary preimage, or indistinguishable
    // from zero at a near-tangency): the root's parameter sits on the rounding
    // boundary, `t * ROOT_SCALE = q_lo + 1/2`, and both neighbours are within
    // a half-quantum, so either is a correct rounding. Round half away from
    // zero to match `quantized_parameter`'s own tie rule (`f64::round`), i.e.
    // take `q_hi`.
    let s = (2.0 * q_lo + 1.0) / (2.0 * scale);
    let f_s = evaluate((s - start) / span);
    let q = if f_s == 0.0 || f_s.signum() == f_lo.signum() {
        q_hi
    } else {
        q_lo
    };
    finish(q, steps)
}

/// Bound: a quadratic (or, when the quadratic coefficient is negligible,
/// linear) polynomial has at most 2 real roots — the fundamental theorem of
/// algebra applied to a degree-2 polynomial. This function pushes at most
/// the discriminant-derived pair `[(-b-root)/(2a), (-b+root)/(2a)]` when a
/// real solution exists, or fewer when the polynomial degenerates.
fn quadratic_roots(constant: f64, linear: f64, quadratic: f64) -> Bounded<f64, 2> {
    let mut roots = Bounded::new();
    if quadratic.abs() <= POLYNOMIAL_EPSILON {
        if linear.abs() > POLYNOMIAL_EPSILON {
            roots.push(-constant / linear);
        }
        return roots;
    }
    let discriminant = linear * linear - 4.0 * quadratic * constant;
    if discriminant < -POLYNOMIAL_EPSILON {
        return roots;
    }
    let root = discriminant.max(0.0).sqrt();
    roots.push((-linear - root) / (2.0 * quadratic));
    roots.push((-linear + root) / (2.0 * quadratic));
    roots
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
    use relief_core::{Bounds, CanonicalView, Chart, ReliefField, WarpCoefficients};

    use crate::{CameraBasis, TargetView};

    use super::{
        Bounded, HALF_QUANTUM, MAX_PARAMETER_SPAN, MAX_SAFEGUARDED_STEPS, PreparedRelief,
        ROOT_SCALE, RenderScratch, correctly_rounded_root, interpolate_unit_cubic,
        quantized_parameter, ratio_to_f64, roots_in_unit_interval, solve_preimages,
    };

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
    fn scalar_solver_retains_three_preimages_of_a_fold() {
        let values = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0]
            .map(|value| (value - 0.2) * (value - 0.5) * (value - 0.8));
        let roots = roots_in_unit_interval(interpolate_unit_cubic(values), 0.0, 1.0);

        assert_eq!(roots.as_slice().len(), 3);
        assert!((roots.as_slice()[0] - 0.2).abs() < 1.0e-6);
        assert!((roots.as_slice()[1] - 0.5).abs() < 1.0e-6);
        assert!((roots.as_slice()[2] - 0.8).abs() < 1.0e-6);
    }

    #[test]
    fn scalar_solver_keeps_a_tangent_preimage() {
        let values = [0.25, 1.0 / 36.0, 1.0 / 36.0, 0.25];
        let roots = roots_in_unit_interval(interpolate_unit_cubic(values), 0.0, 1.0);

        assert_eq!(roots.as_slice(), [0.5]);
    }

    /// Evaluates `c0 + c1 x + c2 x^2 + c3 x^3` — a standalone oracle for the
    /// half-quantum sign property, independent of the solver's own `evaluate`.
    fn poly(coefficients: &[f64; 4], x: f64) -> f64 {
        coefficients[0] + x * (coefficients[1] + x * (coefficients[2] + x * coefficients[3]))
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
    /// here on `2 x^3 - 1`, whose only root in `(0, 1)` is the irrational
    /// `2^(-1/3)`, over the unit segment where parameter and unit coincide.
    #[test]
    fn correctly_rounded_root_certifies_irrational_root_by_half_quantum_sign_change() {
        let coefficients = [-1.0, 0.0, 0.0, 2.0];
        let true_root = 2.0_f64.powf(-1.0 / 3.0);
        let (root, steps) = correctly_rounded_root(
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
        assert!(steps <= MAX_SAFEGUARDED_STEPS, "took {steps} steps");
    }

    /// The span mismatch this solver exists to handle: over a segment of span
    /// 192 (the largest observed in the fixture set — the globe's relief
    /// range), a unit-variable half-quantum is 96 parameter quanta wide, so
    /// correctness must be certified on the parameter grid. The same
    /// irrational-root cubic is solved over `[start, end] = [10, 202]` and the
    /// half-quantum sign property is asserted in parameter space.
    #[test]
    fn correctly_rounded_root_certifies_the_parameter_quantum_across_a_wide_span() {
        let (start, end) = (10.0, 202.0);
        let coefficients = [-1.0, 0.0, 0.0, 2.0];
        let true_unit_root = 2.0_f64.powf(-1.0 / 3.0);
        let true_parameter = start + (end - start) * true_unit_root;
        let (root, steps) = correctly_rounded_root(
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
        assert!(steps <= MAX_SAFEGUARDED_STEPS, "took {steps} steps");
    }

    /// A root whose parameter lands exactly on a representable quantum is
    /// returned exactly. `x^3 - 1/8` has the root `1/2 = 2^23 / 2^24` over the
    /// unit segment, and every quantity here is a power of two, so the solve
    /// hits it with no rounding at all.
    #[test]
    fn correctly_rounded_root_returns_an_exactly_representable_quantum() {
        let coefficients = [-0.125, 0.0, 0.0, 1.0];
        let (root, steps) = correctly_rounded_root(
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
        assert!(steps <= MAX_SAFEGUARDED_STEPS, "took {steps} steps");
    }

    /// A near-tangency with a genuine sign change: `(x - 3/10)^3 + m (x - 3/10)`
    /// has the single real root `3/10`, and with tiny `m` its derivative there
    /// is only `m`, so Newton steps overshoot and are rejected — the bisection
    /// safeguard still converges to the correct quantum.
    #[test]
    fn correctly_rounded_root_converges_through_a_near_tangency() {
        let m = 1.0e-3;
        // Coefficients of (x - 0.3)^3 + m (x - 0.3).
        let coefficients = [-0.027 - m * 0.3, 0.27 + m, -0.9, 1.0];
        let (root, steps) = correctly_rounded_root(
            &coefficients,
            0.0,
            1.0,
            poly(&coefficients, 0.0),
            poly(&coefficients, 1.0),
            0.0,
            1.0,
        );

        assert!(
            (root - 0.3).abs() <= 1.0 / ROOT_SCALE as f64,
            "near-tangent root {root} is farther than one quantum from 0.3"
        );
        assert!(steps <= MAX_SAFEGUARDED_STEPS, "took {steps} steps");
    }

    /// The `MAX_SAFEGUARDED_STEPS` bound is derived, not tuned: each step at
    /// least halves the parameter-space bracket, the initial width is at most
    /// `MAX_PARAMETER_SPAN`, and the loop stops at `HALF_QUANTUM = 2^-25`, so
    /// 33 halvings are exactly sufficient and 32 are not.
    #[test]
    fn safeguarded_step_bound_is_the_minimal_sufficient_halving_count() {
        assert!(MAX_PARAMETER_SPAN * 2.0_f64.powi(-(MAX_SAFEGUARDED_STEPS as i32)) <= HALF_QUANTUM);
        assert!(
            MAX_PARAMETER_SPAN * 2.0_f64.powi(-((MAX_SAFEGUARDED_STEPS - 1) as i32)) > HALF_QUANTUM
        );
    }

    /// The worst case respects the step bound: the widest permitted segment
    /// (`span = MAX_PARAMETER_SPAN`) with a Newton-hostile cubic.
    /// `(x - 1/2)^3 + m (x - 1/2) + k` is monotone with its inflection at
    /// `1/2`; the tangent from a midpoint near the inflection points far
    /// outside the bracket, so Newton is rejected until the bracket has homed
    /// in, forcing the bisection safeguard to carry the early steps.
    #[test]
    fn safeguarded_root_solve_never_exceeds_the_step_bound() {
        let m = 1.0e-2;
        let k = 0.064 + m * 0.4; // places the root at 1/10
        let coefficients = [-0.125 - m * 0.5 + k, 0.75 + m, -1.5, 1.0];
        let (root, steps) = correctly_rounded_root(
            &coefficients,
            0.0,
            1.0,
            poly(&coefficients, 0.0),
            poly(&coefficients, 1.0),
            0.0,
            MAX_PARAMETER_SPAN,
        );

        assert!(
            (root - 0.1).abs() <= 1.0 / (ROOT_SCALE as f64 * MAX_PARAMETER_SPAN),
            "root {root} off 0.1"
        );
        assert!(
            steps <= MAX_SAFEGUARDED_STEPS,
            "safeguarded solve took {steps} steps, exceeding {MAX_SAFEGUARDED_STEPS}"
        );
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
}
