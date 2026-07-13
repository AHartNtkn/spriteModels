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
const LOCAL_HEADER_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];
const CENTRAL_HEADER_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];
const EOCD_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
const ZIP64_EOCD_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x06, 0x06];
const ZIP64_LOCATOR_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x06, 0x07];
const EOCD_FIXED_SIZE: usize = 22;
const CENTRAL_FIXED_SIZE: u64 = 46;
const LOCAL_FIXED_SIZE: u64 = 30;
const MAX_EOCD_SEARCH: u64 = EOCD_FIXED_SIZE as u64 + u16::MAX as u64;
const ALLOWED_NAMES: [&str; 7] = [
    "manifest.json",
    "views/front.png",
    "views/back.png",
    "views/left.png",
    "views/right.png",
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
    extra: Vec<u8>,
}

#[derive(Debug)]
struct PreflightArchive {
    entries: Vec<EntryMetadata>,
    central_start: u64,
    central_end: u64,
    eocd_offset: u64,
    comment: Vec<u8>,
}

enum CandidateParse {
    Selected(Result<PreflightArchive, PackageError>),
    Invalid(PackageError),
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
    let search_len = file_len.min(MAX_EOCD_SEARCH) as usize;
    if search_len < EOCD_FIXED_SIZE {
        return Err(malformed("missing ZIP end record"));
    }
    reader.seek(SeekFrom::End(-(search_len as i64)))?;
    let mut tail = vec![0_u8; search_len];
    reader.read_exact(&mut tail)?;

    let tail_start = file_len - search_len as u64;
    let mut selected = Vec::new();
    let mut rightmost_invalid = None;
    for (index, signature) in tail.windows(EOCD_SIGNATURE.len()).enumerate() {
        if signature != EOCD_SIGNATURE || index + EOCD_FIXED_SIZE > tail.len() {
            continue;
        }
        let comment_len = u16::from_le_bytes([tail[index + 20], tail[index + 21]]) as usize;
        if index + EOCD_FIXED_SIZE + comment_len != tail.len() {
            continue;
        }
        match parse_candidate(
            reader,
            &tail[index..index + EOCD_FIXED_SIZE],
            &tail[index + EOCD_FIXED_SIZE..],
            tail_start + index as u64,
        ) {
            CandidateParse::Selected(candidate) => {
                selected.push(candidate);
                if selected.len() > 1 {
                    return Err(malformed(
                        "ambiguous ZIP archive has multiple structurally valid end records",
                    ));
                }
            }
            CandidateParse::Invalid(error) => rightmost_invalid = Some(error),
        }
    }

    match selected.pop() {
        Some(result) => result,
        None => Err(rightmost_invalid.unwrap_or_else(|| malformed("missing ZIP end record"))),
    }
}

