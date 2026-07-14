use std::collections::{BTreeMap, BTreeSet};

use num_rational::Ratio;
use relief_core::{CanonicalView, Chart, DecodedTexel, ReliefField, SourcePoint, WarpedSample};

use crate::diagnostic::normalize_diagnostics;
use thiserror::Error;

use crate::{
    FragmentKey, FrameBuffer, RenderDiagnostic, TargetView, framebuffer::commit_fragment,
    presets::TargetExtents,
};

const MICROCELLS_PER_AXIS: i64 = 8;

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
struct Vertex {
    warped: WarpedSample,
    flat_screen: [Ratio<i64>; 2],
}

type EqualDepthColors = BTreeMap<Ratio<i64>, BTreeSet<(CanonicalView, [u8; 3])>>;

#[derive(Clone, Copy, Debug)]
struct TightRegion {
    min_x: Ratio<i64>,
    max_x: Ratio<i64>,
    min_y: Ratio<i64>,
    max_y: Ratio<i64>,
}

impl TightRegion {
    fn centered(
        request: &RenderRequest,
        min_x: Ratio<i64>,
        max_x: Ratio<i64>,
        min_y: Ratio<i64>,
        max_y: Ratio<i64>,
    ) -> Self {
        let center_x = Ratio::new(i64::from(request.width), 2);
        let center_y = Ratio::new(i64::from(request.height), 2);
        let half_width = (max_x - min_x) / 2;
        let half_height = (max_y - min_y) / 2;
        Self {
            min_x: center_x - half_width,
            max_x: center_x + half_width,
            min_y: center_y - half_height,
            max_y: center_y + half_height,
        }
    }

    fn contains(self, x: u32, y: u32) -> bool {
        let center_x = Ratio::new(2 * i64::from(x) + 1, 2);
        let center_y = Ratio::new(2 * i64::from(y) + 1, 2);
        center_x >= self.min_x
            && center_x < self.max_x
            && center_y >= self.min_y
            && center_y < self.max_y
    }
}

struct RasterState<'a> {
    frame: &'a mut FrameBuffer,
    equal_depth: &'a mut [EqualDepthColors],
    candidate_views: &'a mut [BTreeSet<CanonicalView>],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PixelBounds {
    x_start: u32,
    x_end: u32,
    y_start: u32,
    y_end: u32,
}

