use std::{
    cell::Cell,
    io::{self, Cursor, Read, Seek, SeekFrom, Write},
    rc::Rc,
};

use depthsprite_format::{PackageError, load_reader};
use png::{BitDepth, ColorType, Encoder};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const LIMIT: u32 = 64 * 1024 * 1024;
const CENTRAL: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];
const EOCD: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
const ZIP64_EOCD: [u8; 4] = [0x50, 0x4b, 0x06, 0x06];
const ZIP64_LOCATOR: [u8; 4] = [0x50, 0x4b, 0x06, 0x07];

fn zip_bytes(entries: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    zip_bytes_with_method(entries, CompressionMethod::Deflated)
}

fn zip_bytes_with_method(entries: Vec<(&str, Vec<u8>)>, method: CompressionMethod) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(&mut bytes);
    for (name, contents) in entries {
        zip.start_file(
            name,
            SimpleFileOptions::default().compression_method(method),
        )
        .unwrap();
        zip.write_all(&contents).unwrap();
    }
    zip.finish().unwrap();
    bytes.into_inner()
}

fn stream_zip_bytes(entries: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut zip = ZipWriter::new_stream(&mut bytes);
        for (name, contents) in entries {
            zip.start_file(
                name,
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
            zip.write_all(&contents).unwrap();
        }
        zip.finish().unwrap();
    }
    bytes
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
    package_with_method_and_order(CompressionMethod::Deflated, false)
}

fn package_with_method_and_order(method: CompressionMethod, reversed: bool) -> Vec<u8> {
    let mut entries = vec![
        (
            "manifest.json",
            br#"{"format":"depthsprite","version":1,"bounds_pixels":[1,1,1],"views":["front"]}
"#
            .to_vec(),
        ),
        ("views/front.png", rgba()),
    ];
    if reversed {
        entries.reverse();
    }
    zip_bytes_with_method(entries, method)
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

fn insert_associated_zip64_records(bytes: &mut Vec<u8>) -> usize {
    let classic_eocd = eocd(bytes);
    let entry_count = u16::from_le_bytes(
        bytes[classic_eocd + 10..classic_eocd + 12]
            .try_into()
            .unwrap(),
    ) as u64;
    let central_size = u32::from_le_bytes(
        bytes[classic_eocd + 12..classic_eocd + 16]
            .try_into()
            .unwrap(),
    ) as u64;
    let central_offset = u32::from_le_bytes(
        bytes[classic_eocd + 16..classic_eocd + 20]
            .try_into()
            .unwrap(),
    ) as u64;

    let mut records = vec![0_u8; 56 + 20];
    records[..4].copy_from_slice(&ZIP64_EOCD);
    records[4..12].copy_from_slice(&44_u64.to_le_bytes());
    records[12..14].copy_from_slice(&45_u16.to_le_bytes());
    records[14..16].copy_from_slice(&45_u16.to_le_bytes());
    records[24..32].copy_from_slice(&entry_count.to_le_bytes());
    records[32..40].copy_from_slice(&entry_count.to_le_bytes());
    records[40..48].copy_from_slice(&central_size.to_le_bytes());
    records[48..56].copy_from_slice(&central_offset.to_le_bytes());
    records[56..60].copy_from_slice(&ZIP64_LOCATOR);
    records[64..72].copy_from_slice(&(classic_eocd as u64).to_le_bytes());
    records[72..76].copy_from_slice(&1_u32.to_le_bytes());
    bytes.splice(classic_eocd..classic_eocd, records);
    classic_eocd + 76
}

struct SparseReader {
    len: u64,
    position: u64,
    bytes_read: Rc<Cell<u64>>,
}

struct SignatureReader {
    len: u64,
    position: u64,
    bytes_read: Rc<Cell<u64>>,
    seeks: Rc<Cell<u64>>,
}

impl Read for SignatureReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let available = self.len.saturating_sub(self.position);
        let count = available.min(buffer.len() as u64) as usize;
        let signature = EOCD;
        for (index, byte) in buffer[..count].iter_mut().enumerate() {
            *byte = signature[((self.position as usize) + index) % signature.len()];
        }
        self.position += count as u64;
        self.bytes_read.set(self.bytes_read.get() + count as u64);
        Ok(count)
    }
}

