use std::error::Error;

use relief_core::{AuthoredModel, Bounds, CanonicalView, Chart};

use crate::pixel::{rgba, shade};

const BASE_RGB: [u8; 3] = [156, 132, 98];
const FACES: [(CanonicalView, i32); 6] = [
    (CanonicalView::Top, 36),
    (CanonicalView::Front, 22),
    (CanonicalView::Left, 12),
    (CanonicalView::Right, -8),
    (CanonicalView::Back, -18),
    (CanonicalView::Bottom, -30),
];

pub fn block_model() -> Result<AuthoredModel, Box<dyn Error>> {
    let bounds = Bounds::new(16, 16, 16)?;
    let charts = FACES
        .into_iter()
        .map(|(view, face_light)| {
            let (width, height) = view.dimensions(bounds);
            let mut pixels = Vec::with_capacity((width * height) as usize);
            for y in 0..height {
                for x in 0..width {
                    let gradient = ((width - x + height - y) / 4) as i32;
                    let pixel = rgba(shade(BASE_RGB, face_light + gradient, 0), 0);
                    debug_assert_eq!(pixel[3], 255);
                    pixels.push(pixel);
                }
            }
            Chart::from_rgba(view, width, height, pixels)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(AuthoredModel::new(bounds, charts)?)
}
