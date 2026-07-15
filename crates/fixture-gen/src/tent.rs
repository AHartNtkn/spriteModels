use std::error::Error;

use relief_core::{AuthoredModel, Bounds, CanonicalView, Chart, EMPTY_RGBA};

use crate::pixel::{directional_light, rgba, shade};

const WIDTH: i32 = 48;
const HEIGHT: i32 = 28;
const DEPTH: i32 = 36;
const RIDGE_X: i32 = 24;
const EAVE_Y: i32 = 13;
const FABRIC: [u8; 3] = [174, 112, 64];
const STRIPE: [u8; 3] = [198, 142, 78];
const SEAM: [u8; 3] = [116, 72, 44];
const FLAP: [u8; 3] = [154, 78, 48];

pub fn tent_model() -> Result<AuthoredModel, Box<dyn Error>> {
    let bounds = Bounds::new(48, 28, 36)?;
    let charts = vec![
        Chart::from_rgba(CanonicalView::Front, 48, 28, front_pixels())?.with_opposite_assignment(),
        Chart::from_rgba(CanonicalView::Right, 36, 28, right_pixels())?.with_opposite_assignment(),
        Chart::from_rgba(CanonicalView::Top, 48, 36, top_pixels())?,
    ];
    Ok(AuthoredModel::new(bounds, charts)?)
}

fn centered_doubled(coordinate: i32, extent: i32) -> i32 {
    2 * coordinate + 1 - extent
}

fn fold(amplitude: i32, distance_from_ridge: i32, periodic_position: i32, denominator: i32) -> i32 {
    amplitude * distance_from_ridge * periodic_position / denominator
}

fn front_roof_line(x: i32) -> i32 {
    centered_doubled(x, WIDTH).abs() * EAVE_Y / WIDTH
}

fn front_mask(x: i32, y: i32) -> bool {
    y >= front_roof_line(x)
}

fn entrance(x: i32, y: i32) -> bool {
    y >= 18 && centered_doubled(x, WIDTH).abs() <= 11
}

fn flap(x: i32, y: i32) -> bool {
    (19..=22).contains(&x) && y >= 18 + (x - 19)
}

fn front_pixels() -> Vec<[u8; 4]> {
    let mut pixels = Vec::with_capacity((WIDTH * HEIGHT) as usize);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            if !front_mask(x, y) || (entrance(x, y) && !flap(x, y)) {
                pixels.push(EMPTY_RGBA);
                continue;
            }
            let distance = centered_doubled(x, WIDTH).abs();
            let periodic = (y + 2 * x).rem_euclid(8);
            let sag = fold(5, distance, periodic, WIDTH * 7);
            let relief = (6 + sag + if flap(x, y) { 14 } else { 0 }).min(144) as u8;
            let seam = x == RIDGE_X || y == EAVE_Y || x.rem_euclid(12) == 0;
            let striped = (x / 4 + y / 5).rem_euclid(4) == 0;
            let base = if flap(x, y) {
                FLAP
            } else if seam {
                SEAM
            } else if striped {
                STRIPE
            } else {
                FABRIC
            };
            let directional = directional_light(CanonicalView::Front, WIDTH, HEIGHT, x, y);
            pixels.push(rgba(shade(base, 16 + directional, relief), relief));
        }
    }
    pixels
}

fn right_roof_line(z: i32) -> i32 {
    let distance = centered_doubled(z, DEPTH).abs();
    3 + distance.pow(2) * 8 / (DEPTH - 1).pow(2)
}

fn right_pixels() -> Vec<[u8; 4]> {
    let mut pixels = Vec::with_capacity((DEPTH * HEIGHT) as usize);
    for y in 0..HEIGHT {
        for z in 0..DEPTH {
            if y < right_roof_line(z) {
                pixels.push(EMPTY_RGBA);
                continue;
            }
            let distance = centered_doubled(z, DEPTH).abs();
            let periodic = (3 * z + y).rem_euclid(10);
            let sag = fold(7, distance, periodic, DEPTH * 9);
            let wall_curve = distance.pow(2) * 26 / (DEPTH - 1).pow(2);
            let relief = (10 + 2 * (HEIGHT - 1 - y) + wall_curve + sag).min(192) as u8;
            let seam = y == EAVE_Y || z.rem_euclid(9) == 0;
            let striped = (z / 4 + y / 6).rem_euclid(3) == 0;
            let base = if seam {
                SEAM
            } else if striped {
                STRIPE
            } else {
                FABRIC
            };
            let directional = directional_light(CanonicalView::Right, DEPTH, HEIGHT, z, y);
            pixels.push(rgba(shade(base, 4 + directional, relief), relief));
        }
    }
    pixels
}

fn top_pixels() -> Vec<[u8; 4]> {
    let mut pixels = Vec::with_capacity((WIDTH * DEPTH) as usize);
    for z in 0..DEPTH {
        for x in 0..WIDTH {
            let distance = centered_doubled(x, WIDTH).abs();
            let periodic = (z + x / 3).rem_euclid(9);
            let sag = fold(6, distance, periodic, WIDTH * 8);
            let roof_plane = 104 * distance / WIDTH;
            let relief = (roof_plane + sag).min(112) as u8;
            // The x=23/24 ridge and x=0/47 eaves use the same landmark indices
            // as the Front peak/eaves; longitudinal seams repeat across Right.
            let ridge = (x - RIDGE_X).abs() <= 1;
            let eave = x == 0 || x == WIDTH - 1;
            let seam = ridge || eave || z.rem_euclid(9) == 0;
            let striped = (x / 5 + z / 6).rem_euclid(3) == 0;
            let base = if seam {
                SEAM
            } else if striped {
                STRIPE
            } else {
                FABRIC
            };
            let directional = directional_light(CanonicalView::Top, WIDTH, DEPTH, x, z);
            pixels.push(rgba(shade(base, 22 + directional, relief), relief));
        }
    }
    pixels
}