fn parse_candidate<R: Read + Seek>(
    reader: &mut R,
    fixed: &[u8],
    comment: &[u8],
    eocd_offset: u64,
) -> CandidateParse {
    let disk = le_u16(fixed, 4);
    let central_disk = le_u16(fixed, 6);
    let disk_entries = le_u16(fixed, 8);
    let entry_count = le_u16(fixed, 10);
    if disk == u16::MAX
        || central_disk == u16::MAX
        || disk_entries == u16::MAX
        || entry_count == u16::MAX
    {
        return CandidateParse::Selected(Err(zip64_error()));
    }
    if disk != 0 || central_disk != 0 || disk_entries != entry_count {
        return CandidateParse::Invalid(malformed(
            "multi-disk ZIP end record is not a valid version 1 package",
        ));
    }
    if entry_count as usize > MAX_ENTRIES {
        return CandidateParse::Selected(Err(PackageError::EntryLimit {
            actual: entry_count as usize,
            limit: MAX_ENTRIES,
        }));
    }

    let central_size = le_u32(fixed, 12) as u64;
    let central_start = le_u32(fixed, 16) as u64;
    if central_size == u32::MAX as u64 || central_start == u32::MAX as u64 {
        return CandidateParse::Selected(Err(zip64_error()));
    }
    let Some(central_end) = central_start.checked_add(central_size) else {
        return CandidateParse::Invalid(malformed("invalid ZIP central directory bounds"));
    };
    if central_end != eocd_offset {
        if (eocd_offset >= 20
            && has_signature_at(reader, eocd_offset - 20, ZIP64_LOCATOR_SIGNATURE))
            || (central_end < eocd_offset
                && has_signature_at(reader, central_end, ZIP64_EOCD_SIGNATURE))
        {
            return CandidateParse::Selected(Err(zip64_error()));
        }
        return CandidateParse::Invalid(malformed("invalid ZIP central directory bounds"));
    }

    match parse_central_directory(reader, central_start, central_end, entry_count as usize) {
        Ok((entries, rejection)) => {
            let preflight = PreflightArchive {
                entries,
                central_start,
                central_end,
                eocd_offset,
                comment: comment.to_vec(),
            };
            CandidateParse::Selected(match rejection {
                Some(error) => Err(error),
                None => validate_preflight(preflight)
                    .and_then(|preflight| validate_local_headers(reader, preflight)),
            })
        }
        Err(error) => CandidateParse::Invalid(error),
    }
}

fn parse_central_directory<R: Read + Seek>(
    reader: &mut R,
    central_start: u64,
    central_end: u64,
    entry_count: usize,
) -> Result<(Vec<EntryMetadata>, Option<PackageError>), PackageError> {
    reader.seek(SeekFrom::Start(central_start))?;
    let mut entries = Vec::with_capacity(entry_count);
    let mut rejection = None;
    for _ in 0..entry_count {
        let record_start = reader.stream_position()?;
        let fixed_end = record_start
            .checked_add(CENTRAL_FIXED_SIZE)
            .ok_or_else(|| malformed("central directory fixed field overflows"))?;
        if fixed_end > central_end {
            return Err(malformed("truncated central directory fixed field"));
        }
        let mut fixed = [0_u8; CENTRAL_FIXED_SIZE as usize];
        reader.read_exact(&mut fixed).map_err(central_read_error)?;
        if fixed[..4] != CENTRAL_HEADER_SIGNATURE {
            return Err(malformed("invalid ZIP central directory entry signature"));
        }

        let name_len = le_u16(&fixed, 28) as u64;
        let extra_len = le_u16(&fixed, 30) as u64;
        let comment_len = le_u16(&fixed, 32) as u64;
        let name_end = fixed_end
            .checked_add(name_len)
            .ok_or_else(|| malformed("central directory name length overflows"))?;
        let extra_end = name_end
            .checked_add(extra_len)
            .ok_or_else(|| malformed("central directory extra length overflows"))?;
        let record_end = extra_end
            .checked_add(comment_len)
            .ok_or_else(|| malformed("central directory comment length overflows"))?;
        if record_end > central_end {
            return Err(malformed(
                "truncated central directory name, extra, or comment field",
            ));
        }

        let mut raw_name = vec![0_u8; name_len as usize];
        reader
            .read_exact(&mut raw_name)
            .map_err(central_read_error)?;
        let mut extra = vec![0_u8; extra_len as usize];
        reader.read_exact(&mut extra).map_err(central_read_error)?;
        let has_zip64_extra = parse_extra_fields(&extra, "central")?;
        reader
            .seek(SeekFrom::Start(record_end))
            .map_err(central_read_error)?;

        let version_needed = le_u16(&fixed, 6);
        let compressed_size = le_u32(&fixed, 20);
        let declared_size = le_u32(&fixed, 24);
        let disk_start = le_u16(&fixed, 34);
        let local_header_offset = le_u32(&fixed, 42);
        let has_zip64_sentinel = compressed_size == u32::MAX
            || declared_size == u32::MAX
            || disk_start == u16::MAX
            || local_header_offset == u32::MAX;
        if has_zip64_extra || has_zip64_sentinel || version_needed >= 45 {
            rejection.get_or_insert_with(zip64_error);
        }
        if disk_start != 0 {
            rejection.get_or_insert_with(|| {
                malformed("multi-disk central directory entry is not supported")
            });
        }

        entries.push(EntryMetadata {
            raw_name,
            name: String::new(),
            flags: le_u16(&fixed, 8),
            method: le_u16(&fixed, 10),
            crc32: le_u32(&fixed, 16),
            compressed_size: compressed_size as u64,
            declared_size: declared_size as u64,
            local_header_offset: local_header_offset as u64,
            central_header_offset: record_start,
            extra,
        });
    }
    if reader.stream_position()? != central_end {
        return Err(malformed(
            "ZIP central directory size does not match its entries",
        ));
    }
    Ok((entries, rejection))
}

