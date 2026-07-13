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
const CENTRAL_HEADER_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];
const EOCD_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
const EOCD_FIXED_SIZE: usize = 22;
const MAX_EOCD_SEARCH: u64 = EOCD_FIXED_SIZE as u64 + u16::MAX as u64;

struct EntryMetadata {
    name: String,
    declared_size: u64,
    compression: CompressionMethod,
    crc32: u32,
}

pub fn load_path(path: impl AsRef<Path>) -> Result<DepthSpriteModel, PackageError> {
    let file = File::open(path)?;
    load_reader(BufReader::new(file))
}

pub fn load_reader<R: Read + Seek>(mut reader: R) -> Result<DepthSpriteModel, PackageError> {
    let central_names = read_preflight_names(&mut reader)?;
    validate_central_names(&central_names)?;

    reader.seek(SeekFrom::Start(0))?;
    let mut archive = ZipArchive::new(reader)?;
    let metadata = inspect_metadata(&mut archive)?;
    let contents = read_all_bounded(&mut archive, &metadata)?;

    let manifest_bytes = contents
        .get("manifest.json")
        .ok_or_else(|| PackageError::MissingEntry("manifest.json".to_owned()))?;
    let manifest: ManifestV1 = serde_json::from_slice(manifest_bytes)
        .map_err(|error| PackageError::Manifest(error.to_string()))?;
    let bounds = manifest.validate()?;
    validate_declared_entries(&metadata, &manifest)?;

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

fn read_preflight_names<R: Read + Seek>(reader: &mut R) -> Result<Vec<Vec<u8>>, PackageError> {
    let file_len = reader.seek(SeekFrom::End(0))?;
    let search_len = file_len.min(MAX_EOCD_SEARCH) as usize;
    if search_len < EOCD_FIXED_SIZE {
        return Err(PackageError::Archive("missing ZIP end record".to_owned()));
    }
    reader.seek(SeekFrom::End(-(search_len as i64)))?;
    let mut tail = vec![0_u8; search_len];
    reader.read_exact(&mut tail)?;
    let eocd = tail
        .windows(EOCD_SIGNATURE.len())
        .enumerate()
        .rev()
        .find_map(|(index, signature)| {
            if signature != EOCD_SIGNATURE || index + EOCD_FIXED_SIZE > tail.len() {
                return None;
            }
            let comment_len = u16::from_le_bytes([tail[index + 20], tail[index + 21]]) as usize;
            (index + EOCD_FIXED_SIZE + comment_len == tail.len()).then_some(index)
        })
        .ok_or_else(|| PackageError::Archive("missing ZIP end record".to_owned()))?;

    let disk = u16::from_le_bytes([tail[eocd + 4], tail[eocd + 5]]);
    let central_disk = u16::from_le_bytes([tail[eocd + 6], tail[eocd + 7]]);
    let disk_entries = u16::from_le_bytes([tail[eocd + 8], tail[eocd + 9]]);
    let entry_count = u16::from_le_bytes([tail[eocd + 10], tail[eocd + 11]]);
    if disk != 0 || central_disk != 0 || disk_entries != entry_count {
        return Err(PackageError::Archive(
            "multi-disk ZIP archives are not supported".to_owned(),
        ));
    }
    if entry_count == u16::MAX {
        return Err(PackageError::Archive(
            "ZIP64 archives are not supported by the version 1 package profile".to_owned(),
        ));
    }
    let entry_count = entry_count as usize;
    if entry_count > MAX_ENTRIES {
        return Err(PackageError::EntryLimit {
            actual: entry_count,
            limit: MAX_ENTRIES,
        });
    }

    let central_size = u32::from_le_bytes([
        tail[eocd + 12],
        tail[eocd + 13],
        tail[eocd + 14],
        tail[eocd + 15],
    ]) as u64;
    let central_start = u32::from_le_bytes([
        tail[eocd + 16],
        tail[eocd + 17],
        tail[eocd + 18],
        tail[eocd + 19],
    ]) as u64;
    if central_size == u32::MAX as u64 || central_start == u32::MAX as u64 {
        return Err(PackageError::Archive(
            "ZIP64 archives are not supported by the version 1 package profile".to_owned(),
        ));
    }
    let eocd_absolute = file_len - search_len as u64 + eocd as u64;
    let central_end = central_start
        .checked_add(central_size)
        .filter(|end| *end == eocd_absolute)
        .ok_or_else(|| PackageError::Archive("invalid ZIP central directory bounds".to_owned()))?;

    reader.seek(SeekFrom::Start(central_start))?;
    let mut names = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        let mut fixed = [0_u8; 46];
        reader.read_exact(&mut fixed)?;
        if fixed[..4] != CENTRAL_HEADER_SIGNATURE {
            return Err(PackageError::Archive(
                "invalid ZIP central directory entry".to_owned(),
            ));
        }
        let name_len = u16::from_le_bytes([fixed[28], fixed[29]]) as usize;
        let extra_len = u16::from_le_bytes([fixed[30], fixed[31]]) as u64;
        let comment_len = u16::from_le_bytes([fixed[32], fixed[33]]) as u64;
        let mut name = vec![0_u8; name_len];
        reader.read_exact(&mut name)?;
        names.push(name);
        reader.seek(SeekFrom::Current((extra_len + comment_len) as i64))?;
    }
    if reader.stream_position()? != central_end {
        return Err(PackageError::Archive(
            "ZIP central directory size does not match its entries".to_owned(),
        ));
    }
    Ok(names)
}

