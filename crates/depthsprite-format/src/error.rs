use std::{io, path::PathBuf};

use relief_core::ModelError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PackageError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("invalid ZIP archive: {0}")]
    Archive(String),
    #[error("missing archive entry: {0}")]
    MissingEntry(String),
    #[error("failed to clean up stranded temporary package {path:?} after {operation}: {source}")]
    TempCleanup {
        path: PathBuf,
        operation: String,
        #[source]
        source: io::Error,
    },
    #[error("invalid manifest: {0}")]
    Manifest(String),
    #[error("expected manifest format 'depthsprite', got {0:?}")]
    WrongFormat(String),
    #[error("unsupported manifest version {0}")]
    UnsupportedVersion(u32),
    #[error(
        "entry {entry} must encode nonpremultiplied 8-bit RGBA PNG, got {color_type} {bit_depth}"
    )]
    InvalidPngType {
        entry: String,
        color_type: String,
        bit_depth: String,
    },
    #[error("invalid PNG in {entry}: {message}")]
    InvalidPng { entry: String, message: String },
    #[error(transparent)]
    Model(#[from] ModelError),
}

impl From<zip::result::ZipError> for PackageError {
    fn from(error: zip::result::ZipError) -> Self {
        Self::Archive(error.to_string())
    }
}
