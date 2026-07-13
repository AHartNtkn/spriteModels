use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufReader, Cursor, Read, Seek, SeekFrom},
    path::Path,
};

use flate2::read::DeflateDecoder;
use png::{BitDepth, ColorType, Decoder, Transformations};
use relief_core::{CanonicalView, Chart, ChartError};
use zip::{CompressionMethod, ZipArchive};

use crate::{DepthSpriteModel, ManifestV1, PackageError};

const MAX_ENTRIES: usize = 7;
const MAX_EXPANDED_SIZE: u64 = 64 * 1024 * 1024;
/// Maximum complete package size accepted before any archive bytes are read.
pub const MAX_ARCHIVE_SIZE: u64 = 65 * 1024 * 1024;
/// Maximum aggregate compressed payload declared by the central directory.
pub const MAX_COMPRESSED_SIZE: u64 = 64 * 1024 * 1024;
const LOCAL_HEADER_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];
const CENTRAL_HEADER_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];
const EOCD_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
const EOCD_FIXED_SIZE: usize = 22;
const CENTRAL_FIXED_SIZE: u64 = 46;
const LOCAL_FIXED_SIZE: u64 = 30;
const CANONICAL_VERSION_NEEDED: u16 = 20;
const CANONICAL_FLAGS: u16 = 0;
const CANONICAL_METHOD: u16 = 8;
const CANONICAL_MODIFIED_TIME: u16 = 0;
const CANONICAL_MODIFIED_DATE: u16 = 33;
const CANONICAL_EXTERNAL_ATTRIBUTES: u32 = 0o100644 << 16;
const ALLOWED_NAMES: [&str; 7] = [
    "manifest.json",
    "views/front.png",
    "views/right.png",
    "views/back.png",
    "views/left.png",
    "views/top.png",
    "views/bottom.png",
];

#[derive(Debug)]
struct EntryMetadata {
    raw_name: Vec<u8>,
    name: String,
    flags: u16,
    method: u16,
    crc32: u32,
    compressed_size: u64,
    declared_size: u64,
    local_header_offset: u64,
    central_header_offset: u64,
    modified_time: u16,
    modified_date: u16,
}

#[derive(Debug)]
struct PreflightArchive {
    entries: Vec<EntryMetadata>,
    central_start: u64,
    eocd_offset: u64,
}

pub fn load_path(path: impl AsRef<Path>) -> Result<DepthSpriteModel, PackageError> {
    let file = File::open(path)?;
    load_reader(BufReader::new(file))
}

pub fn load_reader<R: Read + Seek>(mut reader: R) -> Result<DepthSpriteModel, PackageError> {
    let preflight = preflight_archive(&mut reader)?;

    reader.seek(SeekFrom::Start(0))?;
    let mut archive = ZipArchive::new(reader)?;
    bind_dependency_index(&mut archive, &preflight)?;
    let contents = read_all_bounded(&mut archive, &preflight.entries)?;

    let manifest_bytes = contents
        .get("manifest.json")
        .ok_or_else(|| PackageError::MissingEntry("manifest.json".to_owned()))?;
    let manifest: ManifestV1 = serde_json::from_slice(manifest_bytes)
        .map_err(|error| PackageError::Manifest(error.to_string()))?;
    let bounds = manifest.validate()?;
    validate_declared_entries(&preflight.entries, &manifest)?;

    let mut views = manifest.views.clone();
    views.sort_by_key(|view| view.rank());
    let mut charts = Vec::with_capacity(views.len());
    for view_name in views {
        let entry = view_name.entry_name();
        let bytes = contents
            .get(entry)
            .ok_or_else(|| PackageError::MissingEntry(entry.to_owned()))?;
        charts.push(decode_chart(entry, bytes, bounds, view_name.into())?);
    }
    DepthSpriteModel::new(bounds, charts)
}

