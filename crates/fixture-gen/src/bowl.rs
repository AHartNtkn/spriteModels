use std::error::Error;

use relief_core::{AuthoredModel, Bounds, CanonicalView, Chart, EMPTY_RGBA};

use crate::pixel::{integer_sqrt, mask_boundary, rgba, shade};

const DIAMETER: i32 = 32;
const RADIUS_DOUBLED: i32 = 32;
const TOP_BASE: [u8; 3] = [190, 126, 62];
const FRONT_BASE: [u8; 3] = [142, 72, 44];

pub fn bowl_model() -> Result<AuthoredModel, Box<dyn Error>> {
    let bounds = Bounds::new(32, 12, 32)?;
    let top = Chart::from_rgba(CanonicalView::Top, 32, 32, top_pixels())?;
    let front = Chart::from_rgba(CanonicalView::Front, 32, 12, front_pixels())?;
    Ok(AuthoredModel::new(bounds, vec![front, top])?)
}

fn centered_doubled(coordinate: i32) -> i32 {
    2 * coordinate + 1 - DIAMETER
}

fn circular_span(radius: u32, offset: u32) -> u32 {
    integer_sqrt(
        radius
            .saturating_pow(2)
            .saturating_sub(offset.saturating_pow(2)),
    )
}

fn top_foreground(x: i32, y: i32) -> bool {
    let dx = centered_doubled(x);
    let dy = centered_doubled(y);
    dx * dx + dy * dy <= RADIUS_DOUBLED * RADIUS_DOUBLED
}

fn top_pixels() -> Vec<[u8; 4]> {
    let mut pixels = Vec::with_capacity(32 * 32);
    for y in 0..32_i32 {
        for x in 0..32_i32 {
            if !top_foreground(x, y) {
                pixels.push(EMPTY_RGBA);
                continue;
            }
            let dx = centered_doubled(x);
            let dy = centered_doubled(y);
            let radial_distance = integer_sqrt((dx * dx + dy * dy) as u32);
            let basin_span = circular_span(RADIUS_DOUBLED as u32, radial_distance);
            let boundary = mask_boundary(x, y, top_foreground);
            let relief = if boundary {
                0
            } else {
                (48 * basin_span / RADIUS_DOUBLED as u32).min(48) as u8
            };
            let directional = (32 - x + 32 - y) / 4;
            let rim_highlight = if boundary { 14 } else { 0 };
            pixels.push(rgba(
                shade(TOP_BASE, 18 + directional + rim_highlight, relief),
                relief,
            ));
        }
    }
    pixels
}

fn half_width_doubled(y: u32) -> u32 {
    32 - 30 * y / 11
}

fn row_front_distance(offset: u32, half_width: u32) -> u32 {
    let normalized_offset = (offset * RADIUS_DOUBLED as u32 / half_width).min(32);
    RADIUS_DOUBLED as u32 - circular_span(RADIUS_DOUBLED as u32, normalized_offset)
}

fn front_pixels() -> Vec<[u8; 4]> {
    let mut pixels = Vec::with_capacity(32 * 12);
    for y in 0..12_u32 {
        let half_width = half_width_doubled(y);
        for x in 0..32_u32 {
            let offset = centered_doubled(x as i32).unsigned_abs();
            if offset > half_width {
                pixels.push(EMPTY_RGBA);
                continue;
            }
            let relief = (4 * row_front_distance(offset, half_width) + 2 * y).min(128) as u8;
            let directional = (32 - x + 12 - y) as i32 / 3;
            let rim_highlight = if y == 0 { 14 } else { 0 };
            pixels.push(rgba(
                shade(FRONT_BASE, 16 + directional + rim_highlight, relief),
                relief,
            ));
        }
    }
    pixels
}