pub fn render_model(charts: &[Chart], request: &RenderRequest) -> Result<FrameBuffer, RenderError> {
    let pixel_count = (request.width as usize)
        .checked_mul(request.height as usize)
        .ok_or(RenderError::FrameBufferTooLarge)?;
    let mut frame = FrameBuffer::transparent(request.width, request.height);
    record_relief_bound_diagnostics(charts, &mut frame.diagnostics);
    if pixel_count == 0 {
        finish_diagnostics(&mut frame);
        return Ok(frame);
    }

    let extents = projected_extents(charts, &request.target);
    let Some(TargetExtents {
        min_x,
        max_x,
        min_y,
        max_y,
    }) = extents
    else {
        finish_diagnostics(&mut frame);
        return Ok(frame);
    };
    let offset_x = Ratio::new(i64::from(request.width), 2) - (min_x + max_x) / 2;
    let offset_y = Ratio::new(i64::from(request.height), 2) - (min_y + max_y) / 2;
    let tight_region = TightRegion::centered(request, min_x, max_x, min_y, max_y);
    let mut equal_depth = vec![BTreeMap::new(); pixel_count];
    let mut candidate_views = vec![BTreeSet::new(); pixel_count];
    {
        let mut raster = RasterState {
            frame: &mut frame,
            equal_depth: &mut equal_depth,
            candidate_views: &mut candidate_views,
        };

        for chart in charts {
            let Some(warp) = request
                .target
                .warp_coefficients(chart.view(), chart.bounds())
            else {
                continue;
            };
            let relief = ReliefField::new(chart);
            let (width, height) = chart.dimensions();

            for source_y in 0..height {
                for source_x in 0..width {
                    let Some(cell) = relief.foreground_cell(source_x, source_y) else {
                        continue;
                    };

                    for micro_y in 0..MICROCELLS_PER_AXIS {
                        for micro_x in 0..MICROCELLS_PER_AXIS {
                            let corners = [
                                (micro_x, micro_y),
                                (micro_x + 1, micro_y),
                                (micro_x + 1, micro_y + 1),
                                (micro_x, micro_y + 1),
                            ]
                            .map(|(sub_x, sub_y)| {
                                let source = SourcePoint::new(
                                    Ratio::from_integer(i64::from(source_x))
                                        + Ratio::new(sub_x, MICROCELLS_PER_AXIS),
                                    Ratio::from_integer(i64::from(source_y))
                                        + Ratio::new(sub_y, MICROCELLS_PER_AXIS),
                                );
                                let height = cell.sample_closure(source.clone()).expect(
                                    "microcell corner belongs to its foreground cell closure",
                                );
                                let mut warped = warp.apply(source.clone(), height);
                                let mut flat = warp.apply(source.clone(), Ratio::from_integer(0));
                                warped.screen_x += offset_x;
                                warped.screen_y += offset_y;
                                flat.screen_x += offset_x;
                                flat.screen_y += offset_y;
                                Vertex {
                                    warped,
                                    flat_screen: [flat.screen_x, flat.screen_y],
                                }
                            });

                            rasterize_triangle(
                                chart,
                                [&corners[0], &corners[1], &corners[2]],
                                &mut raster,
                                (source_x, source_y),
                            );
                            rasterize_triangle(
                                chart,
                                [&corners[0], &corners[2], &corners[3]],
                                &mut raster,
                                (source_x, source_y),
                            );
                        }
                    }
                }
            }
        }
    }

    let output_covered_pixels = frame.keys.iter().filter(|key| key.is_some()).count() as u32;
    let conflicting_pixels = candidate_views
        .iter()
        .filter(|views| views.len() > 1)
        .count() as u32;
    if output_covered_pixels > 0
        && u64::from(conflicting_pixels) * 5 > u64::from(output_covered_pixels)
    {
        frame.diagnostics.push(RenderDiagnostic::HeavyChartOverlap {
            covered_pixels: output_covered_pixels,
            conflicting_pixels,
        });
    }

    let total_pixels = (0..request.height)
        .flat_map(|y| (0..request.width).map(move |x| (x, y)))
        .filter(|(x, y)| tight_region.contains(*x, *y))
        .count() as u32;
    let covered_pixels = frame
        .keys
        .iter()
        .enumerate()
        .filter(|(index, key)| {
            let x = (*index % request.width as usize) as u32;
            let y = (*index / request.width as usize) as u32;
            key.is_some() && tight_region.contains(x, y)
        })
        .count() as u32;
    if total_pixels > 0 && u64::from(covered_pixels) * 10 < u64::from(total_pixels) * 7 {
        frame
            .diagnostics
            .push(RenderDiagnostic::InsufficientCoverage {
                covered_pixels,
                total_pixels,
            });
    }

    finish_diagnostics(&mut frame);
    Ok(frame)
}

fn record_relief_bound_diagnostics(charts: &[Chart], diagnostics: &mut Vec<RenderDiagnostic>) {
    for chart in charts {
        let opposing_plane_eighths = match chart.view() {
            CanonicalView::Front | CanonicalView::Back => chart.bounds().depth().saturating_mul(8),
            CanonicalView::Left | CanonicalView::Right => chart.bounds().width().saturating_mul(8),
            CanonicalView::Top | CanonicalView::Bottom => chart.bounds().height().saturating_mul(8),
        };
        let (width, height) = chart.dimensions();
        for source_y in 0..height {
            for source_x in 0..width {
                if let Some(DecodedTexel::Relief { eighths, .. }) = chart.texel(source_x, source_y)
                    && u32::from(eighths) > opposing_plane_eighths
                {
                    diagnostics.push(RenderDiagnostic::ReliefBeyondOpposingPlane {
                        view: chart.view(),
                        source_x,
                        source_y,
                    });
                }
            }
        }
    }
}

fn finish_diagnostics(frame: &mut FrameBuffer) {
    normalize_diagnostics(&mut frame.diagnostics);
}

fn projected_extents(charts: &[Chart], target: &TargetView) -> Option<TargetExtents> {
    charts
        .first()
        .map(|chart| target.framing_extents(chart.bounds()))
}