fn preflight_archive<R: Read + Seek>(reader: &mut R) -> Result<PreflightArchive, PackageError> {
    let file_len = reader.seek(SeekFrom::End(0))?;
    if file_len > MAX_ARCHIVE_SIZE {
        return Err(PackageError::InputSizeLimit {
            actual: file_len,
            limit: MAX_ARCHIVE_SIZE,
        });
    }
    if file_len < EOCD_FIXED_SIZE as u64 {
        return Err(noncanonical("missing final ordinary EOCD"));
    }
    let eocd_offset = file_len - EOCD_FIXED_SIZE as u64;
    reader.seek(SeekFrom::Start(eocd_offset))?;
    let mut eocd = [0_u8; EOCD_FIXED_SIZE];
    reader.read_exact(&mut eocd)?;
    if eocd[..4] != EOCD_SIGNATURE {
        return Err(noncanonical("final 22 bytes are not the sole EOCD"));
    }
    if le_u16(&eocd, 20) != 0 {
        return Err(noncanonical("archive comments are not permitted"));
    }

    let disk = le_u16(&eocd, 4);
    let central_disk = le_u16(&eocd, 6);
    let disk_entries = le_u16(&eocd, 8);
    let entry_count = le_u16(&eocd, 10);
    let central_size = le_u32(&eocd, 12);
    let central_start = le_u32(&eocd, 16);
    if disk == u16::MAX
        || central_disk == u16::MAX
        || disk_entries == u16::MAX
        || entry_count == u16::MAX
        || central_size == u32::MAX
        || central_start == u32::MAX
    {
        return Err(zip64_error());
    }
    if disk != 0 || central_disk != 0 || disk_entries != entry_count {
        return Err(noncanonical("multi-disk EOCD metadata is not permitted"));
    }
    let entry_count = entry_count as usize;
    if !(1..=MAX_ENTRIES).contains(&entry_count) {
        return Err(PackageError::EntryLimit {
            actual: entry_count,
            limit: MAX_ENTRIES,
        });
    }
    let central_start = central_start as u64;
    let central_end = central_start
        .checked_add(central_size as u64)
        .ok_or_else(|| noncanonical("central directory bounds overflow"))?;
    if central_end != eocd_offset {
        return Err(noncanonical(
            "central directory must end exactly at the final EOCD",
        ));
    }

    let entries =
        parse_canonical_central_directory(reader, central_start, central_end, entry_count)?;
    let preflight = PreflightArchive {
        entries,
        central_start,
        eocd_offset,
    };
    validate_canonical_local_entries(reader, preflight)
}

