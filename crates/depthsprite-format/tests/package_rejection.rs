use std::io::{Cursor, Write};

use depthsprite_format::{PackageError, load_reader};
use png::{BitDepth, ColorType, Encoder};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const LIMIT: usize = 64 * 1024 * 1024;

fn zip_bytes(entries: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(&mut bytes);
    for (name, contents) in entries {
        zip.start_file(
            name,
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(&contents).unwrap();
    }
    zip.finish().unwrap();
    bytes.into_inner()
}

fn manifest(bounds: [u32; 3], views_json: &str) -> Vec<u8> {
    format!(
        "{{\"format\":\"depthsprite\",\"version\":1,\"bounds_pixels\":[{},{},{}],\"views\":{views_json}}}\n",
        bounds[0], bounds[1], bounds[2]
    )
    .into_bytes()
}

fn png(color: ColorType, depth: BitDepth, width: u32, height: u32, data: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes, width, height);
    encoder.set_color(color);
    encoder.set_depth(depth);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(data).unwrap();
    writer.finish().unwrap();
    bytes
}

fn rgba(width: u32, height: u32) -> Vec<u8> {
    png(
        ColorType::Rgba,
        BitDepth::Eight,
        width,
        height,
        &vec![255; width as usize * height as usize * 4],
    )
}

fn palette_png() -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes, 1, 1);
    encoder.set_color(ColorType::Indexed);
    encoder.set_depth(BitDepth::Eight);
    encoder.set_palette(vec![1, 2, 3]);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&[0]).unwrap();
    writer.finish().unwrap();
    bytes
}

fn patch_central_uncompressed_size(bytes: &mut [u8], size: u32) {
    let signature = [0x50, 0x4b, 0x01, 0x02];
    let index = bytes
        .windows(signature.len())
        .position(|window| window == signature)
        .unwrap();
    bytes[index + 24..index + 28].copy_from_slice(&size.to_le_bytes());
}

fn patch_eocd_entry_count(bytes: &mut [u8], count: u16) {
    let signature = [0x50, 0x4b, 0x05, 0x06];
    let index = bytes
        .windows(signature.len())
        .rposition(|window| window == signature)
        .unwrap();
    bytes[index + 8..index + 10].copy_from_slice(&count.to_le_bytes());
    bytes[index + 10..index + 12].copy_from_slice(&count.to_le_bytes());
}

fn replace_all_equal_length(bytes: &mut [u8], from: &[u8], to: &[u8]) {
    assert_eq!(from.len(), to.len());
    let mut offset = 0;
    while let Some(index) = bytes[offset..]
        .windows(from.len())
        .position(|window| window == from)
    {
        let start = offset + index;
        bytes[start..start + to.len()].copy_from_slice(to);
        offset = start + to.len();
    }
}

#[test]
fn unsafe_entry_names_are_rejected_before_png_decode() {
    for unsafe_name in [
        "../front.png",
        "/views/front.png",
        "views\\front.png",
        "C:/views/front.png",
        "views/./front.png",
        "views//front.png",
    ] {
        let bytes = zip_bytes(vec![(unsafe_name, b"not a png".to_vec())]);
        assert!(
            matches!(
                load_reader(Cursor::new(bytes)),
                Err(PackageError::UnsafeEntry(name)) if name == unsafe_name
            ),
            "unsafe name was not classified before content decode: {unsafe_name}"
        );
    }
}

#[test]
fn entry_count_limit_is_checked_before_png_decode() {
    let entries = vec![
        ("manifest.json", b"not json".to_vec()),
        ("views/front.png", b"not png".to_vec()),
        ("views/back.png", b"not png".to_vec()),
        ("views/left.png", b"not png".to_vec()),
        ("views/right.png", b"not png".to_vec()),
        ("views/top.png", b"not png".to_vec()),
        ("views/bottom.png", b"not png".to_vec()),
        ("other.bin", b"still not png".to_vec()),
    ];
    assert!(matches!(
        load_reader(Cursor::new(zip_bytes(entries))),
        Err(PackageError::EntryLimit { .. })
    ));
}