impl Seek for SignatureReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.seeks.set(self.seeks.get() + 1);
        let next = match position {
            SeekFrom::Start(value) => value as i128,
            SeekFrom::End(value) => self.len as i128 + value as i128,
            SeekFrom::Current(value) => self.position as i128 + value as i128,
        };
        if !(0..=u64::MAX as i128).contains(&next) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid seek"));
        }
        self.position = next as u64;
        Ok(self.position)
    }
}

impl Read for SparseReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let available = self.len.saturating_sub(self.position);
        let count = available.min(buffer.len() as u64) as usize;
        buffer[..count].fill(0);
        self.position += count as u64;
        self.bytes_read.set(self.bytes_read.get() + count as u64);
        Ok(count)
    }
}

impl Seek for SparseReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let next = match position {
            SeekFrom::Start(value) => value as i128,
            SeekFrom::End(value) => self.len as i128 + value as i128,
            SeekFrom::Current(value) => self.position as i128 + value as i128,
        };
        if !(0..=u64::MAX as i128).contains(&next) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid seek"));
        }
        self.position = next as u64;
        Ok(self.position)
    }
}

fn assert_input_limit<R: Read + Seek>(reader: R) {
    let error = load_reader(reader).expect_err("oversized archive input must be rejected");
    assert!(
        error.to_string().contains("archive input data is"),
        "expected archive input limit error, got {error:?}"
    );
}

