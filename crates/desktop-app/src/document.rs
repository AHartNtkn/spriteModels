use std::{
    ffi::OsString,
    fs::OpenOptions,
    io::{BufWriter, Cursor, Write},
    path::{Path, PathBuf},
};

use depthsprite_format::{
    DepthSpriteModel, PackageError, load_path, save_path_atomic, save_writer,
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
}

pub struct Document {
    model: DepthSpriteModel,
    path: PathBuf,
    display_name: String,
    model_hash: u32,
}

impl Document {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DocumentError> {
        let path = path.as_ref();
        let model = load_path(path)?;
        Self::from_loaded(model, path)
    }

    pub fn replace_from_path(&mut self, path: impl AsRef<Path>) -> Result<(), DocumentError> {
        let replacement = Self::open(path)?;
        *self = replacement;
        Ok(())
    }

    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<(), DocumentError> {
        let path = path.as_ref();
        save_path_atomic(&self.model, path)?;
        self.path = path.to_owned();
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
        Some(&self.path)
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn model_hash(&self) -> u32 {
        self.model_hash
    }

    fn from_loaded(model: DepthSpriteModel, path: &Path) -> Result<Self, DocumentError> {
        let model_hash = canonical_hash(&model)?;
        Ok(Self {
            model,
            path: path.to_owned(),
            display_name: display_name(path),
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
    let temporary = temporary_path(destination);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;

    let before_replace = (|| -> Result<(), DocumentError> {
        let mut writer = BufWriter::new(file);
        writer.write_all(bytes)?;
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

    sync_parent(destination)?;
    Ok(())
}

fn temporary_path(destination: &Path) -> PathBuf {
    let mut name: OsString = destination.as_os_str().to_owned();
    name.push(".tmp");
    PathBuf::from(name)
}

fn report_cleanup_result(
    temporary: &Path,
    operation: DocumentError,
    cleanup: std::io::Result<()>,
) -> DocumentError {
    match cleanup {
        Ok(()) => operation,
        Err(source) => DocumentError::TempCleanup {
            path: temporary.to_owned(),
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