fn validate_central_names(names: &[Vec<u8>]) -> Result<(), PackageError> {
    let mut unique = HashSet::with_capacity(names.len());
    for raw in names {
        let name = std::str::from_utf8(raw)
            .map_err(|_| PackageError::UnsafeEntry(String::from_utf8_lossy(raw).into_owned()))?;
        validate_safe_name(name)?;
        if !unique.insert(name) {
            return Err(PackageError::DuplicateEntry(name.to_owned()));
        }
    }
    Ok(())
}

fn validate_safe_name(name: &str) -> Result<(), PackageError> {
    let drive_style = name.as_bytes().get(1) == Some(&b':')
        && name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic);
    if name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || drive_style
        || name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(PackageError::UnsafeEntry(name.to_owned()));
    }
    Ok(())
}

fn inspect_metadata<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
) -> Result<Vec<EntryMetadata>, PackageError> {
    let mut metadata = Vec::with_capacity(archive.len());
    let mut declared_total = 0_u64;
    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index)?;
        let name = std::str::from_utf8(entry.name_raw())
            .map_err(|_| {
                PackageError::UnsafeEntry(String::from_utf8_lossy(entry.name_raw()).into_owned())
            })?
            .to_owned();
        if entry.encrypted() {
            return Err(PackageError::EncryptedEntry(name));
        }
        if !entry.is_file() {
            return Err(PackageError::UnsafeEntry(name));
        }
        let compression = entry.compression();
        if !matches!(
            compression,
            CompressionMethod::Stored | CompressionMethod::Deflated
        ) {
            return Err(PackageError::UnsupportedCompression {
                entry: name,
                method: format!("{compression:?}"),
            });
        }
        declared_total = declared_total.saturating_add(entry.size());
        if declared_total > MAX_EXPANDED_SIZE {
            return Err(PackageError::ExpandedSizeLimit {
                actual: declared_total,
                limit: MAX_EXPANDED_SIZE,
            });
        }
        metadata.push(EntryMetadata {
            name,
            declared_size: entry.size(),
            compression,
            crc32: entry.crc32(),
        });
    }
    Ok(metadata)
}

fn read_all_bounded<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    metadata: &[EntryMetadata],
) -> Result<HashMap<String, Vec<u8>>, PackageError> {
    let mut total = 0_u64;
    let mut contents = HashMap::with_capacity(metadata.len());
    for (index, data) in metadata.iter().enumerate() {
        let raw = archive.by_index_raw(index)?;
        let mut expanded: Box<dyn Read + '_> = match data.compression {
            CompressionMethod::Stored => Box::new(raw),
            CompressionMethod::Deflated => Box::new(DeflateDecoder::new(raw)),
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
