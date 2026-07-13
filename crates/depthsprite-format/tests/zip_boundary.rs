use std::io::{Cursor, Write};

use depthsprite_format::{PackageError, load_reader};
use png::{BitDepth, ColorType, Encoder};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const LIMIT: u32 = 64 * 1024 * 1024;
const CENTRAL: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];
const EOCD: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
const ZIP64_EOCD: [u8; 4] = [0x50, 0x4b, 0x06, 0x06];
const ZIP64_LOCATOR: [u8; 4] = [0x50, 0x4b, 0x06, 0x07];

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

fn rgba() -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes, 1, 1);
    encoder.set_color(ColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&[1, 2, 3, 4]).unwrap();
    writer.finish().unwrap();
    bytes
}

fn valid_package() -> Vec<u8> {
    zip_bytes(vec![
        (
            "manifest.json",
            br#"{"format":"depthsprite","version":1,"bounds_pixels":[1,1,1],"views":["front"]}
"#
            .to_vec(),
        ),
        ("views/front.png", rgba()),
    ])
}

fn eocd(bytes: &[u8]) -> usize {
    bytes
        .windows(EOCD.len())
        .rposition(|window| window == EOCD)
        .unwrap()
}

fn central_start(bytes: &[u8]) -> usize {
    let end = eocd(bytes);
    u32::from_le_bytes(bytes[end + 16..end + 20].try_into().unwrap()) as usize
}

fn central_record(bytes: &[u8], expected_name: &str) -> usize {
    let mut offset = central_start(bytes);
    let end = eocd(bytes);
    while offset < end {
        assert_eq!(bytes[offset..offset + 4], CENTRAL);
        let name_len =
            u16::from_le_bytes(bytes[offset + 28..offset + 30].try_into().unwrap()) as usize;
        let extra_len =
            u16::from_le_bytes(bytes[offset + 30..offset + 32].try_into().unwrap()) as usize;
        let comment_len =
            u16::from_le_bytes(bytes[offset + 32..offset + 34].try_into().unwrap()) as usize;
        let name = &bytes[offset + 46..offset + 46 + name_len];
        if name == expected_name.as_bytes() {
            return offset;
        }
        offset += 46 + name_len + extra_len + comment_len;
    }
    panic!("missing central record {expected_name}");
}

fn patch_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn patch_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn insert_central_extra(bytes: &mut Vec<u8>, extra: &[u8]) {
    let start = central_start(bytes);
    assert_eq!(bytes[start..start + 4], CENTRAL);
    let name_len = u16::from_le_bytes(bytes[start + 28..start + 30].try_into().unwrap()) as usize;
    let old_extra = u16::from_le_bytes(bytes[start + 30..start + 32].try_into().unwrap()) as usize;
    let insert_at = start + 46 + name_len + old_extra;
    bytes.splice(insert_at..insert_at, extra.iter().copied());
    patch_u16(bytes, start + 30, (old_extra + extra.len()) as u16);
    let end = eocd(bytes);
    let old_size = u32::from_le_bytes(bytes[end + 12..end + 16].try_into().unwrap());
    patch_u32(bytes, end + 12, old_size + extra.len() as u32);
}

fn insert_central_comment(bytes: &mut Vec<u8>, comment: &[u8]) {
    insert_central_comment_at(bytes, central_start(bytes), comment);
}

fn insert_central_comment_at(bytes: &mut Vec<u8>, start: usize, comment: &[u8]) {
    let name_len = u16::from_le_bytes(bytes[start + 28..start + 30].try_into().unwrap()) as usize;
    let extra_len = u16::from_le_bytes(bytes[start + 30..start + 32].try_into().unwrap()) as usize;
    let old_comment =
        u16::from_le_bytes(bytes[start + 32..start + 34].try_into().unwrap()) as usize;
    let insert_at = start + 46 + name_len + extra_len + old_comment;
    bytes.splice(insert_at..insert_at, comment.iter().copied());
    patch_u16(bytes, start + 32, (old_comment + comment.len()) as u16);
    let end = eocd(bytes);
    let old_size = u32::from_le_bytes(bytes[end + 12..end + 16].try_into().unwrap());
    patch_u32(bytes, end + 12, old_size + comment.len() as u32);
}

fn append_eocd_comment(bytes: &mut Vec<u8>, comment: &[u8]) {
    assert!(comment.len() <= u16::MAX as usize);
    let end = eocd(bytes);
    patch_u16(bytes, end + 20, comment.len() as u16);
    bytes.extend_from_slice(comment);
}

fn truncate_first_central_record(bytes: &mut Vec<u8>, retained: usize) {
    let start = central_start(bytes);
    let end = eocd(bytes);
    assert!(retained < end - start);
    bytes.drain(start + retained..end);
    let new_end = eocd(bytes);
    patch_u32(bytes, new_end + 12, retained as u32);
}

