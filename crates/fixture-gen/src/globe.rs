use std::error::Error;

use relief_core::{AuthoredModel, Bounds, CanonicalView, Chart, EMPTY_RGBA};

use crate::pixel::{integer_sqrt, mask_boundary, rgba, shade};

const SIZE: i32 = 48;
const RADIUS: i32 = 48;
const FRONT_OCEAN: [u8; 3] = [38, 108, 166];
const FRONT_LAND: [u8; 3] = [80, 142, 74];
const BACK_OCEAN: [u8; 3] = [34, 88, 148];
const BACK_LAND: [u8; 3] = [156, 118, 64];

pub fn globe_model() -> Result<AuthoredModel, Box<dyn Error>> {
    let bounds = Bounds::new(48, 48, 48)?;
    let front = Chart::from_rgba(
        CanonicalView::Front,
        48,
        48,
        hemisphere_pixels(CanonicalView::Front),
    )?;
    let back = Chart::from_rgba(
        CanonicalView::Back,
        48,
        48,
        hemisphere_pixels(CanonicalView::Back),
    )?;
    Ok(AuthoredModel::new(bounds, vec![front, back])?)
}

fn doubled_center(coordinate: i32) -> i32 {
    2 * coordinate + 1 - SIZE
}

fn foreground(x: i32, y: i32) -> bool {
    let dx = doubled_center(x);
    let dy = doubled_center(y);
    dx * dx + dy * dy <= RADIUS * RADIUS
}

fn continent(view: CanonicalView, dx: i32, dy: i32) -> bool {
    let longitude = match view {
        CanonicalView::Front => dx + dy / 3 + 11,
        CanonicalView::Back => dx - dy / 4 - 7,
        _ => unreachable!("the globe authors only Front and Back"),
    };
    let latitude_band = (dy + 48).div_euclid(12);
    let longitude_band = (longitude + 96).div_euclid(14);
    match view {
        CanonicalView::Front => {
            (latitude_band + 2 * longitude_band).rem_euclid(5) <= 1 && (dy < 31 || longitude < 9)
        }
        CanonicalView::Back => {
            (2 * latitude_band + longitude_band).rem_euclid(7) <= 2 && (dy > -33 || longitude > -18)
        }
        _ => unreachable!("the globe authors only Front and Back"),
    }
}

fn hemisphere_pixels(view: CanonicalView) -> Vec<[u8; 4]> {
    let mut pixels = Vec::with_capacity(48 * 48);
    for y in 0..48_i32 {
        for x in 0..48_i32 {
            if !foreground(x, y) {
                pixels.push(EMPTY_RGBA);
                continue;
            }
            let dx = doubled_center(x);
            let dy = doubled_center(y);
            let remaining = (RADIUS * RADIUS - dx * dx - dy * dy) as u32;
            let mut relief = (4 * (RADIUS as u32 - integer_sqrt(remaining))).min(192) as u8;
            if mask_boundary(x, y, foreground) {
                relief = 192;
            }

            let inset = relief <= 184;
            let grid = inset && (dx.unsigned_abs() % 12 == 1 || dy.unsigned_abs() % 12 == 1);
            let land = inset && continent(view, dx, dy);
            let base = match (view, land) {
                (CanonicalView::Front, false) => FRONT_OCEAN,
                (CanonicalView::Front, true) => FRONT_LAND,
                (CanonicalView::Back, false) => BACK_OCEAN,
                (CanonicalView::Back, true) => BACK_LAND,
                _ => unreachable!("the globe authors only Front and Back"),
            };
            let directional = (48 - x + 48 - y) / 6;
            let grid_light = if grid { 18 } else { 0 };
            pixels.push(rgba(
                shade(base, 12 + directional + grid_light, relief),
                relief,
            ));
        }
    }
    pixels
}
