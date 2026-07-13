use std::collections::{BTreeMap, BTreeSet};

use num_rational::Ratio;
use relief_core::{CanonicalView, Chart, DecodedTexel, ReliefField, SourcePoint, WarpedSample};
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
    source: SourcePoint,
    warped: WarpedSample,
}

type EqualDepthColors = BTreeMap<Ratio<i64>, BTreeMap<CanonicalView, [u8; 3]>>;

struct RasterState<'a> {
    frame: &'a mut FrameBuffer,
    equal_depth: &'a mut [EqualDepthColors],
    candidate_views: &'a mut [BTreeSet<CanonicalView>],
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
                                warped.screen_x += offset_x;
                                warped.screen_y += offset_y;
                                Vertex { source, warped }
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

    let covered_pixels = frame.keys.iter().filter(|key| key.is_some()).count() as u32;
    let conflicting_pixels = candidate_views
        .iter()
        .filter(|views| views.len() > 1)
        .count() as u32;
    if covered_pixels > 0 && u64::from(conflicting_pixels) * 5 > u64::from(covered_pixels) {
        frame.diagnostics.push(RenderDiagnostic::HeavyChartOverlap {
            covered_pixels,
            conflicting_pixels,
        });
    }

    let tight_min_x = Ratio::new(i64::from(request.width), 2) - (max_x - min_x) / 2;
    let tight_max_x = Ratio::new(i64::from(request.width), 2) + (max_x - min_x) / 2;
    let tight_min_y = Ratio::new(i64::from(request.height), 2) - (max_y - min_y) / 2;
    let tight_max_y = Ratio::new(i64::from(request.height), 2) + (max_y - min_y) / 2;
    let total_pixels = (0..request.height)
        .flat_map(|y| (0..request.width).map(move |x| (x, y)))
        .filter(|(x, y)| {
            let center_x = Ratio::new(2 * i64::from(*x) + 1, 2);
            let center_y = Ratio::new(2 * i64::from(*y) + 1, 2);
            center_x >= tight_min_x
                && center_x < tight_max_x
                && center_y >= tight_min_y
                && center_y < tight_max_y
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
    frame.diagnostics.sort();
    frame.diagnostics.dedup();
}

fn projected_extents(charts: &[Chart], target: &TargetView) -> Option<TargetExtents> {
    charts
        .iter()
        .find(|chart| target.is_front_facing(chart.view()))
        .map(|chart| target.framing_extents(chart.bounds()))
}

fn rasterize_triangle(
    chart: &Chart,
    mut vertices: [&Vertex; 3],
    state: &mut RasterState<'_>,
    source_cell: (u32, u32),
) {
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
        state.frame.diagnostics.push(RenderDiagnostic::WarpFold {
            view: chart.view(),
            source_x: source_cell.0,
            source_y: source_cell.1,
        });
        vertices.swap(1, 2);
        area = -area;
    }

    for y in 0..state.frame.height() {
        for x in 0..state.frame.width() {
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
            let source = SourcePoint::new(
                interpolate([
                    &vertices[0].source.x,
                    &vertices[1].source.x,
                    &vertices[2].source.x,
                ]),
                interpolate([
                    &vertices[0].source.y,
                    &vertices[1].source.y,
                    &vertices[2].source.y,
                ]),
            );
            let depth = interpolate([
                &vertices[0].warped.depth,
                &vertices[1].warped.depth,
                &vertices[2].warped.depth,
            ]);
            let Some((source_x, source_y, rgb)) = nearest_source_texel(chart, &source) else {
                continue;
            };
            let index = (y * state.frame.width() + x) as usize;
            state.candidate_views[index].insert(chart.view());
            for (&other_view, &other_rgb) in
                state.equal_depth[index].get(&depth).into_iter().flatten()
            {
                if other_view != chart.view() && other_rgb != rgb {
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
                .entry(chart.view())
                .or_insert(rgb);
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

fn nearest_source_texel(chart: &Chart, point: &SourcePoint) -> Option<(u32, u32, [u8; 3])> {
    let (width, height) = chart.dimensions();
    let mut nearest: Option<(Ratio<i64>, u32, u32, [u8; 3])> = None;

    for y in 0..height {
        for x in 0..width {
            let Some(DecodedTexel::Relief { rgb, .. }) = chart.texel(x, y) else {
                continue;
            };
            let dx = point.x - Ratio::new(2 * i64::from(x) + 1, 2);
            let dy = point.y - Ratio::new(2 * i64::from(y) + 1, 2);
            let distance = dx * dx + dy * dy;
            let candidate = (distance, y, x, rgb);
            if nearest.as_ref().is_none_or(|current| candidate < *current) {
                nearest = Some(candidate);
            }
        }
    }

    nearest.map(|(_, y, x, rgb)| (x, y, rgb))
}
