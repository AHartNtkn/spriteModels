use std::{fs::File, path::Path};

use depthsprite_format::load_rgba_png;
use png::{BitDepth, ColorType, Encoder};
use tempfile::tempdir;

fn write_png(
    path: &Path,
    width: u32,
    height: u32,
    color_type: ColorType,
    pixels: &[u8],
    palette: Option<&[u8]>,
    transparency: Option<&[u8]>,
) {
    let mut encoder = Encoder::new(File::create(path).unwrap(), width, height);
    encoder.set_color(color_type);
    encoder.set_depth(BitDepth::Eight);
    if let Some(palette) = palette {
        encoder.set_palette(palette.to_vec());
    }
    if let Some(transparency) = transparency {
        encoder.set_trns(transparency.to_vec());
    }
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(pixels).unwrap();
    writer.finish().unwrap();
}

#[test]
fn rgba_source_png_preserves_every_channel_without_interpreting_alpha() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("rgba.png");
    write_png(
        &path,
        2,
        1,
        ColorType::Rgba,
        &[17, 31, 47, 0, 99, 88, 77, 193],
        None,
        None,
    );

    let image = load_rgba_png(&path).unwrap();

    assert_eq!((image.width, image.height), (2, 1));
    assert_eq!(image.pixels, [[17, 31, 47, 0], [99, 88, 77, 193]]);
}

#[test]
fn rgb_source_png_normalizes_to_opaque_rgba() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("rgb.png");
    write_png(&path, 2, 1, ColorType::Rgb, &[1, 2, 3, 4, 5, 6], None, None);

    let image = load_rgba_png(&path).unwrap();

    assert_eq!((image.width, image.height), (2, 1));
    assert_eq!(image.pixels, [[1, 2, 3, 255], [4, 5, 6, 255]]);
}

#[test]
fn indexed_source_png_expands_palette_and_transparency_to_rgba() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("indexed.png");
    write_png(
        &path,
        3,
        1,
        ColorType::Indexed,
        &[0, 1, 2],
        Some(&[10, 20, 30, 40, 50, 60, 70, 80, 90]),
        Some(&[0, 127, 255]),
    );

    let image = load_rgba_png(&path).unwrap();

    assert_eq!((image.width, image.height), (3, 1));
    assert_eq!(
        image.pixels,
        [[10, 20, 30, 0], [40, 50, 60, 127], [70, 80, 90, 255]]
    );
}
