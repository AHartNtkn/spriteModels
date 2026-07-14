use std::{fs::File, io::BufReader, path::Path};

use png::{BitDepth, ColorType, Decoder, Transformations};

use crate::PackageError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[u8; 4]>,
}

pub fn load_rgba_png(path: impl AsRef<Path>) -> Result<RgbaImage, PackageError> {
    let path = path.as_ref();
    let entry = path.display().to_string();
    let mut decoder = Decoder::new(BufReader::new(File::open(path)?));
    decoder.set_transformations(Transformations::normalize_to_color8() | Transformations::ALPHA);
    let mut reader = decoder
        .read_info()
        .map_err(|error| PackageError::InvalidPng {
            entry: entry.clone(),
            message: error.to_string(),
        })?;
    let buffer_size = reader
        .output_buffer_size()
        .ok_or_else(|| PackageError::InvalidPng {
            entry: entry.clone(),
            message: "decoded image is too large".to_owned(),
        })?;
    let mut buffer = vec![0; buffer_size];
    let output = reader
        .next_frame(&mut buffer)
        .map_err(|error| PackageError::InvalidPng {
            entry,
            message: error.to_string(),
        })?;
    debug_assert_eq!(output.bit_depth, BitDepth::Eight);

    let pixels = match output.color_type {
        ColorType::Rgba => buffer[..output.buffer_size()]
            .chunks_exact(4)
            .map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]])
            .collect(),
        ColorType::GrayscaleAlpha => buffer[..output.buffer_size()]
            .chunks_exact(2)
            .map(|pixel| [pixel[0], pixel[0], pixel[0], pixel[1]])
            .collect(),
        color_type => {
            return Err(PackageError::InvalidPng {
                entry: path.display().to_string(),
                message: format!("could not normalize {color_type:?} PNG to RGBA8"),
            });
        }
    };

    Ok(RgbaImage {
        width: output.width,
        height: output.height,
        pixels,
    })
}
