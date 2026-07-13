#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodedTexel {
    Background,
    Relief { rgb: [u8; 3], eighths: u8 },
}

pub fn decode_rgba([r, g, b, a]: [u8; 4]) -> DecodedTexel {
    if a == 0 {
        DecodedTexel::Background
    } else {
        DecodedTexel::Relief {
            rgb: [r, g, b],
            eighths: 255 - a,
        }
    }
}
