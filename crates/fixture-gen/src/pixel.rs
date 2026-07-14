use std::{error::Error, path::Path};

use depthsprite_format::save_path_atomic;
use relief_core::AuthoredModel;

pub(crate) fn rgba(rgb: [u8; 3], relief: u8) -> [u8; 4] {
    [rgb[0], rgb[1], rgb[2], 255 - relief]
}

pub(crate) fn integer_sqrt(value: u32) -> u32 {
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

pub(crate) fn shade(base: [u8; 3], light: i32, relief: u8) -> [u8; 3] {
    std::array::from_fn(|channel| {
        (i32::from(base[channel]) + light - i32::from(relief) / 8).clamp(0, 255) as u8
    })
}

pub(crate) fn mask_boundary(x: i32, y: i32, foreground: impl Fn(i32, i32) -> bool) -> bool {
    foreground(x, y)
        && (-1..=1).any(|dy| (-1..=1).any(|dx| (dx != 0 || dy != 0) && !foreground(x + dx, y + dy)))
}

pub(crate) fn save_package(
    output: &Path,
    name: &str,
    model: &AuthoredModel,
) -> Result<(), Box<dyn Error>> {
    save_path_atomic(model, output.join(name))?;
    Ok(())
}