fn parse_canonical_central_directory<R: Read + Seek>(
    reader: &mut R,
    central_start: u64,
    central_end: u64,
    entry_count: usize,
) -> Result<Vec<EntryMetadata>, PackageError> {
    reader.seek(SeekFrom::Start(central_start))?;
    let mut entries = Vec::with_capacity(entry_count);
    let mut last_name_rank = 0_usize;
    let mut unique_names = HashSet::with_capacity(entry_count);
    let mut compressed_total = 0_u64;
    let mut expanded_total = 0_u64;

    for index in 0..entry_count {
        let record_start = reader.stream_position()?;
        let fixed_end = record_start
            .checked_add(CENTRAL_FIXED_SIZE)
            .ok_or_else(|| noncanonical("central fixed field bounds overflow"))?;
        if fixed_end > central_end {
            return Err(noncanonical("truncated central directory record"));
        }
        let mut fixed = [0_u8; CENTRAL_FIXED_SIZE as usize];
        reader.read_exact(&mut fixed).map_err(central_read_error)?;
        if fixed[..4] != CENTRAL_HEADER_SIGNATURE {
            return Err(noncanonical("invalid central directory signature"));
        }

        let name_len = le_u16(&fixed, 28) as u64;
        let extra_len = le_u16(&fixed, 30) as u64;
        let comment_len = le_u16(&fixed, 32) as u64;
        let name_end = fixed_end
            .checked_add(name_len)
            .ok_or_else(|| noncanonical("central name bounds overflow"))?;
        let record_end = name_end
            .checked_add(extra_len)
            .and_then(|end| end.checked_add(comment_len))
            .ok_or_else(|| noncanonical("central variable fields overflow"))?;
        if record_end > central_end {
            return Err(noncanonical("truncated central variable fields"));
        }
        let mut raw_name = vec![0_u8; name_len as usize];
        reader
            .read_exact(&mut raw_name)
            .map_err(central_read_error)?;
        let name = validate_safe_name(&raw_name)?;
        if extra_len != 0 {
            return Err(noncanonical("central extra fields are not permitted"));
        }
        if comment_len != 0 {
            return Err(noncanonical("entry comments are not permitted"));
        }

        let Some(name_rank) = ALLOWED_NAMES.iter().position(|allowed| *allowed == name) else {
            return Err(PackageError::UndeclaredEntry(name));
        };
        if !unique_names.insert(name.clone()) {
            return Err(PackageError::DuplicateEntry(name));
        }
        if (index == 0 && name_rank != 0)
            || (index > 0 && (name_rank == 0 || name_rank <= last_name_rank))
        {
            return Err(noncanonical(
                "entries must be manifest first then canonical view rank",
            ));
        }
        last_name_rank = name_rank;

        let version_needed = le_u16(&fixed, 6);
        let flags = le_u16(&fixed, 8);
        let method = le_u16(&fixed, 10);
        let compressed_size = le_u32(&fixed, 20);
        let declared_size = le_u32(&fixed, 24);
        let disk_start = le_u16(&fixed, 34);
        let local_header_offset = le_u32(&fixed, 42);
        if compressed_size == u32::MAX
            || declared_size == u32::MAX
            || disk_start == u16::MAX
            || local_header_offset == u32::MAX
            || version_needed >= 45
        {
            return Err(zip64_error());
        }
        if flags & 1 != 0 {
            return Err(PackageError::EncryptedEntry(name));
        }
        if flags & 0x0008 != 0 {
            return Err(noncanonical("data descriptors are not permitted"));
        }
        if flags != CANONICAL_FLAGS {
            return Err(noncanonical("noncanonical general-purpose flags"));
        }
        if method != CANONICAL_METHOD {
            return Err(noncanonical("only canonical Deflate method 8 is permitted"));
        }
        if version_needed != CANONICAL_VERSION_NEEDED {
            return Err(noncanonical("noncanonical version-needed metadata"));
        }
        if !matches!(le_u16(&fixed, 4), 20 | 0x0314)
            || le_u16(&fixed, 12) != CANONICAL_MODIFIED_TIME
            || le_u16(&fixed, 14) != CANONICAL_MODIFIED_DATE
            || disk_start != 0
            || le_u16(&fixed, 36) != 0
            || le_u32(&fixed, 38) != CANONICAL_EXTERNAL_ATTRIBUTES
        {
            return Err(noncanonical("noncanonical fixed central metadata"));
        }

        compressed_total = compressed_total.checked_add(compressed_size as u64).ok_or(
            PackageError::InputSizeLimit {
                actual: u64::MAX,
                limit: MAX_COMPRESSED_SIZE,
            },
        )?;
        if compressed_total > MAX_COMPRESSED_SIZE {
            return Err(PackageError::InputSizeLimit {
                actual: compressed_total,
                limit: MAX_COMPRESSED_SIZE,
            });
        }
        expanded_total = expanded_total.checked_add(declared_size as u64).ok_or(
            PackageError::ExpandedSizeLimit {
                actual: u64::MAX,
                limit: MAX_EXPANDED_SIZE,
            },
        )?;
        if expanded_total > MAX_EXPANDED_SIZE {
            return Err(PackageError::ExpandedSizeLimit {
                actual: expanded_total,
                limit: MAX_EXPANDED_SIZE,
            });
        }

        entries.push(EntryMetadata {
            raw_name,
            name,
            flags,
            method,
            crc32: le_u32(&fixed, 16),
            compressed_size: compressed_size as u64,
            declared_size: declared_size as u64,
            local_header_offset: local_header_offset as u64,
            central_header_offset: record_start,
            modified_time: le_u16(&fixed, 12),
            modified_date: le_u16(&fixed, 14),
        });
        reader.seek(SeekFrom::Start(record_end))?;
    }
    if reader.stream_position()? != central_end {
        return Err(noncanonical(
            "central directory has gaps or undeclared records",
        ));
    }
    Ok(entries)
}