fn parse_extra_fields(extra: &[u8], context: &str) -> Result<bool, PackageError> {
    let mut offset = 0_usize;
    let mut has_zip64 = false;
    while offset < extra.len() {
        let header_end = offset
            .checked_add(4)
            .filter(|end| *end <= extra.len())
            .ok_or_else(|| malformed(format!("truncated {context} extra field header")))?;
        let id = le_u16(extra, offset);
        let length = le_u16(extra, offset + 2) as usize;
        let field_end = header_end
            .checked_add(length)
            .filter(|end| *end <= extra.len())
            .ok_or_else(|| malformed(format!("truncated {context} extra field payload")))?;
        has_zip64 |= id == 0x0001;
        offset = field_end;
    }
    Ok(has_zip64)
}

fn validate_preflight(mut preflight: PreflightArchive) -> Result<PreflightArchive, PackageError> {
    let mut declared_total = 0_u64;
    for entry in &preflight.entries {
        declared_total = declared_total.saturating_add(entry.declared_size);
        if declared_total > MAX_EXPANDED_SIZE {
            return Err(PackageError::ExpandedSizeLimit {
                actual: declared_total,
                limit: MAX_EXPANDED_SIZE,
            });
        }
    }

    let mut unique = HashSet::with_capacity(preflight.entries.len());
    for entry in &mut preflight.entries {
        let name = validate_safe_name(&entry.raw_name)?;
        if !ALLOWED_NAMES.contains(&name.as_str()) {
            return Err(PackageError::UndeclaredEntry(name));
        }
        if !unique.insert(name.clone()) {
            return Err(PackageError::DuplicateEntry(name));
        }
        if entry.flags & 1 != 0 {
            return Err(PackageError::EncryptedEntry(name));
        }
        if !matches!(entry.method, 0 | 8) {
            return Err(PackageError::UnsupportedCompression {
                entry: name,
                method: entry.method.to_string(),
            });
        }
        entry.name = name;
    }
    Ok(preflight)
}

