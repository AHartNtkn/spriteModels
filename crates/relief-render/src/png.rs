use png::{BitDepth, ColorType, Compression, Encoder, EncodingError, Filter};

use crate::FrameBuffer;

pub fn encode_png(frame: &FrameBuffer) -> Result<Vec<u8>, EncodingError> {
    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes, frame.width(), frame.height());
    encoder.set_color(ColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);
    encoder.set_compression(Compression::High);
    encoder.set_filter(Filter::NoFilter);
    let mut writer = encoder.write_header()?;
    let rgba: Vec<u8> = frame
        .pixels()
        .iter()
        .flat_map(|pixel| pixel.iter().copied())
        .collect();
    writer.write_image_data(&rgba)?;
    writer.finish()?;
    Ok(bytes)
}
