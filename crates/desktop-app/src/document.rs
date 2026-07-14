use std::{
    io::{Cursor, Write},
    path::{Path, PathBuf},
};

use depthsprite_format::{
    DepthSpriteModel, PackageError, load_path, load_reader, save_path_atomic, save_writer,
};
use relief_render::{SheetError, SheetRequest, encode_png, render_sheet};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DocumentError {
    #[error(transparent)]
    Package(#[from] PackageError),
    #[error(transparent)]
    Sheet(#[from] SheetError),
    #[error("failed to encode export PNG: {0}")]
    Png(#[from] png::EncodingError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to clean up export temporary file {path:?} after {operation}: {source}")]
    TempCleanup {
        path: PathBuf,
        operation: String,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "destination replacement completed for {path:?}, but parent durability sync failed: {source}"
    )]
    PostReplaceSync {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Clone)]
pub struct Document {
    model: DepthSpriteModel,
    path: Option<PathBuf>,
    display_name: String,
    model_hash: u32,
}

impl Document {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DocumentError> {
        let path = path.as_ref();
        let model = load_path(path)?;
        Self::from_loaded(model, Some(path.to_owned()), display_name(path))
    }

    pub fn from_bundled(display_name: &str, bytes: &'static [u8]) -> Result<Self, DocumentError> {
        let model = load_reader(Cursor::new(bytes))?;
        Self::from_loaded(model, None, display_name.to_owned())
    }

    pub fn replace_from_path(&mut self, path: impl AsRef<Path>) -> Result<(), DocumentError> {
        let replacement = Self::open(path)?;
        *self = replacement;
        Ok(())
    }

    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<(), DocumentError> {
        let path = path.as_ref();
        save_path_atomic(&self.model, path)?;
        self.path = Some(path.to_owned());
        self.display_name = display_name(path);
        Ok(())
    }

    pub fn export_sheet(
        &self,
        path: impl AsRef<Path>,
        request: &SheetRequest,
    ) -> Result<(), DocumentError> {
        let frame = render_sheet(self.model.charts(), self.model.bounds(), request)?;
        let bytes = encode_png(&frame)?;
        write_bytes_atomic(path.as_ref(), &bytes)
    }

    pub fn model(&self) -> &DepthSpriteModel {
        &self.model
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn model_hash(&self) -> u32 {
        self.model_hash
    }

    fn from_loaded(
        model: DepthSpriteModel,
        path: Option<PathBuf>,
        display_name: String,
    ) -> Result<Self, DocumentError> {
        let model_hash = canonical_hash(&model)?;
        Ok(Self {
            model,
            path,
            display_name,
            model_hash,
        })
    }
}

fn canonical_hash(model: &DepthSpriteModel) -> Result<u32, PackageError> {
    let mut bytes = Cursor::new(Vec::new());
    save_writer(model, &mut bytes)?;
    Ok(crc32fast::hash(bytes.get_ref()))
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

fn write_bytes_atomic(destination: &Path, bytes: &[u8]) -> Result<(), DocumentError> {
    write_bytes_atomic_with(destination, bytes, replace_file, sync_parent)
}

fn write_bytes_atomic_with(
    destination: &Path,
    bytes: &[u8],
    replace: impl FnOnce(&Path, &Path) -> Result<(), DocumentError>,
    sync_destination_parent: impl FnOnce(&Path) -> Result<(), DocumentError>,
) -> Result<(), DocumentError> {
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::Builder::new()
        .prefix(".depthsprite-export-")
        .tempfile_in(parent)?;
    let write_result = (|| -> Result<(), DocumentError> {
        temporary.write_all(bytes)?;
        temporary.flush()?;
        temporary.as_file().sync_all()?;
        Ok(())
    })();
    let (file, temporary_path) = temporary.into_parts();
    drop(file);
    if let Err(operation) = write_result {
        return Err(report_cleanup_result(temporary_path, operation));
    }
    if let Err(operation) = replace(temporary_path.as_ref(), destination) {
        return Err(report_cleanup_result(temporary_path, operation));
    }

    if let Err(error) = sync_destination_parent(destination) {
        let source = match error {
            DocumentError::Io(source) => source,
            other => std::io::Error::other(other.to_string()),
        };
        return Err(DocumentError::PostReplaceSync {
            path: destination.to_owned(),
            source,
        });
    }
    Ok(())
}

fn report_cleanup_result(temporary: tempfile::TempPath, operation: DocumentError) -> DocumentError {
    let path = temporary.to_path_buf();
    match temporary.close() {
        Ok(()) => operation,
        Err(source) => DocumentError::TempCleanup {
            path,
            operation: operation.to_string(),
            source,
        },
    }
}

#[cfg(unix)]
fn replace_file(source: &Path, destination: &Path) -> Result<(), DocumentError> {
    std::fs::rename(source, destination)?;
    Ok(())
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> Result<(), DocumentError> {
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
    // SAFETY: Both paths are owned, NUL-terminated UTF-16 buffers alive for the call.
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
fn sync_parent(destination: &Path) -> Result<(), DocumentError> {
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
fn sync_parent(_destination: &Path) -> Result<(), DocumentError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, io};

    use tempfile::tempdir;

    use super::{DocumentError, write_bytes_atomic_with};

    #[test]
    fn pre_replace_failure_preserves_destination_and_cleans_owned_temporary() {
        let temp = tempdir().unwrap();
        let destination = temp.path().join("sheet.png");
        fs::write(&destination, b"old").unwrap();

        let error = write_bytes_atomic_with(
            &destination,
            b"new",
            |_source, _destination| Err(io::Error::other("replace denied").into()),
            |_destination| Ok(()),
        )
        .unwrap_err();

        assert!(error.to_string().contains("replace denied"));
        assert_eq!(fs::read(&destination).unwrap(), b"old");
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    }

    #[test]
    fn post_replace_sync_failure_says_destination_was_replaced() {
        let temp = tempdir().unwrap();
        let destination = temp.path().join("sheet.png");
        fs::write(&destination, b"old").unwrap();

        let error = write_bytes_atomic_with(
            &destination,
            b"new",
            |source, destination| {
                fs::rename(source, destination)?;
                Ok(())
            },
            |_destination| Err(io::Error::other("directory sync denied").into()),
        )
        .unwrap_err();

        assert!(matches!(error, DocumentError::PostReplaceSync { .. }));
        assert!(error.to_string().contains("replacement completed"));
        assert_eq!(fs::read(&destination).unwrap(), b"new");
    }
}
