use std::{io, path::PathBuf};

use relief_core::{Bounds, CanonicalView, ChartError};
use thiserror::Error;

use crate::CanonicalViewName;

#[derive(Debug, Error)]
pub enum PackageError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("invalid ZIP archive: {0}")]
    Archive(String),
    #[error("archive contains {actual} entries; the limit is {limit}")]
    EntryLimit { actual: usize, limit: usize },
    #[error("archive expanded data is {actual} bytes; the limit is {limit}")]
    ExpandedSizeLimit { actual: u64, limit: u64 },
    #[error("archive input data is {actual} bytes; the limit is {limit}")]
    InputSizeLimit { actual: u64, limit: u64 },
    #[error("unsafe or non-normalized archive entry: {0}")]
    UnsafeEntry(String),
    #[error("duplicate archive entry: {0}")]
    DuplicateEntry(String),
    #[error("missing archive entry: {0}")]
    MissingEntry(String),
    #[error("undeclared archive entry: {0}")]
    UndeclaredEntry(String),
    #[error("unsupported compression for archive entry {entry}: {method}")]
    UnsupportedCompression { entry: String, method: String },
    #[error("encrypted archive entry is not supported: {0}")]
    EncryptedEntry(String),
    #[error("archive entry {entry} declares {declared} bytes but expands to {actual} bytes")]
    SizeMismatch {
        entry: String,
        declared: u64,
        actual: u64,
    },
    #[error("archive entry failed its CRC check: {0}")]
    Integrity(String),
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
    #[error("model bounds must be nonzero and at most 512 on each axis, got {0:?}")]
    InvalidBounds([u32; 3]),
    #[error("model must contain between one and six views, got {0}")]
    ViewCount(usize),
    #[error("model contains duplicate view {0:?}")]
    DuplicateView(CanonicalViewName),
    #[error("model contains no charts")]
    EmptyModel,
    #[error("chart bounds {actual:?} do not match model bounds {expected:?}")]
    MixedBounds { expected: Bounds, actual: Bounds },
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
    #[error("invalid chart for {view:?}: {source}")]
    InvalidChart {
        view: CanonicalView,
        #[source]
        source: ChartError,
    },
}

impl From<zip::result::ZipError> for PackageError {
    fn from(error: zip::result::ZipError) -> Self {
        Self::Archive(error.to_string())
    }
}
