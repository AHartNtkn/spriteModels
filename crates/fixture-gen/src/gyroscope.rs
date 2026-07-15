use std::error::Error;

use relief_core::{AuthoredModel, Bounds, CanonicalView, Chart, EMPTY_RGBA};

use crate::pixel::{directional_light, rgba, shade};

const SIZE: i32 = 48;
const RING_BASES: [[u8; 3]; 3] = [[188, 64, 58], [58, 168, 88], [58, 96, 196]];

#[derive(Clone, Copy)]
struct Projection {
    radius_u: i32,
    radius_v: i32,
    direction_u: i32,
    direction_v: i32,
    thickness: i32,
    phase: i32,
    draw_order: i32,
}

// Approved six-view projection table. Records are outer red, middle green,
// inner blue and fields are (radius_u, radius_v, direction_u, direction_v,
// thickness, phase, draw_order). The acceptance fixture carries this same table.
const PROJECTIONS: [(CanonicalView, [Projection; 3]); 6] = [
    (
        CanonicalView::Front,
        [
            p(21, 15, 4, 1, 2, 1, 0),
            p(16, 10, 3, -2, 2, 5, 2),
            p(10, 6, 1, 0, 2, 9, 1),
        ],
    ),
    (
        CanonicalView::Back,
        [
            p(20, 14, 3, -2, 2, 7, 2),
            p(16, 11, 4, 1, 2, 2, 0),
            p(9, 6, 1, 1, 2, 11, 1),
        ],
    ),
    (
        CanonicalView::Left,
        [
            p(21, 13, 2, 3, 2, 4, 1),
            p(15, 11, 1, -3, 2, 8, 0),
            p(10, 5, 1, 0, 2, 12, 2),
        ],
    ),
    (
        CanonicalView::Right,
        [
            p(20, 15, 1, -3, 2, 10, 0),
            p(16, 9, 2, 3, 2, 3, 2),
            p(9, 6, 0, 1, 2, 14, 1),
        ],
    ),
    (
        CanonicalView::Top,
        [
            p(19, 16, 3, 1, 2, 6, 2),
            p(16, 10, 1, 3, 2, 12, 1),
            p(10, 6, 2, -1, 2, 1, 0),
        ],
    ),
    (
        CanonicalView::Bottom,
        [
            p(21, 14, 1, 3, 2, 13, 1),
            p(15, 11, 3, -1, 2, 4, 0),
            p(9, 5, 1, 2, 2, 7, 2),
        ],
    ),
];

const fn p(
    radius_u: i32,
    radius_v: i32,
    direction_u: i32,
    direction_v: i32,
    thickness: i32,
    phase: i32,
    draw_order: i32,
) -> Projection {
    Projection {
        radius_u,
        radius_v,
        direction_u,
        direction_v,
        thickness,
        phase,
        draw_order,
    }
}

pub fn gyroscope_model() -> Result<AuthoredModel, Box<dyn Error>> {
    let bounds = Bounds::new(48, 48, 48)?;
    let charts = PROJECTIONS
        .into_iter()
        .map(|(view, projections)| {
            Chart::from_rgba(view, 48, 48, observation_pixels(view, &projections))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(AuthoredModel::new(bounds, charts)?)
}

fn centered_doubled(coordinate: i32) -> i32 {
    2 * coordinate + 1 - SIZE
}

fn projection_sample(x: i32, y: i32, projection: Projection) -> Option<(i64, i64, i64)> {
    let x = centered_doubled(x);
    let y = centered_doubled(y);
    let u = i64::from(projection.direction_u * x + projection.direction_v * y);
    let v = i64::from(-projection.direction_v * x + projection.direction_u * y);
    let norm = i64::from(projection.direction_u.pow(2) + projection.direction_v.pow(2));
    let radius_u = i64::from(2 * projection.radius_u);
    let radius_v = i64::from(2 * projection.radius_v);
    let metric =
        u.pow(2) * 4096 / (norm * radius_u.pow(2)) + v.pow(2) * 4096 / (norm * radius_v.pow(2));
    // Linearizing the squared ellipse metric gives 2 * thickness / radius;
    // use the minor radius so the requested source-pixel half-width survives.
    let band = 8192 * i64::from(projection.thickness)
        / i64::from(projection.radius_u.min(projection.radius_v));
    ((metric - 4096).abs() <= band).then_some((metric, u, v))
}

fn arc_step(u: i64, v: i64) -> i32 {
    let abs_u = u.unsigned_abs() as i64;
    let abs_v = v.unsigned_abs() as i64;
    let fraction = (4 * abs_v / (abs_u + abs_v).max(1)).min(3) as i32;
    match (u >= 0, v >= 0) {
        (true, true) => fraction,
        (false, true) => 7 - fraction,
        (false, false) => 8 + fraction,
        (true, false) => 15 - fraction,
    }
}

fn observation_pixels(view: CanonicalView, projections: &[Projection; 3]) -> Vec<[u8; 4]> {
    let face_light = match view {
        CanonicalView::Top => 18,
        CanonicalView::Front => 12,
        CanonicalView::Left => 7,
        CanonicalView::Right => 1,
        CanonicalView::Back => -5,
        CanonicalView::Bottom => -10,
    };
    let mut pixels = Vec::with_capacity((SIZE * SIZE) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let selected = projections
                .iter()
                .copied()
                .enumerate()
                .filter_map(|(ring, projection)| {
                    projection_sample(x, y, projection)
                        .map(|sample| (projection.draw_order, ring, projection, sample))
                })
                .max_by_key(|(draw_order, _, _, _)| *draw_order);
            let Some((_, ring, projection, (metric, u, v))) = selected else {
                pixels.push(EMPTY_RGBA);
                continue;
            };

            let band = 8192 * i64::from(projection.thickness)
                / i64::from(projection.radius_u.min(projection.radius_v));
            let across_band = ((metric - 4096).abs() * 88 / band) as u8;
            let phased_arc = (arc_step(u, v) + projection.phase).rem_euclid(16) as u8;
            let relief = 8 + across_band + 5 * phased_arc;
            debug_assert!(relief <= 192);
            let directional = directional_light(view, SIZE, SIZE, x, y);
            pixels.push(rgba(
                shade(RING_BASES[ring], face_light + directional, relief),
                relief,
            ));
        }
    }
    pixels
}
