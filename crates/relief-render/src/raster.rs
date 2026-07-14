use num_rational::Ratio;
use relief_core::{Chart, DecodedTexel, ReliefField, SourcePoint, WarpedSample};
use thiserror::Error;

use crate::{
    FragmentKey, FrameBuffer, TargetView, framebuffer::commit_fragment, presets::TargetExtents,
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
}

struct RasterState<'a> {
    frame: &'a mut FrameBuffer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PixelBounds {
    x_start: u32,
    x_end: u32,
    y_start: u32,
    y_end: u32,
}

pub fn render_model(charts: &[Chart], request: &RenderRequest) -> Result<FrameBuffer, RenderError> {
    (request.width as usize)
        .checked_mul(request.height as usize)
        .ok_or(RenderError::FrameBufferTooLarge)?;
    let mut frame = FrameBuffer::transparent(request.width, request.height);
    if request.width == 0 || request.height == 0 {
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
        return Ok(frame);
    };
    let offset_x = Ratio::new(i64::from(request.width), 2) - (min_x + max_x) / 2;
    let offset_y = Ratio::new(i64::from(request.height), 2) - (min_y + max_y) / 2;
    {
        let mut raster = RasterState { frame: &mut frame };

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
                                warped.screen_x += offset_x;
                                warped.screen_y += offset_y;
                                Vertex { warped }
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

    Ok(frame)
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
    let rgb = match chart.texel_at(source_cell.0, source_cell.1) {
        Some(DecodedTexel::Relief { rgb, .. }) => rgb,
        _ => unreachable!("only an owning foreground cell reaches rasterization"),
    };
    let [first, second, third] = vertices;
    let mut area = edge(
        first,
        second,
        &third.warped.screen_x,
        &third.warped.screen_y,
    );
    if area == Ratio::from_integer(0) {
        return;
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
