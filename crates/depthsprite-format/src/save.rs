use std::{
    ffi::OsString,
    fs::OpenOptions,
    io::{BufWriter, Seek, Write},
    path::{Path, PathBuf},
};

use png::{BitDepth, ColorType, Compression, Encoder, Filter};
use relief_core::{Chart, DecodedTexel};
use zip::{CompressionMethod, DateTime, ZipWriter, write::SimpleFileOptions};

use crate::{CanonicalViewName, DepthSpriteModel, ManifestV1, PackageError};

pub fn save_writer<W: Write + Seek>(
    model: &DepthSpriteModel,
    writer: &mut W,
) -> Result<(), PackageError> {
    let mut archive = ZipWriter::new(writer);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644);

    let bounds = model.bounds();
    let manifest = ManifestV1 {
        format: "depthsprite".to_owned(),
        version: 1,
        bounds_pixels: [bounds.width(), bounds.height(), bounds.depth()],
        views: model
            .charts()
            .iter()
            .map(|chart| chart.view().into())
            .collect(),
    };
    let mut manifest_bytes =
        serde_json::to_vec(&manifest).map_err(|error| PackageError::Manifest(error.to_string()))?;
    manifest_bytes.push(b'\n');
    archive.start_file("manifest.json", options)?;
    archive.write_all(&manifest_bytes)?;

    for chart in model.charts() {
        let view: CanonicalViewName = chart.view().into();
        archive.start_file(view.entry_name(), options)?;
        archive.write_all(&encode_chart(chart, view.entry_name())?)?;
    }
    archive.finish()?;
    Ok(())
}

pub fn save_path_atomic(
    model: &DepthSpriteModel,
    destination: impl AsRef<Path>,
) -> Result<(), PackageError> {
    let destination = destination.as_ref();
    let temporary = temporary_path(destination);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;

    let before_replace = (|| -> Result<(), PackageError> {
        let mut writer = BufWriter::new(file);
        save_writer(model, &mut writer)?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        drop(writer);
        replace_file(&temporary, destination)?;
        Ok(())
    })();
    if before_replace.is_err() {
        let _ = std::fs::remove_file(&temporary);
        return before_replace;
    }

    sync_parent(destination)?;
    Ok(())
}

fn encode_chart(chart: &Chart, entry: &str) -> Result<Vec<u8>, PackageError> {
    let (width, height) = chart.dimensions();
    let mut rgba = Vec::with_capacity(chart.texels().len() * 4);
    for texel in chart.texels() {
        match texel {
            DecodedTexel::Background => rgba.extend_from_slice(&[0, 0, 0, 0]),
            DecodedTexel::Relief { rgb, eighths } => {
                rgba.extend_from_slice(rgb);
                rgba.push(255 - eighths);
            }
        }
    }

    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes, width, height);
    encoder.set_color(ColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);
    encoder.set_compression(Compression::High);
    encoder.set_filter(Filter::Paeth);
    let mut writer = encoder
        .write_header()
        .map_err(|error| PackageError::InvalidPng {
            entry: entry.to_owned(),
            message: error.to_string(),
        })?;
    writer
        .write_image_data(&rgba)
        .map_err(|error| PackageError::InvalidPng {
            entry: entry.to_owned(),
            message: error.to_string(),
        })?;
    writer.finish().map_err(|error| PackageError::InvalidPng {
        entry: entry.to_owned(),
        message: error.to_string(),
    })?;
    Ok(bytes)
}

fn temporary_path(destination: &Path) -> PathBuf {
    let mut name: OsString = destination.as_os_str().to_owned();
    name.push(".tmp");
    PathBuf::from(name)
}

#[cfg(unix)]
fn replace_file(source: &Path, destination: &Path) -> Result<(), PackageError> {
    std::fs::rename(source, destination)?;
    Ok(())
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> Result<(), PackageError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source_wide: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination_wide: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: Both paths are owned, NUL-terminated UTF-16 buffers that remain alive for the call.
    let result = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(unix)]
fn sync_parent(destination: &Path) -> Result<(), PackageError> {
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty());
    match std::fs::File::open(parent.unwrap_or_else(|| Path::new(".")))?.sync_all() {
        Ok(()) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::InvalidInput | std::io::ErrorKind::Unsupported
            ) => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

#[cfg(windows)]
fn sync_parent(_destination: &Path) -> Result<(), PackageError> {
    // MOVEFILE_WRITE_THROUGH flushes the move operation; Windows does not offer a portable
    // directory-handle equivalent to Unix directory fsync through std.
    Ok(())
}