#[test]
fn declared_entry_count_limit_is_checked_before_archive_indexing() {
    let mut archive = zip_bytes(vec![("manifest.json", b"not json".to_vec())]);
    patch_eocd_entry_count(&mut archive, 65_534);
    assert!(matches!(
        load_reader(Cursor::new(archive)),
        Err(PackageError::EntryLimit {
            actual: 65_534,
            limit: 7
        })
    ));
}

#[test]
fn duplicate_missing_and_undeclared_entries_are_rejected_structurally() {
    let mut duplicate = zip_bytes(vec![
        ("manifest.json", manifest([1, 1, 1], r#"["front"]"#)),
        ("views/front.png", rgba(1, 1)),
        ("views/right.png", b"not a png".to_vec()),
    ]);
    replace_all_equal_length(&mut duplicate, b"views/right.png", b"views/front.png");
    assert!(matches!(
        load_reader(Cursor::new(duplicate)),
        Err(PackageError::DuplicateEntry(name)) if name == "views/front.png"
    ));

    let missing = zip_bytes(vec![("manifest.json", manifest([1, 1, 1], r#"["front"]"#))]);
    assert!(matches!(
        load_reader(Cursor::new(missing)),
        Err(PackageError::MissingEntry(name)) if name == "views/front.png"
    ));

    let undeclared = zip_bytes(vec![
        ("manifest.json", manifest([1, 1, 1], r#"["front"]"#)),
        ("views/front.png", rgba(1, 1)),
        ("views/top.png", b"not a png".to_vec()),
    ]);
    assert!(matches!(
        load_reader(Cursor::new(undeclared)),
        Err(PackageError::UndeclaredEntry(name)) if name == "views/top.png"
    ));

    let unknown = zip_bytes(vec![
        ("manifest.json", manifest([1, 1, 1], r#"["front"]"#)),
        ("views/front.png", rgba(1, 1)),
        ("cache.bin", b"renderer cache".to_vec()),
    ]);
    assert!(matches!(
        load_reader(Cursor::new(unknown)),
        Err(PackageError::UndeclaredEntry(name)) if name == "cache.bin"
    ));
}

#[test]
fn manifest_schema_format_version_and_views_are_strict() {
    let cases = [
        (
            br#"{"format":"depthsprite","version":1,"bounds_pixels":[1,1,1],"views":["front"],"cache":true}"#.to_vec(),
            "schema",
        ),
        (
            br#"{"format":"sprite","version":1,"bounds_pixels":[1,1,1],"views":["front"]}"#.to_vec(),
            "format",
        ),
        (
            br#"{"format":"depthsprite","version":2,"bounds_pixels":[1,1,1],"views":["front"]}"#.to_vec(),
            "version",
        ),
        (
            br#"{"format":"depthsprite","version":1,"bounds_pixels":[1,1,1],"views":["Front"]}"#.to_vec(),
            "schema",
        ),
    ];
    for (manifest_bytes, expected) in cases {
        let archive = zip_bytes(vec![
            ("manifest.json", manifest_bytes),
            ("views/front.png", rgba(1, 1)),
        ]);
        let error = load_reader(Cursor::new(archive)).unwrap_err();
        match expected {
            "format" => assert!(matches!(error, PackageError::WrongFormat(_))),
            "version" => assert!(matches!(error, PackageError::UnsupportedVersion(2))),
            _ => assert!(matches!(error, PackageError::Manifest(_))),
        }
    }

    let duplicate_views = zip_bytes(vec![
        ("manifest.json", manifest([1, 1, 1], r#"["front","front"]"#)),
        ("views/front.png", rgba(1, 1)),
    ]);
    assert!(matches!(
        load_reader(Cursor::new(duplicate_views)),
        Err(PackageError::DuplicateView(_))
    ));

    let empty_views = zip_bytes(vec![("manifest.json", manifest([1, 1, 1], "[]"))]);
    assert!(matches!(
        load_reader(Cursor::new(empty_views)),
        Err(PackageError::ViewCount(0))
    ));
}

#[test]
fn bounds_and_chart_dimensions_are_validated() {
    for bounds in [[0, 1, 1], [1, 513, 1], [1, 1, 513]] {
        let archive = zip_bytes(vec![
            ("manifest.json", manifest(bounds, r#"["front"]"#)),
            ("views/front.png", b"not a png".to_vec()),
        ]);
        assert!(matches!(
            load_reader(Cursor::new(archive)),
            Err(PackageError::InvalidBounds(actual)) if actual == bounds
        ));
    }

    let wrong_dimensions = zip_bytes(vec![
        ("manifest.json", manifest([2, 1, 1], r#"["front"]"#)),
        ("views/front.png", rgba(1, 1)),
    ]);
    assert!(matches!(
        load_reader(Cursor::new(wrong_dimensions)),
        Err(PackageError::InvalidChart { .. })
    ));

    let mixed_dimensions = zip_bytes(vec![
        ("manifest.json", manifest([2, 1, 2], r#"["front","top"]"#)),
        ("views/front.png", rgba(2, 1)),
        ("views/top.png", rgba(2, 1)),
    ]);
    assert!(matches!(
        load_reader(Cursor::new(mixed_dimensions)),
        Err(PackageError::InvalidChart { .. })
    ));
}

#[test]
fn encoded_png_must_be_exact_rgba8_without_conversion() {
    let rgb = zip_bytes(vec![
        ("manifest.json", manifest([1, 1, 1], r#"["front"]"#)),
        (
            "views/front.png",
            png(ColorType::Rgb, BitDepth::Eight, 1, 1, &[1, 2, 3]),
        ),
    ]);
    assert!(matches!(
        load_reader(Cursor::new(rgb)),
        Err(PackageError::InvalidPngType { .. })
    ));

    for wrong_color in [
        png(ColorType::Grayscale, BitDepth::Eight, 1, 1, &[7]),
        palette_png(),
    ] {
        let archive = zip_bytes(vec![
            ("manifest.json", manifest([1, 1, 1], r#"["front"]"#)),
            ("views/front.png", wrong_color),
        ]);
        assert!(matches!(
            load_reader(Cursor::new(archive)),
            Err(PackageError::InvalidPngType { .. })
        ));
    }

    let rgba16 = zip_bytes(vec![
        ("manifest.json", manifest([1, 1, 1], r#"["front"]"#)),
        (
            "views/front.png",
            png(
                ColorType::Rgba,
                BitDepth::Sixteen,
                1,
                1,
                &[0, 1, 0, 2, 0, 3, 0, 4],
            ),
        ),
    ]);
    assert!(matches!(
        load_reader(Cursor::new(rgba16)),
        Err(PackageError::InvalidPngType { .. })
    ));
}

#[test]
fn declared_expanded_size_limit_precedes_content_decode() {
    let mut archive = zip_bytes(vec![("manifest.json", b"not json".to_vec())]);
    patch_central_uncompressed_size(&mut archive, LIMIT as u32 + 1);
    assert!(matches!(
        load_reader(Cursor::new(archive)),
        Err(PackageError::ExpandedSizeLimit { .. })
    ));
}

#[test]
fn bounded_actual_reads_defeat_misleading_expanded_size_headers() {
    let mut archive = zip_bytes(vec![("manifest.json", vec![b' '; LIMIT + 1])]);
    patch_central_uncompressed_size(&mut archive, 1);
    assert!(matches!(
        load_reader(Cursor::new(archive)),
        Err(PackageError::ExpandedSizeLimit { .. })
    ));
}
