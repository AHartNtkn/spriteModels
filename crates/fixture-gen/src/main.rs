use std::{
    error::Error,
    ffi::OsString,
    path::{Path, PathBuf},
};

use depthsprite_format::{DepthSpriteModel, save_path_atomic};
use relief_core::{Bounds, CanonicalView, Chart};

const TOP_RGB: [u8; 3] = [216, 156, 85];
const FRONT_RGB: [u8; 3] = [144, 76, 52];

fn main() -> Result<(), Box<dyn Error>> {
    let output = output_directory(std::env::args_os())?;
    std::fs::create_dir_all(&output)?;
    save_path_atomic(&bowl_model()?, output.join("bowl.depthsprite"))?;
    save_path_atomic(&block_model()?, output.join("block.depthsprite"))?;
    Ok(())
}

fn output_directory(mut arguments: impl Iterator<Item = OsString>) -> Result<PathBuf, String> {
    let program = arguments
        .next()
        .unwrap_or_else(|| OsString::from("fixture-gen"));
    let Some(output) = arguments.next() else {
        return Err(format!(
            "usage: {} <output-directory>",
            Path::new(&program).display()
        ));
    };
    if arguments.next().is_some() {
        return Err(format!(
            "usage: {} <output-directory>",
            Path::new(&program).display()
        ));
    }
    Ok(PathBuf::from(output))
}

fn bowl_model() -> Result<DepthSpriteModel, Box<dyn Error>> {
    let bounds = Bounds::new(32, 16, 32)?;
    let front = Chart::from_rgba(bounds, CanonicalView::Front, 32, 16, bowl_front_pixels())?;
    let top = Chart::from_rgba(bounds, CanonicalView::Top, 32, 32, bowl_top_pixels())?;
    Ok(DepthSpriteModel::new(bounds, vec![front, top])?)
}

fn bowl_top_pixels() -> Vec<[u8; 4]> {
    const OUTER_RADIUS_DOUBLED: i32 = 28;
    const ZERO_RIM_START_SQUARED: u64 = 770;
    const CENTER_DISTANCE_SQUARED: u64 = 2;
    let mut pixels = Vec::with_capacity(32 * 32);
    for y in 0..32_i32 {
        for x in 0..32_i32 {
            let dx = 2 * x + 1 - 32;
            let dy = 2 * y + 1 - 32;
            let distance_squared = dx * dx + dy * dy;
            if distance_squared > OUTER_RADIUS_DOUBLED * OUTER_RADIUS_DOUBLED {
                pixels.push([0, 0, 0, 0]);
                continue;
            }
            let distance_squared = distance_squared as u64;
            let relief = if distance_squared >= ZERO_RIM_START_SQUARED {
                0
            } else {
                let rim_fourth = ZERO_RIM_START_SQUARED.pow(2);
                let numerator = 64 * (rim_fourth - distance_squared.pow(2));
                let denominator = rim_fourth - CENTER_DISTANCE_SQUARED.pow(2);
                ((numerator + denominator / 2) / denominator).clamp(0, 64) as u8
            };
            pixels.push(rgba(TOP_RGB, relief));
        }
    }
    pixels
}

fn bowl_front_pixels() -> Vec<[u8; 4]> {
    let mut pixels = Vec::with_capacity(32 * 16);
    for y in 0..16_u32 {
        for x in 0..32_u32 {
            let doubled_offset = (2 * x as i32 + 1 - 32).unsigned_abs();
            let curve = (31_u32.pow(2) - doubled_offset.pow(2)) * 10 / 31_u32.pow(2);
            let bottom = 4 + curve;
            if !(2..=bottom).contains(&y) {
                pixels.push([0, 0, 0, 0]);
                continue;
            }
            let edge_distance = x.min(31 - x);
            let relief_eighths = 256_u32
                .saturating_sub(edge_distance.saturating_mul(4))
                .min(254) as u8;
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
            bounds,
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
