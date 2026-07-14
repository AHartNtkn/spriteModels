use std::{
    fs::File,
    io::{BufReader, Cursor, Read, Seek},
    path::Path,
};

use png::{BitDepth, ColorType, Decoder, Transformations};
use relief_core::{CanonicalView, Chart, ChartError};
use zip::ZipArchive;

use crate::{DepthSpriteModel, ManifestV1, PackageError};

pub fn load_path(path: impl AsRef<Path>) -> Result<DepthSpriteModel, PackageError> {
    load_reader(BufReader::new(File::open(path)?))
}

pub fn load_reader<R: Read + Seek>(reader: R) -> Result<DepthSpriteModel, PackageError> {
    let mut archive = ZipArchive::new(reader)?;
    let manifest_bytes = read_entry(&mut archive, "manifest.json")?;
    let manifest: ManifestV1 = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| PackageError::Manifest(error.to_string()))?;
    let bounds = manifest.validate()?;

    let mut charts = Vec::with_capacity(manifest.views.len());
    for view_name in manifest.views {
        let entry = view_name.entry_name();
        let bytes = read_entry(&mut archive, entry)?;
        charts.push(decode_chart(entry, &bytes, bounds, view_name.into())?);
    }
    DepthSpriteModel::new(bounds, charts)
}

fn read_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>, PackageError> {
    let mut file = archive
        .by_name(name)
        .map_err(|_| PackageError::MissingEntry(name.to_owned()))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
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
