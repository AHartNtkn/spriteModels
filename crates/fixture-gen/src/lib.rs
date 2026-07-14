use std::{error::Error, path::Path};

use depthsprite_format::{DepthSpriteModel, save_path_atomic};
use relief_core::{Bounds, CanonicalView, Chart};

const TOP_RGB: [u8; 3] = [216, 156, 85];
const FRONT_RGB: [u8; 3] = [144, 76, 52];
const TOP_RADIUS_DOUBLED: i32 = 28;

pub fn generate_examples(output: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir_all(output)?;
    save_path_atomic(&bowl_model()?, output.join("bowl.depthsprite"))?;
    save_path_atomic(&block_model()?, output.join("block.depthsprite"))?;
    Ok(())
}

fn bowl_model() -> Result<DepthSpriteModel, Box<dyn Error>> {
    let bounds = Bounds::new(32, 16, 32)?;
    let front = Chart::from_rgba(CanonicalView::Front, 32, 16, bowl_front_pixels())?;
    let top = Chart::from_rgba(CanonicalView::Top, 32, 32, bowl_top_pixels())?;
    Ok(DepthSpriteModel::new(bounds, vec![front, top])?)
}

fn top_is_foreground(x: i32, y: i32) -> bool {
    let dx = 2 * x + 1 - 32;
    let dy = 2 * y + 1 - 32;
    dx * dx + dy * dy <= TOP_RADIUS_DOUBLED * TOP_RADIUS_DOUBLED
}

fn top_is_boundary(x: i32, y: i32) -> bool {
    top_is_foreground(x, y)
        && (-1..=1)
            .any(|dy| (-1..=1).any(|dx| (dx != 0 || dy != 0) && !top_is_foreground(x + dx, y + dy)))
}

fn bowl_top_pixels() -> Vec<[u8; 4]> {
    const ZERO_RADIUS_SQUARED: u32 = 650;
    const CENTER_DISTANCE_SQUARED: u32 = 2;
    let mut pixels = Vec::with_capacity(32 * 32);
    for y in 0..32_i32 {
        for x in 0..32_i32 {
            if !top_is_foreground(x, y) {
                pixels.push([0, 0, 0, 0]);
                continue;
            }
            let dx = 2 * x + 1 - 32;
            let dy = 2 * y + 1 - 32;
            let distance_squared = (dx * dx + dy * dy) as u32;
            let relief = if top_is_boundary(x, y) || distance_squared >= ZERO_RADIUS_SQUARED {
                0
            } else {
                let denominator = ZERO_RADIUS_SQUARED - CENTER_DISTANCE_SQUARED;
                let numerator = 64 * (ZERO_RADIUS_SQUARED - distance_squared);
                ((numerator + denominator / 2) / denominator).min(64) as u8
            };
            pixels.push(rgba(TOP_RGB, relief));
        }
    }
    pixels
}

fn integer_sqrt(value: u32) -> u32 {
    let mut low = 0;
    let mut high = value.min(u16::MAX.into()) + 1;
    while low + 1 < high {
        let midpoint = (low + high) / 2;
        if midpoint * midpoint <= value {
            low = midpoint;
        } else {
            high = midpoint;
        }
    }
    low
}

fn rounded_integer_sqrt(value: u32) -> u32 {
    let lower = integer_sqrt(value);
    let upper = lower + 1;
    if value - lower * lower < upper * upper - value {
        lower
    } else {
        upper
    }
}

fn bowl_front_pixels() -> Vec<[u8; 4]> {
    let mut pixels = Vec::with_capacity(32 * 16);
    for y in 0..16_u32 {
        for x in 0..32_u32 {
            let doubled_offset = (2 * x as i32 + 1 - 32).unsigned_abs();
            let profile_height = (31_u32.pow(2) - doubled_offset.pow(2)) * 10 / 31_u32.pow(2);
            let bottom = 4 + profile_height.saturating_sub(1);
            if !(2..=bottom).contains(&y) {
                pixels.push([0, 0, 0, 0]);
                continue;
            }
            let rounded_depth = rounded_integer_sqrt(32_u32.pow(2) - doubled_offset.pow(2));
            let relief_eighths = ((32 - rounded_depth) * 4) as u8;
            pixels.push(rgba(FRONT_RGB, relief_eighths));
        }
    }
    pixels
}

fn block_model() -> Result<DepthSpriteModel, Box<dyn Error>> {
    let bounds = Bounds::new(16, 16, 16)?;
    let charts = [
        (CanonicalView::Front, [220, 70, 70]),
        (CanonicalView::Right, [70, 180, 90]),
        (CanonicalView::Top, [80, 120, 230]),
    ]
    .into_iter()
    .map(|(view, color)| {
        let (width, height) = view.dimensions(bounds);
        Chart::from_rgba(
            view,
            width,
            height,
            vec![rgba(color, 0); (width * height) as usize],
        )
    })
    .collect::<Result<Vec<_>, _>>()?;
    Ok(DepthSpriteModel::new(bounds, charts)?)
}

fn rgba(rgb: [u8; 3], relief_eighths: u8) -> [u8; 4] {
    [rgb[0], rgb[1], rgb[2], 255 - relief_eighths]
}