fn assert_archive_contains(bytes: Vec<u8>, needle: &str) {
    match load_reader(Cursor::new(bytes)) {
        Err(PackageError::Archive(message)) => assert!(
            message.contains(needle),
            "expected archive error containing {needle:?}, got {message:?}"
        ),
        other => panic!("expected malformed archive error containing {needle:?}, got {other:?}"),
    }
}

#[test]
fn declared_total_limit_is_enforced_before_dependency_method_parsing() {
    let mut bytes = zip_bytes(vec![("manifest.json", b"not json".to_vec())]);
    let start = central_start(&bytes);
    patch_u16(&mut bytes, start + 10, 99);
    patch_u32(&mut bytes, start + 24, LIMIT + 1);

    assert!(matches!(
        load_reader(Cursor::new(bytes)),
        Err(PackageError::ExpandedSizeLimit { .. })
    ));
}

#[test]
fn multiple_structurally_valid_eocd_candidates_are_rejected() {
    let mut bytes = valid_package();
    let first_eocd = eocd(&bytes);
    let central = bytes[central_start(&bytes)..first_eocd].to_vec();
    let mut second_eocd = bytes[first_eocd..first_eocd + 22].to_vec();

    patch_u16(
        &mut bytes,
        first_eocd + 20,
        (central.len() + second_eocd.len()) as u16,
    );
    let second_central_start = bytes.len();
    bytes.extend_from_slice(&central);
    patch_u32(&mut second_eocd, 16, second_central_start as u32);
    bytes.extend_from_slice(&second_eocd);

    assert_archive_contains(bytes, "ambiguous");
}

#[test]
fn unsupported_zip64_candidate_still_makes_eocd_selection_ambiguous() {
    let mut bytes = valid_package();
    let first_eocd = eocd(&bytes);

    let mut second_source = valid_package();
    insert_central_extra(&mut second_source, &[0x01, 0x00, 0x00, 0x00]);
    let second_source_eocd = eocd(&second_source);
    let second_central = second_source[central_start(&second_source)..second_source_eocd].to_vec();
    let mut second_eocd = second_source[second_source_eocd..second_source_eocd + 22].to_vec();

    patch_u16(
        &mut bytes,
        first_eocd + 20,
        (second_central.len() + second_eocd.len()) as u16,
    );
    let second_central_start = bytes.len();
    bytes.extend_from_slice(&second_central);
    patch_u32(&mut second_eocd, 16, second_central_start as u32);
    bytes.extend_from_slice(&second_eocd);

    assert_archive_contains(bytes, "ambiguous");
}

#[test]
fn maximum_eocd_comment_with_invalid_embedded_candidate_remains_unambiguous() {
    let mut bytes = valid_package();
    let mut comment = vec![b'x'; u16::MAX as usize];
    let fake = 100;
    let fake_comment_len = (comment.len() - fake - 22) as u16;
    comment[fake..fake + 4].copy_from_slice(&EOCD);
    patch_u16(&mut comment, fake + 20, fake_comment_len);
    patch_u16(&mut comment, fake + 4, 1);
    append_eocd_comment(&mut bytes, &comment);

    assert!(load_reader(Cursor::new(bytes)).is_ok());
}

#[test]
fn zip64_central_extra_and_sentinel_forms_are_rejected_before_indexing() {
    let mut extra = valid_package();
    insert_central_extra(&mut extra, &[0x01, 0x00, 0x00, 0x00]);
    assert_archive_contains(extra, "ZIP64");

    for (offset, width) in [(20, 4), (24, 4), (34, 2), (42, 4)] {
        let mut sentinel = valid_package();
        let start = central_start(&sentinel);
        if width == 2 {
            patch_u16(&mut sentinel, start + offset, u16::MAX);
        } else {
            patch_u32(&mut sentinel, start + offset, u32::MAX);
        }
        assert_archive_contains(sentinel, "ZIP64");
    }

    let mut version_needed = valid_package();
    let start = central_start(&version_needed);
    patch_u16(&mut version_needed, start + 6, 45);
    assert_archive_contains(version_needed, "ZIP64");
}

#[test]
fn zip64_local_extra_and_sentinel_forms_are_rejected_before_indexing() {
    let mut extra = valid_package();
    let name_len = u16::from_le_bytes(extra[26..28].try_into().unwrap()) as usize;
    patch_u16(&mut extra, 28, 4);
    let extra_start = 30 + name_len;
    extra[extra_start..extra_start + 4].copy_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    assert_archive_contains(extra, "ZIP64");

    let mut sentinel = valid_package();
    patch_u32(&mut sentinel, 18, u32::MAX);
    assert_archive_contains(sentinel, "ZIP64");

    let mut version = valid_package();
    patch_u16(&mut version, 4, 45);
    assert_archive_contains(version, "ZIP64");
}

