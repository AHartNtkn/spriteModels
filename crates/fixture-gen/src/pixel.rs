use std::{error::Error, path::Path};

use depthsprite_format::save_path_atomic;
use relief_core::{AuthoredModel, CanonicalView};

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
    let exposure = light - i32::from(relief) / 6;
    let cartoon_offset = (3 * exposure).clamp(-144, 144);
    std::array::from_fn(|channel| (i32::from(base[channel]) + cartoon_offset).clamp(0, 255) as u8)
}

pub(crate) fn directional_light(
    view: CanonicalView,
    width: i32,
    height: i32,
    x: i32,
    y: i32,
) -> i32 {
    let horizontal_sign = match view {
        CanonicalView::Back | CanonicalView::Right => -1,
        _ => 1,
    };
    let vertical_sign = match view {
        CanonicalView::Bottom => -1,
        _ => 1,
    };
    let horizontal = horizontal_sign * (width - 1 - 2 * x);
    let vertical = vertical_sign * (height - 1 - 2 * y);
    36 * (horizontal + vertical) / (width + height - 2).max(1)
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