fn validate_local_headers<R: Read + Seek>(
    reader: &mut R,
    preflight: PreflightArchive,
) -> Result<PreflightArchive, PackageError> {
    let mut occupied = Vec::with_capacity(preflight.entries.len());
    let mut local_declared_total = 0_u64;
    for entry in &preflight.entries {
        let fixed_end = entry
            .local_header_offset
            .checked_add(LOCAL_FIXED_SIZE)
            .ok_or_else(|| malformed("local header fixed field overflows"))?;
        if fixed_end > preflight.central_start {
            return Err(malformed("truncated local header fixed field"));
        }
        reader.seek(SeekFrom::Start(entry.local_header_offset))?;
        let mut fixed = [0_u8; LOCAL_FIXED_SIZE as usize];
        reader.read_exact(&mut fixed).map_err(local_read_error)?;
        if fixed[..4] != LOCAL_HEADER_SIGNATURE {
            return Err(malformed("invalid ZIP local header signature"));
        }

        let name_len = le_u16(&fixed, 26) as u64;
        let extra_len = le_u16(&fixed, 28) as u64;
        let name_end = fixed_end
            .checked_add(name_len)
            .ok_or_else(|| malformed("local header name length overflows"))?;
        let data_start = name_end
            .checked_add(extra_len)
            .ok_or_else(|| malformed("local header extra length overflows"))?;
        let data_end = data_start
            .checked_add(entry.compressed_size)
            .ok_or_else(|| malformed("compressed entry data bounds overflow"))?;
        if data_end > preflight.central_start {
            return Err(malformed(
                "local header or compressed entry data crosses the central directory",
            ));
        }

        let mut raw_name = vec![0_u8; name_len as usize];
        reader.read_exact(&mut raw_name).map_err(local_read_error)?;
        let mut extra = vec![0_u8; extra_len as usize];
        reader.read_exact(&mut extra).map_err(local_read_error)?;
        let has_zip64_extra = parse_extra_fields(&extra, "local")?;
        let version_needed = le_u16(&fixed, 4);
        let local_flags = le_u16(&fixed, 6);
        let local_method = le_u16(&fixed, 8);
        let local_crc = le_u32(&fixed, 14);
        let local_compressed = le_u32(&fixed, 18);
        let local_uncompressed = le_u32(&fixed, 22);

        if has_zip64_extra
            || version_needed >= 45
            || local_compressed == u32::MAX
            || local_uncompressed == u32::MAX
        {
            return Err(zip64_error());
        }
        if local_flags & 0x0008 == 0 {
            local_declared_total = local_declared_total.saturating_add(local_uncompressed as u64);
            if local_declared_total > MAX_EXPANDED_SIZE {
                return Err(PackageError::ExpandedSizeLimit {
                    actual: local_declared_total,
                    limit: MAX_EXPANDED_SIZE,
                });
            }
        }
        if raw_name != entry.raw_name || local_flags != entry.flags || local_method != entry.method
        {
            return Err(parser_differential(format!(
                "local and central identity fields differ for {}",
                entry.name
            )));
        }
        if local_flags & 0x0008 == 0
            && (local_crc != entry.crc32
                || local_compressed as u64 != entry.compressed_size
                || local_uncompressed as u64 != entry.declared_size)
        {
            return Err(parser_differential(format!(
                "local and central sizes or CRC differ for {}",
                entry.name
            )));
        }
        occupied.push((entry.local_header_offset, data_end));
    }

    occupied.sort_unstable();
    if occupied.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(malformed("local ZIP entries overlap"));
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
        || archive.comment() != preflight.comment
        || preflight.central_end != preflight.eocd_offset
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
        let expected_method = match expected.method {
            0 => CompressionMethod::Stored,
            8 => CompressionMethod::Deflated,
            _ => unreachable!("preflight accepted only supported methods"),
        };
        if actual.name_raw() != expected.raw_name
            || actual.compressed_size() != expected.compressed_size
            || actual.size() != expected.declared_size
            || actual.crc32() != expected.crc32
            || actual.compression() != expected_method
            || actual.header_start() != expected.local_header_offset
            || actual.central_header_start() != expected.central_header_offset
            || actual.extra_data().unwrap_or_default() != expected.extra
            || actual.encrypted() != (expected.flags & 1 != 0)
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
        let mut expanded: Box<dyn Read + '_> = match data.method {
            0 => Box::new(raw),
            8 => Box::new(DeflateDecoder::new(raw)),
            _ => unreachable!("compression was validated before reading"),
        };
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

fn has_signature_at<R: Read + Seek>(reader: &mut R, offset: u64, signature: [u8; 4]) -> bool {
    if reader.seek(SeekFrom::Start(offset)).is_err() {
        return false;
    }
    let mut actual = [0_u8; 4];
    reader.read_exact(&mut actual).is_ok() && actual == signature
}

fn malformed(message: impl Into<String>) -> PackageError {
    PackageError::Archive(message.into())
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