fn validate_canonical_local_entries<R: Read + Seek>(
    reader: &mut R,
    preflight: PreflightArchive,
) -> Result<PreflightArchive, PackageError> {
    let mut expected_offset = 0_u64;
    for entry in &preflight.entries {
        if entry.local_header_offset != expected_offset {
            return Err(noncanonical(
                "local entries must be contiguous from byte zero in central order",
            ));
        }
        reader.seek(SeekFrom::Start(expected_offset))?;
        let mut fixed = [0_u8; LOCAL_FIXED_SIZE as usize];
        reader.read_exact(&mut fixed).map_err(local_read_error)?;
        if fixed[..4] != LOCAL_HEADER_SIGNATURE {
            return Err(noncanonical("invalid local header signature"));
        }

        let name_len = le_u16(&fixed, 26) as u64;
        let extra_len = le_u16(&fixed, 28) as u64;
        let name_end = expected_offset
            .checked_add(LOCAL_FIXED_SIZE)
            .and_then(|end| end.checked_add(name_len))
            .ok_or_else(|| noncanonical("local name bounds overflow"))?;
        if name_end > preflight.central_start {
            return Err(noncanonical("truncated local name"));
        }
        let mut raw_name = vec![0_u8; name_len as usize];
        reader.read_exact(&mut raw_name).map_err(local_read_error)?;
        validate_safe_name(&raw_name)?;
        if extra_len != 0 {
            return Err(noncanonical("local extra fields are not permitted"));
        }
        if le_u16(&fixed, 4) >= 45
            || le_u32(&fixed, 18) == u32::MAX
            || le_u32(&fixed, 22) == u32::MAX
        {
            return Err(zip64_error());
        }
        let data_end = name_end
            .checked_add(entry.compressed_size)
            .ok_or_else(|| noncanonical("compressed payload bounds overflow"))?;
        if data_end > preflight.central_start {
            return Err(noncanonical("compressed payload crosses central directory"));
        }

        if raw_name != entry.raw_name
            || le_u16(&fixed, 4) != CANONICAL_VERSION_NEEDED
            || le_u16(&fixed, 6) != entry.flags
            || le_u16(&fixed, 8) != entry.method
            || le_u16(&fixed, 10) != entry.modified_time
            || le_u16(&fixed, 12) != entry.modified_date
            || le_u32(&fixed, 14) != entry.crc32
            || le_u32(&fixed, 18) as u64 != entry.compressed_size
            || le_u32(&fixed, 22) as u64 != entry.declared_size
        {
            return Err(noncanonical(
                "local and central entry metadata must agree exactly",
            ));
        }
        expected_offset = data_end;
    }
    if expected_offset != preflight.central_start {
        return Err(noncanonical(
            "last local payload must end exactly at the central directory",
        ));
    }
    Ok(preflight)
}

fn validate_safe_name(raw: &[u8]) -> Result<String, PackageError> {
    let display = String::from_utf8_lossy(raw).into_owned();
    if !raw.is_ascii() || raw.iter().any(|byte| byte.is_ascii_control()) {
        return Err(PackageError::UnsafeEntry(display));
    }
    let name =
        String::from_utf8(raw.to_vec()).map_err(|_| PackageError::UnsafeEntry(display.clone()))?;
    if name.is_empty()
        || name.starts_with('/')
        || name.contains(['\\', ':'])
        || name.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.ends_with('.')
                || part.ends_with(' ')
        })
    {
        return Err(PackageError::UnsafeEntry(name));
    }
    Ok(name)
}

fn bind_dependency_index<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    preflight: &PreflightArchive,
) -> Result<(), PackageError> {
    if archive.offset() != 0
        || archive.central_directory_start() != preflight.central_start
        || !archive.comment().is_empty()
        || preflight.central_start > preflight.eocd_offset
    {
        return Err(parser_differential(
            "ZIP parser selected different archive or central-directory bounds",
        ));
    }
    if archive.len() != preflight.entries.len() {
        return Err(parser_differential(format!(
            "preflight found {} entries but ZIP parser indexed {}",
            preflight.entries.len(),
            archive.len()
        )));
    }
    for (index, expected) in preflight.entries.iter().enumerate() {
        let actual = archive.by_index_raw(index)?;
        if actual.name_raw() != expected.raw_name
            || actual.compressed_size() != expected.compressed_size
            || actual.size() != expected.declared_size
            || actual.crc32() != expected.crc32
            || actual.compression() != CompressionMethod::Deflated
            || actual.header_start() != expected.local_header_offset
            || actual.central_header_start() != expected.central_header_offset
            || !actual.extra_data().unwrap_or_default().is_empty()
            || actual.encrypted()
        {
            return Err(parser_differential(format!(
                "ZIP parser metadata differs from authoritative preflight at entry {index}"
            )));
        }
        if !actual.is_file() {
            return Err(PackageError::UnsafeEntry(expected.name.clone()));
        }
    }
    Ok(())
}