#[test]
fn associated_zip64_locator_and_eocd_are_rejected_explicitly() {
    let mut locator = valid_package();
    let end = eocd(&locator);
    let mut record = vec![0_u8; 20];
    record[..4].copy_from_slice(&ZIP64_LOCATOR);
    locator.splice(end..end, record);
    assert_archive_contains(locator, "ZIP64");

    let mut structures = valid_package();
    let end = eocd(&structures);
    let mut record = vec![0_u8; 56 + 20];
    record[..4].copy_from_slice(&ZIP64_EOCD);
    record[56..60].copy_from_slice(&ZIP64_LOCATOR);
    structures.splice(end..end, record);
    assert_archive_contains(structures, "ZIP64");
}

#[test]
fn locator_signature_inside_conventional_central_comment_is_not_associated_zip64() {
    let mut bytes = valid_package();
    let mut comment = [0_u8; 20];
    comment[..4].copy_from_slice(&ZIP64_LOCATOR);
    let last_record = central_record(&bytes, "views/front.png");
    insert_central_comment_at(&mut bytes, last_record, &comment);

    assert!(load_reader(Cursor::new(bytes)).is_ok());
}

#[test]
fn truncated_central_fixed_name_extra_and_comment_fields_are_malformed() {
    let mut fixed = valid_package();
    truncate_first_central_record(&mut fixed, 40);
    assert_archive_contains(fixed, "central");

    let mut name = valid_package();
    truncate_first_central_record(&mut name, 46 + 5);
    assert_archive_contains(name, "central");

    let mut extra = valid_package();
    insert_central_extra(&mut extra, &[0xfe, 0xca, 4, 0, 1, 2, 3, 4]);
    let start = central_start(&extra);
    let name_len = u16::from_le_bytes(extra[start + 28..start + 30].try_into().unwrap()) as usize;
    truncate_first_central_record(&mut extra, 46 + name_len + 6);
    assert_archive_contains(extra, "central");

    let mut comment = valid_package();
    insert_central_comment(&mut comment, b"0123456789");
    let start = central_start(&comment);
    let name_len = u16::from_le_bytes(comment[start + 28..start + 30].try_into().unwrap()) as usize;
    let extra_len =
        u16::from_le_bytes(comment[start + 30..start + 32].try_into().unwrap()) as usize;
    truncate_first_central_record(&mut comment, 46 + name_len + extra_len + 8);
    assert_archive_contains(comment, "central");
}

#[test]
fn central_extra_records_use_checked_lengths() {
    let mut truncated_header = valid_package();
    insert_central_extra(&mut truncated_header, &[0xfe, 0xca, 0]);
    assert_archive_contains(truncated_header, "extra");

    let mut overflowing_payload = valid_package();
    insert_central_extra(&mut overflowing_payload, &[0xfe, 0xca, 0xff, 0xff]);
    assert_archive_contains(overflowing_payload, "extra");
}

#[test]
fn maximum_central_extra_and_comment_lengths_are_bounded_and_accepted() {
    let mut bytes = valid_package();
    let mut extra = Vec::with_capacity(u16::MAX as usize);
    extra.extend_from_slice(&0xcafe_u16.to_le_bytes());
    extra.extend_from_slice(&(u16::MAX - 4).to_le_bytes());
    extra.resize(u16::MAX as usize, 7);
    insert_central_extra(&mut bytes, &extra);
    insert_central_comment(&mut bytes, &vec![b'c'; u16::MAX as usize]);

    assert!(load_reader(Cursor::new(bytes)).is_ok());
}

#[test]
fn unsafe_cross_platform_names_precede_missing_manifest_and_content_decode() {
    for unsafe_name in [
        "manifest.json\0evil",
        "manifest.json\u{1f}",
        "views/front.png.",
        "views/front.png ",
        "views:front.png",
        "véws/front.png",
    ] {
        let bytes = zip_bytes(vec![(unsafe_name, b"not content".to_vec())]);
        assert!(
            matches!(
                load_reader(Cursor::new(bytes)),
                Err(PackageError::UnsafeEntry(name)) if name == unsafe_name
            ),
            "unsafe name was not classified first: {unsafe_name:?}"
        );
    }
}

#[test]
fn exact_entry_allowlist_precedes_corrupt_unknown_content() {
    let mut bytes = zip_bytes(vec![
        (
            "manifest.json",
            br#"{"format":"depthsprite","version":1,"bounds_pixels":[1,1,1],"views":["front"]}
"#
            .to_vec(),
        ),
        ("views/front.png", rgba()),
        ("cache.bin", b"x".to_vec()),
    ]);
    let cache_record = central_record(&bytes, "cache.bin");
    patch_u32(&mut bytes, cache_record + 16, 0);
    let result = load_reader(Cursor::new(bytes));
    assert!(
        matches!(result, Err(PackageError::UndeclaredEntry(ref name)) if name == "cache.bin"),
        "unexpected result: {result:?}"
    );
}

#[test]
fn maximum_legal_name_is_bounded_then_rejected_by_exact_allowlist() {
    let long_name = "a".repeat(u16::MAX as usize);
    let bytes = zip_bytes(vec![(long_name.as_str(), b"not content".to_vec())]);
    assert!(matches!(
        load_reader(Cursor::new(bytes)),
        Err(PackageError::UndeclaredEntry(name)) if name == long_name
    ));
}
