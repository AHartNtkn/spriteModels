use num_rational::Ratio;
use relief_core::{
    Bounds, DecodedTexel, InverseWarpLine, ReliefField, ResolvedCharts, SourcePoint,
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

#[derive(Clone, Debug)]
struct Preimage {
    parameter: Ratio<i64>,
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

    fn quadrants_at(&self, source_x: u32, source_y: u32, x: f64, y: f64) -> Vec<usize> {
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
        vertical
            .iter()
            .flat_map(|bottom| horizontal.iter().map(move |right| right + 2 * bottom))
            .collect()
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
    let frame_charts: Vec<_> = prepared
        .charts
        .charts()
        .iter()
        .zip(prepared.reliefs.iter())
        .map(|(chart, entry)| {
            let warp = request.target.warp_coefficients(chart.view(), bounds);
            let facing = request.target.facing_coefficients(chart.view(), bounds);
            (chart, &entry.relief, warp, facing, entry.maximum_relief)
        })
        .collect();

    for y in 0..request.height {
        for x in 0..request.width {
            let screen_x = Ratio::new(2 * i64::from(x) + 1, 2) - offset_x;
            let screen_y = Ratio::new(2 * i64::from(y) + 1, 2) - offset_y;

            for (chart, relief, warp, facing, maximum_relief) in &frame_charts {
                let Some(line) = warp.inverse_line(screen_x, screen_y) else {
                    continue;
                };

                for preimage in solve_preimages(relief, &line, *facing, *maximum_relief) {
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
                            depth: line.depth_at(preimage.parameter),
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

fn solve_preimages(
    field: &PreparedRelief,
    line: &InverseWarpLine,
    facing: FacingCoefficients,
    maximum_relief: f64,
) -> Vec<Preimage> {
    let coefficients = line
        .variable_coefficients()
        .map(|[offset, slope]| [ratio_to_f64(offset), ratio_to_f64(slope)]);
    let [
        [x_offset, x_slope],
        [y_offset, y_slope],
        [relief_offset, relief_slope],
    ] = coefficients;
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
        return Vec::new();
    }
    let [range_start, range_end] = parameter_range;
    if !range_start.is_finite() || !range_end.is_finite() {
        return Vec::new();
    }

    let mut boundaries = vec![range_start, range_end];
    add_grid_crossings(
        &mut boundaries,
        x_offset,
        x_slope,
        width,
        range_start,
        range_end,
    );
    add_grid_crossings(
        &mut boundaries,
        y_offset,
        y_slope,
        height,
        range_start,
        range_end,
    );
    boundaries.sort_by(f64::total_cmp);
    boundaries.dedup_by(|left, right| (*left - *right).abs() <= COORDINATE_EPSILON);

    let mut preimages = Vec::new();
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

                    for unit in roots_in_unit_interval(polynomial) {
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
                            parameter: quantized_ratio(parameter),
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
            && (ratio_to_f64(left.parameter) - ratio_to_f64(right.parameter)).abs() <= 1.0e-7
    });
    preimages
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

fn add_grid_crossings(
    boundaries: &mut Vec<f64>,
    offset: f64,
    slope: f64,
    extent: u32,
    parameter_start: f64,
    parameter_end: f64,
) {
    if slope.abs() <= COORDINATE_EPSILON {
        return;
    }
    for half_step in 0..=extent * 2 {
        let coordinate = f64::from(half_step) * 0.5;
        let parameter = (coordinate - offset) / slope;
        if parameter > parameter_start && parameter < parameter_end {
            boundaries.push(parameter);
        }
    }
}

fn containing_cells(coordinate: f64, extent: u32) -> Vec<u32> {
    if extent == 0
        || coordinate < -COORDINATE_EPSILON
        || coordinate > f64::from(extent) + COORDINATE_EPSILON
    {
        return Vec::new();
    }

    let rounded = coordinate.round();
    if (coordinate - rounded).abs() <= COORDINATE_EPSILON {
        let boundary = rounded.clamp(0.0, f64::from(extent)) as u32;
        match boundary {
            0 => vec![0],
            value if value == extent => vec![extent - 1],
            value => vec![value - 1, value],
        }
    } else {
        vec![(coordinate.floor() as u32).min(extent - 1)]
    }
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

fn roots_in_unit_interval(mut polynomial: [f64; 4]) -> Vec<f64> {
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
        return (polynomial[0].abs() <= tolerance)
            .then_some(vec![0.0, 1.0])
            .unwrap_or_default();
    }

    let mut partitions = vec![0.0, 1.0];
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
    let mut roots = Vec::new();
    for &point in &partitions {
        if evaluate(point).abs() <= tolerance {
            roots.push(point);
        }
    }
    for interval in partitions.windows(2) {
        let mut left = interval[0];
        let mut right = interval[1];
        let mut left_value = evaluate(left);
        let right_value = evaluate(right);
        if left_value.abs() <= tolerance
            || right_value.abs() <= tolerance
            || left_value.signum() == right_value.signum()
        {
            continue;
        }
        for _ in 0..56 {
            let middle = (left + right) * 0.5;
            let middle_value = evaluate(middle);
            if middle_value.abs() <= tolerance {
                left = middle;
                right = middle;
                break;
            }
            if middle_value.signum() == left_value.signum() {
                left = middle;
                left_value = middle_value;
            } else {
                right = middle;
            }
        }
        roots.push((left + right) * 0.5);
    }
    roots.sort_by(f64::total_cmp);
    roots.dedup_by(|left, right| (*left - *right).abs() <= 1.0e-7);
    roots
}

fn quadratic_roots(constant: f64, linear: f64, quadratic: f64) -> Vec<f64> {
    if quadratic.abs() <= POLYNOMIAL_EPSILON {
        return (linear.abs() > POLYNOMIAL_EPSILON)
            .then_some(vec![-constant / linear])
            .unwrap_or_default();
    }
    let discriminant = linear * linear - 4.0 * quadratic * constant;
    if discriminant < -POLYNOMIAL_EPSILON {
        return Vec::new();
    }
    let root = discriminant.max(0.0).sqrt();
    vec![
        (-linear - root) / (2.0 * quadratic),
        (-linear + root) / (2.0 * quadratic),
    ]
}

fn ratio_to_f64(value: Ratio<i64>) -> f64 {
    *value.numer() as f64 / *value.denom() as f64
}

fn quantized_ratio(value: f64) -> Ratio<i64> {
    Ratio::new((value * ROOT_SCALE as f64).round() as i64, ROOT_SCALE)
}

#[cfg(test)]
mod tests {
    use num_rational::Ratio;
    use relief_core::{Bounds, CanonicalView, Chart, ReliefField, WarpCoefficients};

    use crate::{CameraBasis, TargetView};

    use super::{
        PreparedRelief, interpolate_unit_cubic, ratio_to_f64, roots_in_unit_interval,
        solve_preimages,
    };

    #[test]
    fn scalar_solver_retains_three_preimages_of_a_fold() {
        let values = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0]
            .map(|value| (value - 0.2) * (value - 0.5) * (value - 0.8));
        let roots = roots_in_unit_interval(interpolate_unit_cubic(values));

        assert_eq!(roots.len(), 3);
        assert!((roots[0] - 0.2).abs() < 1.0e-6);
        assert!((roots[1] - 0.5).abs() < 1.0e-6);
        assert!((roots[2] - 0.8).abs() < 1.0e-6);
    }

    #[test]
    fn scalar_solver_keeps_a_tangent_preimage() {
        let values = [0.25, 1.0 / 36.0, 1.0 / 36.0, 0.25];
        let roots = roots_in_unit_interval(interpolate_unit_cubic(values));

        assert_eq!(roots, vec![0.5]);
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
        let line = warp
            .inverse_line(Ratio::new(3, 4), Ratio::new(1, 2))
            .unwrap();

        let facing = TargetView::front()
            .facing_coefficients(CanonicalView::Front, Bounds::new(3, 1, 4).unwrap());
        let preimages = solve_preimages(&prepared, &line, facing, 16.0);
        let locations: Vec<_> = preimages
            .iter()
            .map(|preimage| {
                (
                    preimage.source_x,
                    (ratio_to_f64(line.relief_at(preimage.parameter)) * 1_000.0).round() / 1_000.0,
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
        let line = warp
            .inverse_line(Ratio::new(3, 4), Ratio::new(1, 2))
            .unwrap();
        let facing = target.facing_coefficients(CanonicalView::Front, bounds);

        let locations: Vec<_> = solve_preimages(&prepared, &line, facing, 16.0)
            .iter()
            .map(|preimage| {
                (
                    preimage.source_x,
                    (ratio_to_f64(line.relief_at(preimage.parameter)) * 1_000.0).round() / 1_000.0,
                )
            })
            .collect();

        assert_eq!(locations, vec![(1, 4.0), (2, 16.0)]);
    }
}
