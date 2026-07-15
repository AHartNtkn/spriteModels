use std::{
    ffi::OsString,
    fs::OpenOptions,
    io::{BufWriter, Seek, Write},
    path::{Path, PathBuf},
};

use png::{BitDepth, ColorType, Compression, Encoder, Filter};
use relief_core::{AuthoredModel, Chart};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use crate::{CanonicalViewName, ManifestV1, PackageError, SourceV1};

pub fn save_writer<W: Write + Seek>(
    model: &AuthoredModel,
    writer: &mut W,
) -> Result<(), PackageError> {
    let mut archive = ZipWriter::new(writer);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let bounds = model.bounds();
    let manifest = ManifestV1 {
        format: "depthsprite".to_owned(),
        version: 1,
        bounds_pixels: [bounds.width(), bounds.height(), bounds.depth()],
        sources: model
            .charts()
            .iter()
            .map(|chart| SourceV1 {
                view: chart.view().into(),
                opposite: chart.supplies_opposite(),
                mirror: chart.mirrors_opposite(),
            })
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
    model: &AuthoredModel,
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
    if let Err(operation) = before_replace {
        return Err(report_cleanup_result(
            &temporary,
            operation,
            std::fs::remove_file(&temporary),
        ));
    }

    Ok(())
}

fn encode_chart(chart: &Chart, entry: &str) -> Result<Vec<u8>, PackageError> {
    let (width, height) = chart.dimensions();
    let mut rgba = Vec::with_capacity(chart.rgba().len() * 4);
    for pixel in chart.rgba() {
        rgba.extend_from_slice(pixel);
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

fn report_cleanup_result(
    temporary: &Path,
    operation: PackageError,
    cleanup: std::io::Result<()>,
) -> PackageError {
    match cleanup {
        Ok(()) => operation,
        Err(source) => PackageError::TempCleanup {
            path: temporary.to_owned(),
            operation: operation.to_string(),
            source,
        },
    }
}

fn replace_file(source: &Path, destination: &Path) -> Result<(), PackageError> {
    std::fs::rename(source, destination)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{io, path::Path};

    use crate::PackageError;

    use super::report_cleanup_result;

    #[test]
    fn cleanup_failure_reports_stranded_temp_and_original_operation() {
        let operation = PackageError::Io(io::Error::other("archive write failed"));
        let error = report_cleanup_result(
            Path::new("sprite.depthsprite.tmp"),
            operation,
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "temp removal denied",
            )),
        );

        match error {
            PackageError::TempCleanup {
                path,
                operation,
                source,
            } => {
                assert_eq!(path, Path::new("sprite.depthsprite.tmp"));
                assert!(operation.to_string().contains("archive write failed"));
                assert_eq!(source.kind(), io::ErrorKind::PermissionDenied);
            }
            other => panic!("expected visible temp cleanup error, got {other:?}"),
        }
    }
}