fn read_all_bounded<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    metadata: &[EntryMetadata],
) -> Result<HashMap<String, Vec<u8>>, PackageError> {
    let mut total = 0_u64;
    let mut contents = HashMap::with_capacity(metadata.len());
    for (index, data) in metadata.iter().enumerate() {
        let raw = archive.by_index_raw(index)?;
        let mut expanded = DeflateDecoder::new(raw);
        let remaining = MAX_EXPANDED_SIZE - total;
        let mut bytes = Vec::new();
        expanded
            .by_ref()
            .take(remaining + 1)
            .read_to_end(&mut bytes)?;
        total += bytes.len() as u64;
        if total > MAX_EXPANDED_SIZE {
            return Err(PackageError::ExpandedSizeLimit {
                actual: total,
                limit: MAX_EXPANDED_SIZE,
            });
        }
        if bytes.len() as u64 != data.declared_size {
            return Err(PackageError::SizeMismatch {
                entry: data.name.clone(),
                declared: data.declared_size,
                actual: bytes.len() as u64,
            });
        }
        if crc32fast::hash(&bytes) != data.crc32 {
            return Err(PackageError::Integrity(data.name.clone()));
        }
        contents.insert(data.name.clone(), bytes);
    }
    Ok(contents)
}

fn validate_declared_entries(
    metadata: &[EntryMetadata],
    manifest: &ManifestV1,
) -> Result<(), PackageError> {
    let expected: HashSet<&str> = manifest
        .views
        .iter()
        .map(|view| view.entry_name())
        .collect();
    for entry in metadata {
        if entry.name != "manifest.json" && !expected.contains(entry.name.as_str()) {
            return Err(PackageError::UndeclaredEntry(entry.name.clone()));
        }
    }
    for expected_name in expected {
        if !metadata.iter().any(|entry| entry.name == expected_name) {
            return Err(PackageError::MissingEntry(expected_name.to_owned()));
        }
    }
    if metadata.len() != manifest.views.len() + 1
        || metadata[1..]
            .iter()
            .zip(&manifest.views)
            .any(|(entry, view)| entry.name != view.entry_name())
    {
        return Err(noncanonical(
            "manifest views must match entry names in canonical rank order",
        ));
    }
    Ok(())
}

fn decode_chart(
    entry: &str,
    bytes: &[u8],
    bounds: relief_core::Bounds,
    view: CanonicalView,
) -> Result<Chart, PackageError> {
    let mut decoder = Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(Transformations::IDENTITY);
    let mut reader = decoder
        .read_info()
        .map_err(|error| PackageError::InvalidPng {
            entry: entry.to_owned(),
            message: error.to_string(),
        })?;
    let info = reader.info();
    if info.color_type != ColorType::Rgba || info.bit_depth != BitDepth::Eight {
        return Err(PackageError::InvalidPngType {
            entry: entry.to_owned(),
            color_type: format!("{:?}", info.color_type),
            bit_depth: format!("{:?}", info.bit_depth),
        });
    }
    if let Err(source @ ChartError::DimensionMismatch { .. }) =
        Chart::from_rgba(bounds, view, info.width, info.height, Vec::new())
    {
        return Err(PackageError::InvalidChart { view, source });
    }
    let buffer_size = reader
        .output_buffer_size()
        .ok_or_else(|| PackageError::InvalidPng {
            entry: entry.to_owned(),
            message: "decoded image is too large".to_owned(),
        })?;
    let mut buffer = vec![0; buffer_size];
    let output = reader
        .next_frame(&mut buffer)
        .map_err(|error| PackageError::InvalidPng {
            entry: entry.to_owned(),
            message: error.to_string(),
        })?;
    let pixels = buffer[..output.buffer_size()]
        .chunks_exact(4)
        .map(|chunk| [chunk[0], chunk[1], chunk[2], chunk[3]])
        .collect();
    Chart::from_rgba(bounds, view, output.width, output.height, pixels)
        .map_err(|source| PackageError::InvalidChart { view, source })
}

fn le_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn le_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn malformed(message: impl Into<String>) -> PackageError {
    PackageError::Archive(message.into())
}

fn noncanonical(reason: impl Into<String>) -> PackageError {
    malformed(format!(
        "noncanonical version-1 canonical ZIP32 envelope: {}",
        reason.into()
    ))
}

fn parser_differential(message: impl Into<String>) -> PackageError {
    PackageError::Archive(format!(
        "malformed archive parser differential: {}",
        message.into()
    ))
}

fn zip64_error() -> PackageError {
    malformed("ZIP64 structures are not supported by the version 1 package profile")
}

fn central_read_error(error: std::io::Error) -> PackageError {
    malformed(format!("truncated central directory: {error}"))
}

fn local_read_error(error: std::io::Error) -> PackageError {
    malformed(format!("truncated local header: {error}"))
}