fn rasterize_triangle(
    chart: &Chart,
    mut vertices: [&Vertex; 3],
    state: &mut RasterState<'_>,
    source_cell: (u32, u32),
) {
    let rgb = match chart.texel(source_cell.0, source_cell.1) {
        Some(DecodedTexel::Relief { rgb, .. }) => rgb,
        _ => unreachable!("only an owning foreground cell reaches rasterization"),
    };
    let [first, second, third] = vertices;
    let flat_area = flat_edge(first, second, third.flat_screen[0], third.flat_screen[1]);
    let mut area = edge(
        first,
        second,
        &third.warped.screen_x,
        &third.warped.screen_y,
    );
    if area == Ratio::from_integer(0) {
        return;
    }
    if flat_area != Ratio::from_integer(0)
        && (flat_area < Ratio::from_integer(0)) != (area < Ratio::from_integer(0))
    {
        state.frame.diagnostics.push(RenderDiagnostic::WarpFold {
            view: chart.view(),
            source_x: source_cell.0,
            source_y: source_cell.1,
        });
    }
    if area < Ratio::from_integer(0) {
        vertices.swap(1, 2);
        area = -area;
    }

    let Some(bounds) = triangle_pixel_bounds(&vertices, state.frame.width(), state.frame.height())
    else {
        return;
    };

    for y in bounds.y_start..bounds.y_end {
        for x in bounds.x_start..bounds.x_end {
            let point_x = Ratio::new(2 * i64::from(x) + 1, 2);
            let point_y = Ratio::new(2 * i64::from(y) + 1, 2);
            if !covered_by_top_left_rule(&vertices, &point_x, &point_y) {
                continue;
            }

            let weights = [
                edge(vertices[1], vertices[2], &point_x, &point_y) / area,
                edge(vertices[2], vertices[0], &point_x, &point_y) / area,
                edge(vertices[0], vertices[1], &point_x, &point_y) / area,
            ];
            let interpolate = |values: [&Ratio<i64>; 3]| {
                weights
                    .iter()
                    .zip(values)
                    .fold(Ratio::from_integer(0), |sum, (weight, value)| {
                        sum + *weight * *value
                    })
            };
            let depth = interpolate([
                &vertices[0].warped.depth,
                &vertices[1].warped.depth,
                &vertices[2].warped.depth,
            ]);
            let (source_x, source_y) = source_cell;
            let index = (y * state.frame.width() + x) as usize;
            state.candidate_views[index].insert(chart.view());
            for &(other_view, other_rgb) in
                state.equal_depth[index].get(&depth).into_iter().flatten()
            {
                if other_rgb != rgb {
                    let (first, second) = if other_view < chart.view() {
                        (other_view, chart.view())
                    } else {
                        (chart.view(), other_view)
                    };
                    state
                        .frame
                        .diagnostics
                        .push(RenderDiagnostic::EqualDepthColorConflict {
                            x,
                            y,
                            first,
                            second,
                        });
                }
            }
            state.equal_depth[index]
                .entry(depth)
                .or_default()
                .insert((chart.view(), rgb));
            commit_fragment(
                state.frame,
                x,
                y,
                FragmentKey {
                    depth,
                    chart_rank: chart.view().rank(),
                    source_y,
                    source_x,
                },
                rgb,
            );
        }
    }
}

fn triangle_pixel_bounds(vertices: &[&Vertex; 3], width: u32, height: u32) -> Option<PixelBounds> {
    let min_x = vertices
        .iter()
        .map(|vertex| vertex.warped.screen_x)
        .min()
        .expect("a triangle has three x coordinates");
    let max_x = vertices
        .iter()
        .map(|vertex| vertex.warped.screen_x)
        .max()
        .expect("a triangle has three x coordinates");
    let min_y = vertices
        .iter()
        .map(|vertex| vertex.warped.screen_y)
        .min()
        .expect("a triangle has three y coordinates");
    let max_y = vertices
        .iter()
        .map(|vertex| vertex.warped.screen_y)
        .max()
        .expect("a triangle has three y coordinates");
    let (x_start, x_end) = pixel_center_range(min_x, max_x, width)?;
    let (y_start, y_end) = pixel_center_range(min_y, max_y, height)?;
    Some(PixelBounds {
        x_start,
        x_end,
        y_start,
        y_end,
    })
}

fn pixel_center_range(minimum: Ratio<i64>, maximum: Ratio<i64>, limit: u32) -> Option<(u32, u32)> {
    let half = Ratio::new(1, 2);
    let start = (minimum - half)
        .ceil()
        .to_integer()
        .clamp(0, i64::from(limit));
    let end = ((maximum - half).floor().to_integer() + 1).clamp(0, i64::from(limit));
    (start < end).then_some((start as u32, end as u32))
}

fn edge(first: &Vertex, second: &Vertex, x: &Ratio<i64>, y: &Ratio<i64>) -> Ratio<i64> {
    (second.warped.screen_x - first.warped.screen_x) * (*y - first.warped.screen_y)
        - (second.warped.screen_y - first.warped.screen_y) * (*x - first.warped.screen_x)
}

fn flat_edge(first: &Vertex, second: &Vertex, x: Ratio<i64>, y: Ratio<i64>) -> Ratio<i64> {
    (second.flat_screen[0] - first.flat_screen[0]) * (y - first.flat_screen[1])
        - (second.flat_screen[1] - first.flat_screen[1]) * (x - first.flat_screen[0])
}