fn assert_canonical_envelope_rejection(bytes: Vec<u8>) {
    match load_reader(Cursor::new(bytes)) {
        Err(PackageError::Archive(message)) => assert!(
            message.contains("canonical ZIP32 envelope"),
            "unexpected archive error: {message:?}"
        ),
        other => panic!("expected canonical ZIP32 envelope rejection, got {other:?}"),
    }
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

fn assert_explicit_zip64_rejection(bytes: Vec<u8>) {
    match load_reader(Cursor::new(bytes)) {
        Err(PackageError::Archive(message)) => assert_eq!(
            message,
            "ZIP64 structures are not supported by the version 1 package profile"
        ),
        other => panic!("expected explicit ZIP64 rejection, got {other:?}"),
    }
}

#[test]
fn appended_duplicate_eocd_is_rejected() {
    let mut bytes = valid_package();
    let end = eocd(&bytes);
    let duplicate = bytes[end..end + 22].to_vec();
    bytes.extend_from_slice(&duplicate);
    assert_canonical_envelope_rejection(bytes);
}

#[test]
fn structurally_valid_eight_entry_archive_fails_the_entry_limit() {
    let names: Vec<String> = (0..8).map(|index| format!("entry-{index}")).collect();
    let entries = names
        .iter()
        .map(|name| (name.as_str(), Vec::new()))
        .collect();

    assert!(matches!(
        load_reader(Cursor::new(zip_bytes(entries))),
        Err(PackageError::EntryLimit {
            actual: 8,
            limit: 7
        })
    ));
}

#[test]
fn structurally_valid_multidisk_eocd_fails_after_qualification() {
    let mut bytes = valid_package();
    let end = eocd(&bytes);
    patch_u16(&mut bytes, end + 4, 1);
    patch_u16(&mut bytes, end + 6, 1);

    assert_archive_contains(bytes, "multi-disk");
}

#[test]
fn declared_compressed_bytes_over_the_input_limit_fail_in_preflight() {
    let mut bytes = valid_package();
    let start = central_start(&bytes);
    patch_u32(&mut bytes, start + 20, LIMIT + 1);
    patch_u32(&mut bytes, 18, LIMIT + 1);

    assert_input_limit(Cursor::new(bytes));
}

#[test]
fn sparse_oversized_archive_is_rejected_without_reading_input() {
    let bytes_read = Rc::new(Cell::new(0));
    let reader = SparseReader {
        len: 128 * 1024 * 1024,
        position: 0,
        bytes_read: Rc::clone(&bytes_read),
    };

    assert_input_limit(reader);
    assert_eq!(bytes_read.get(), 0);
}

#[test]
fn zip64_central_extra_and_sentinel_forms_are_rejected_before_indexing() {
    for (offset, width) in [(20, 4), (24, 4), (34, 2), (42, 4)] {
        let mut sentinel = valid_package();
        let start = central_start(&sentinel);
        if width == 2 {
            patch_u16(&mut sentinel, start + offset, u16::MAX);
        } else {
            patch_u32(&mut sentinel, start + offset, u32::MAX);
        }
        assert_explicit_zip64_rejection(sentinel);
    }

    let mut version_needed = valid_package();
    let start = central_start(&version_needed);
    patch_u16(&mut version_needed, start + 6, 45);
    assert_explicit_zip64_rejection(version_needed);
}

#[test]
fn zip64_local_extra_and_sentinel_forms_are_rejected_before_indexing() {
    for offset in [18, 22] {
        let mut sentinel = valid_package();
        patch_u32(&mut sentinel, offset, u32::MAX);
        assert_explicit_zip64_rejection(sentinel);
    }

    let mut version = valid_package();
    patch_u16(&mut version, 4, 45);
    assert_explicit_zip64_rejection(version);
}

#[test]
fn associated_zip64_locator_and_eocd_are_rejected_explicitly() {
    for (offset, width) in [(4, 2), (6, 2), (8, 2), (10, 2), (12, 4), (16, 4)] {
        let mut bytes = valid_package();
        let end = insert_associated_zip64_records(&mut bytes);
        if width == 2 {
            patch_u16(&mut bytes, end + offset, u16::MAX);
        } else {
            patch_u32(&mut bytes, end + offset, u32::MAX);
        }
        assert_explicit_zip64_rejection(bytes);
    }
}

#[test]
fn truncated_central_fixed_and_name_fields_are_malformed() {
    let mut fixed = valid_package();
    truncate_first_central_record(&mut fixed, 40);
    assert_archive_contains(fixed, "central");

    let mut name = valid_package();
    truncate_first_central_record(&mut name, 46 + 5);
    assert_archive_contains(name, "central");
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

#[test]
fn canonical_envelope_rejects_archive_and_entry_comments() {
    let mut archive_comment = valid_package();
    append_eocd_comment(&mut archive_comment, b"comment");
    assert_canonical_envelope_rejection(archive_comment);

    let mut entry_comment = valid_package();
    insert_central_comment(&mut entry_comment, b"comment");
    assert_canonical_envelope_rejection(entry_comment);
}

#[test]
fn canonical_envelope_rejects_any_extra_field() {
    let mut bytes = valid_package();
    insert_central_extra(&mut bytes, &[0xfe, 0xca, 0, 0]);
    assert_canonical_envelope_rejection(bytes);

    let mut local = valid_package();
    patch_u16(&mut local, 28, 4);
    assert_canonical_envelope_rejection(local);
}

#[test]
fn canonical_envelope_rejects_data_descriptors() {
    let bytes = stream_zip_bytes(vec![
        (
            "manifest.json",
            br#"{"format":"depthsprite","version":1,"bounds_pixels":[1,1,1],"views":["front"]}
"#
            .to_vec(),
        ),
        ("views/front.png", rgba()),
    ]);
    assert_canonical_envelope_rejection(bytes);
}

#[test]
fn canonical_envelope_rejects_stored_and_reordered_entries() {
    assert_canonical_envelope_rejection(package_with_method_and_order(
        CompressionMethod::Stored,
        false,
    ));
    assert_canonical_envelope_rejection(package_with_method_and_order(
        CompressionMethod::Deflated,
        true,
    ));
}

#[test]
fn canonical_envelope_rejects_noncanonical_manifest_view_order() {
    let bytes = zip_bytes(vec![
        (
            "manifest.json",
            br#"{"format":"depthsprite","version":1,"bounds_pixels":[1,1,1],"views":["top","front"]}
"#
            .to_vec(),
        ),
        ("views/front.png", rgba()),
        ("views/top.png", rgba()),
    ]);

    assert_canonical_envelope_rejection(bytes);
}

#[test]
fn canonical_envelope_rejects_gaps_between_local_entries() {
    let mut bytes = valid_package();
    let old_central_start = central_start(&bytes);
    let front_central = central_record(&bytes, "views/front.png");
    let front_local = u32::from_le_bytes(
        bytes[front_central + 42..front_central + 46]
            .try_into()
            .unwrap(),
    ) as usize;
    bytes.insert(front_local, 0);
    let end = eocd(&bytes);
    patch_u32(&mut bytes, end + 16, (old_central_start + 1) as u32);
    let shifted_front_central = central_record(&bytes, "views/front.png");
    patch_u32(
        &mut bytes,
        shifted_front_central + 42,
        (front_local + 1) as u32,
    );

    assert_canonical_envelope_rejection(bytes);
}

#[test]
fn canonical_envelope_rejects_leading_and_trailing_junk() {
    let mut leading = valid_package();
    leading.insert(0, 0);
    assert_canonical_envelope_rejection(leading);

    let mut trailing = valid_package();
    trailing.push(0);
    assert_canonical_envelope_rejection(trailing);
}

#[test]
fn canonical_envelope_rejects_encryption_and_metadata_mismatch() {
    let mut encrypted = valid_package();
    let manifest_central = central_record(&encrypted, "manifest.json");
    patch_u16(&mut encrypted, 6, 1);
    patch_u16(&mut encrypted, manifest_central + 8, 1);
    assert!(matches!(
        load_reader(Cursor::new(encrypted)),
        Err(PackageError::EncryptedEntry(name)) if name == "manifest.json"
    ));

    let mut utf8_flag = valid_package();
    let manifest_central = central_record(&utf8_flag, "manifest.json");
    patch_u16(&mut utf8_flag, 6, 0x0800);
    patch_u16(&mut utf8_flag, manifest_central + 8, 0x0800);
    assert_canonical_envelope_rejection(utf8_flag);

    let mut mismatch = valid_package();
    let crc = u32::from_le_bytes(mismatch[14..18].try_into().unwrap());
    patch_u32(&mut mismatch, 14, crc ^ 1);
    assert_canonical_envelope_rejection(mismatch);
}

#[test]
fn canonical_envelope_rejects_noncanonical_fixed_metadata() {
    let mut bytes = valid_package();
    let manifest_central = central_record(&bytes, "manifest.json");
    patch_u16(&mut bytes, 10, 1);
    patch_u16(&mut bytes, manifest_central + 12, 1);

    assert_canonical_envelope_rejection(bytes);
}

#[test]
fn signature_filled_maximum_input_uses_only_fixed_tail_work() {
    let bytes_read = Rc::new(Cell::new(0));
    let seeks = Rc::new(Cell::new(0));
    let reader = SignatureReader {
        len: 65 * 1024 * 1024,
        position: 0,
        bytes_read: Rc::clone(&bytes_read),
        seeks: Rc::clone(&seeks),
    };

    assert!(load_reader(reader).is_err());
    assert!(
        bytes_read.get() <= 64,
        "read {} bytes while rejecting the fixed envelope",
        bytes_read.get()
    );
    assert!(seeks.get() <= 4, "performed {} seeks", seeks.get());
}
