use std::error::Error;

use relief_core::{AuthoredModel, Bounds, CanonicalView, Chart, EMPTY_RGBA};

use crate::pixel::{directional_light, integer_sqrt, rgba, shade};

const WIDTH: i32 = 48;
const HEIGHT: i32 = 32;
const DEPTH: i32 = 48;
const DOME_EAVE_Y: i32 = 20;
const HORIZONTAL_RADIUS: i32 = 48;
const VERTICAL_RADIUS: i32 = 40;
const STONE: [u8; 3] = [132, 142, 150];
const DRUM_STONE: [u8; 3] = [118, 128, 138];
const RIB: [u8; 3] = [188, 152, 78];
const WINDOW: [u8; 3] = [42, 58, 74];

pub fn dome_model() -> Result<AuthoredModel, Box<dyn Error>> {
    let bounds = Bounds::new(48, 32, 48)?;
    let charts = vec![
        Chart::from_rgba(
            CanonicalView::Front,
            48,
            32,
            side_pixels(CanonicalView::Front),
        )?
        .with_opposite_assignment(),
        Chart::from_rgba(
            CanonicalView::Right,
            48,
            32,
            side_pixels(CanonicalView::Right),
        )?
        .with_opposite_assignment(),
        Chart::from_rgba(CanonicalView::Top, 48, 48, top_pixels())?,
    ];
    Ok(AuthoredModel::new(bounds, charts)?)
}

fn centered_doubled(coordinate: i32, extent: i32) -> i32 {
    2 * coordinate + 1 - extent
}

fn dome_cross_section_remaining(dx: i32, y: i32) -> i32 {
    let dy = 2 * y + 1 - 2 * DOME_EAVE_Y;
    HORIZONTAL_RADIUS.pow(2)
        - dx.pow(2)
        - dy.pow(2) * HORIZONTAL_RADIUS.pow(2) / VERTICAL_RADIUS.pow(2)
}

fn side_foreground(x: i32, y: i32) -> bool {
    let dx = centered_doubled(x, WIDTH);
    if y < DOME_EAVE_Y {
        dome_cross_section_remaining(dx, y) >= 0
    } else {
        dx.abs() <= 43
    }
}

fn dome_rib(dx: i32, y: i32) -> bool {
    let dy = 2 * y + 1 - 2 * DOME_EAVE_Y;
    let half_span_squared =
        HORIZONTAL_RADIUS.pow(2) - dy.pow(2) * HORIZONTAL_RADIUS.pow(2) / VERTICAL_RADIUS.pow(2);
    if half_span_squared <= 0 {
        return true;
    }
    let half_span = integer_sqrt(half_span_squared as u32).max(1) as i32;
    let angular_index = dx * 24 / half_span;
    angular_index.rem_euclid(8) <= 1 || y <= 1
}

fn drum_window(x: i32, y: i32, phase: i32) -> bool {
    if !(24..=28).contains(&y) {
        return false;
    }
    let bay = (x + phase).rem_euclid(12);
    (4..=7).contains(&bay)
}

fn side_pixels(view: CanonicalView) -> Vec<[u8; 4]> {
    let phase = match view {
        CanonicalView::Front => 0,
        CanonicalView::Right => 3,
        _ => unreachable!("dome sides are Front and Right"),
    };
    let face_light = if view == CanonicalView::Front { 14 } else { 3 };
    let mut pixels = Vec::with_capacity((WIDTH * HEIGHT) as usize);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            if !side_foreground(x, y) {
                pixels.push(EMPTY_RGBA);
                continue;
            }
            let dx = centered_doubled(x, WIDTH);
            let dome = y < DOME_EAVE_Y;
            let span = if dome {
                integer_sqrt(dome_cross_section_remaining(dx, y) as u32) as i32
            } else {
                integer_sqrt((HORIZONTAL_RADIUS.pow(2) - dx.pow(2)) as u32) as i32
            };
            let shell_relief = 2 * (HORIZONTAL_RADIUS - span);
            let molding = if !dome && matches!(y, 20 | 22 | 30) {
                8
            } else {
                0
            };
            let relief = (shell_relief + molding).min(192) as u8;
            let rib = if dome {
                dome_rib(dx, y)
            } else {
                x.rem_euclid(8) <= 1 || matches!(y, 20 | 22 | 30)
            };
            let window = !rib && drum_window(x, y, phase);
            let base = if window {
                WINDOW
            } else if rib {
                RIB
            } else if dome {
                STONE
            } else {
                DRUM_STONE
            };
            let directional = directional_light(view, WIDTH, HEIGHT, x, y);
            pixels.push(rgba(shade(base, face_light + directional, relief), relief));
        }
    }
    pixels
}

fn top_foreground(x: i32, z: i32) -> bool {
    let dx = centered_doubled(x, WIDTH);
    let dz = centered_doubled(z, DEPTH);
    dx.pow(2) + dz.pow(2) <= HORIZONTAL_RADIUS.pow(2)
}

fn radial_rib(dx: i32, dz: i32) -> bool {
    dx.abs() <= 1
        || dz.abs() <= 1
        || (dx - dz).abs() <= 2
        || (dx + dz).abs() <= 2
        || (2 * dx - dz).abs() <= 2
        || (2 * dx + dz).abs() <= 2
        || (dx - 2 * dz).abs() <= 2
        || (dx + 2 * dz).abs() <= 2
}

fn top_pixels() -> Vec<[u8; 4]> {
    let mut pixels = Vec::with_capacity((WIDTH * DEPTH) as usize);
    for z in 0..DEPTH {
        for x in 0..WIDTH {
            if !top_foreground(x, z) {
                pixels.push(EMPTY_RGBA);
                continue;
            }
            let dx = centered_doubled(x, WIDTH);
            let dz = centered_doubled(z, DEPTH);
            let radial_remaining = HORIZONTAL_RADIUS.pow(2) - dx.pow(2) - dz.pow(2);
            let vertical_span = integer_sqrt(
                (radial_remaining * VERTICAL_RADIUS.pow(2) / HORIZONTAL_RADIUS.pow(2)) as u32,
            ) as i32;
            let relief = (2 * (VERTICAL_RADIUS - vertical_span)).min(128) as u8;
            let rib = radial_rib(dx, dz) || dx.pow(2) + dz.pow(2) <= 9;
            let base = if rib { RIB } else { STONE };
            let directional = directional_light(CanonicalView::Top, WIDTH, DEPTH, x, z);
            pixels.push(rgba(shade(base, 24 + directional, relief), relief));
        }
    }
    pixels
}