fn covered_by_top_left_rule(vertices: &[&Vertex; 3], x: &Ratio<i64>, y: &Ratio<i64>) -> bool {
    (0..3).all(|index| {
        let first = vertices[index];
        let second = vertices[(index + 1) % 3];
        let value = edge(first, second, x, y);
        value > Ratio::from_integer(0)
            || (value == Ratio::from_integer(0) && is_top_left(first, second))
    })
}

fn is_top_left(first: &Vertex, second: &Vertex) -> bool {
    let dx = second.warped.screen_x - first.warped.screen_x;
    let dy = second.warped.screen_y - first.warped.screen_y;
    dy < Ratio::from_integer(0) || (dy == Ratio::from_integer(0) && dx > Ratio::from_integer(0))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use num_rational::Ratio;
    use relief_core::WarpedSample;

    use super::{PixelBounds, Vertex, covered_by_top_left_rule, edge, triangle_pixel_bounds};

    fn vertex(x: Ratio<i64>, y: Ratio<i64>) -> Vertex {
        Vertex {
            warped: WarpedSample {
                screen_x: x,
                screen_y: y,
                depth: Ratio::from_integer(0),
            },
            flat_screen: [Ratio::from_integer(0), Ratio::from_integer(0)],
        }
    }

    fn point(numerator: i64, denominator: i64) -> Ratio<i64> {
        Ratio::new(numerator, denominator)
    }

    fn assert_bounds_match_full_scan(mut vertices: [Vertex; 3], width: u32, height: u32) {
        if edge(
            &vertices[0],
            &vertices[1],
            &vertices[2].warped.screen_x,
            &vertices[2].warped.screen_y,
        ) < Ratio::from_integer(0)
        {
            vertices.swap(1, 2);
        }
        let refs = [&vertices[0], &vertices[1], &vertices[2]];
        let full: BTreeSet<_> = (0..height)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .filter(|(x, y)| {
                covered_by_top_left_rule(
                    &refs,
                    &Ratio::new(2 * i64::from(*x) + 1, 2),
                    &Ratio::new(2 * i64::from(*y) + 1, 2),
                )
            })
            .collect();
        let bounded = triangle_pixel_bounds(&refs, width, height);
        let bounded_hits: BTreeSet<_> = bounded
            .into_iter()
            .flat_map(|bounds| {
                (bounds.y_start..bounds.y_end).flat_map(move |y| {
                    (bounds.x_start..bounds.x_end).filter_map(move |x| {
                        covered_by_top_left_rule(
                            &refs,
                            &Ratio::new(2 * i64::from(x) + 1, 2),
                            &Ratio::new(2 * i64::from(y) + 1, 2),
                        )
                        .then_some((x, y))
                    })
                })
            })
            .collect();
        assert_eq!(bounded_hits, full);
    }

    #[test]
    fn exact_bounds_keep_negative_and_partly_offscreen_coverage() {
        assert_bounds_match_full_scan(
            [
                vertex(point(-3, 2), point(-1, 2)),
                vertex(point(7, 2), point(1, 2)),
                vertex(point(1, 2), point(9, 2)),
            ],
            4,
            4,
        );
    }

    #[test]
    fn exact_bounds_keep_fractional_edge_pixel_centers() {
        assert_bounds_match_full_scan(
            [
                vertex(point(1, 4), point(1, 4)),
                vertex(point(13, 4), point(3, 4)),
                vertex(point(3, 4), point(15, 4)),
            ],
            5,
            5,
        );
    }

    #[test]
    fn exact_bounds_are_winding_independent_after_mirrored_normalization() {
        assert_bounds_match_full_scan(
            [
                vertex(point(1, 1), point(1, 1)),
                vertex(point(1, 1), point(4, 1)),
                vertex(point(4, 1), point(1, 1)),
            ],
            6,
            6,
        );
    }

    #[test]
    fn exact_bounds_keep_frame_boundary_touching_triangles_without_full_scan() {
        let vertices = [
            vertex(point(2, 1), point(2, 1)),
            vertex(point(4, 1), point(2, 1)),
            vertex(point(4, 1), point(4, 1)),
        ];
        let refs = [&vertices[0], &vertices[1], &vertices[2]];
        assert_eq!(
            triangle_pixel_bounds(&refs, 4, 4),
            Some(PixelBounds {
                x_start: 2,
                x_end: 4,
                y_start: 2,
                y_end: 4,
            })
        );
        assert_bounds_match_full_scan(vertices, 4, 4);
    }
}
