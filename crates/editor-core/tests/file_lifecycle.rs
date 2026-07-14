use std::{fs::File, path::Path};

use editor_core::{EditorDocument, EditorError};
use png::{BitDepth, ColorType, Encoder};
use relief_core::{Bounds, CanonicalView, ModelError};
use tempfile::tempdir;

const VIEW: CanonicalView = CanonicalView::Front;

fn write_rgba_png(path: &Path, width: u32, height: u32, pixels: &[[u8; 4]]) {
    let bytes: Vec<_> = pixels
        .iter()
        .flat_map(|pixel| pixel.iter().copied())
        .collect();
    let mut encoder = Encoder::new(File::create(path).unwrap(), width, height);
    encoder.set_color(ColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&bytes).unwrap();
    writer.finish().unwrap();
}

fn pixels(document: &EditorDocument) -> Vec<[u8; 4]> {
    document.source(VIEW).unwrap().rgba().to_vec()
}

#[test]
fn wrong_source_dimensions_leave_the_installed_document_unchanged() {
    let directory = tempdir().unwrap();
    let source_path = directory.path().join("wrong-size.png");
    write_rgba_png(&source_path, 1, 1, &[[9, 8, 7, 6]]);
    let mut document = EditorDocument::new(Bounds::new(2, 1, 1).unwrap(), VIEW);
    let before_pixels = pixels(&document);
    let before_path = document.path().map(Path::to_owned);
    let before_revision = document.revision();

    assert!(document.import_source_png(VIEW, &source_path).is_err());

    assert_eq!(pixels(&document), before_pixels);
    assert_eq!(document.path(), before_path.as_deref());
    assert_eq!(document.revision(), before_revision);
    assert!(!document.is_dirty());
    assert!(!document.can_undo());
    assert!(!document.can_redo());
}

#[test]
fn excessive_relief_import_leaves_model_and_history_unchanged() {
    let directory = tempdir().unwrap();
    let source_path = directory.path().join("too-deep.png");
    write_rgba_png(&source_path, 1, 1, &[[9, 8, 7, 250]]);
    let mut document = EditorDocument::new(Bounds::new(1, 1, 1).unwrap(), VIEW);
    document.add_source(CanonicalView::Back).unwrap();
    assert!(document.undo());
    let before_model = document.to_model();
    let before_bounds = document.bounds();
    let before_rgba = pixels(&document);
    let before_revision = document.revision();
    let before_dirty = document.is_dirty();
    let before_can_undo = document.can_undo();
    let before_can_redo = document.can_redo();

    let error = document.import_source_png(VIEW, &source_path).unwrap_err();

    assert!(matches!(
        error,
        EditorError::Model(ModelError::ReliefBeyondMaximum {
            view: CanonicalView::Front,
            x: 0,
            y: 0,
            actual: 5,
            maximum: 4,
        })
    ));
    assert_eq!(document.to_model(), before_model);
    assert_eq!(document.bounds(), before_bounds);
    assert_eq!(pixels(&document), before_rgba);
    assert_eq!(document.revision(), before_revision);
    assert_eq!(document.is_dirty(), before_dirty);
    assert_eq!(document.can_undo(), before_can_undo);
    assert_eq!(document.can_redo(), before_can_redo);
}

#[test]
fn failed_open_does_not_replace_an_installed_document() {
    let directory = tempdir().unwrap();
    let package_path = directory.path().join("installed.depthsprite");
    let mut document = EditorDocument::new(Bounds::new(1, 1, 1).unwrap(), VIEW);
    document.save_as(&package_path).unwrap();
    let before_pixels = pixels(&document);
    let before_path = document.path().map(Path::to_owned);
    let before_dirty = document.is_dirty();

    assert!(EditorDocument::open(directory.path().join("invalid.depthsprite")).is_err());

    assert_eq!(pixels(&document), before_pixels);
    assert_eq!(document.path(), before_path.as_deref());
    assert_eq!(document.is_dirty(), before_dirty);
}

#[test]
fn save_and_reopen_preserve_exact_authored_bytes_and_mark_the_document_clean() {
    let directory = tempdir().unwrap();
    let source_path = directory.path().join("source.png");
    let package_path = directory.path().join("sprite.depthsprite");
    let authored = [[17, 31, 47, 0], [99, 88, 77, 193]];
    write_rgba_png(&source_path, 2, 1, &authored);
    let mut document = EditorDocument::new(Bounds::new(2, 1, 63).unwrap(), VIEW);

    document.import_source_png(VIEW, &source_path).unwrap();
    assert!(document.is_dirty());
    document.save_as(&package_path).unwrap();

    assert_eq!(document.path(), Some(package_path.as_path()));
    assert!(!document.is_dirty());
    let reopened = EditorDocument::open(&package_path).unwrap();
    assert_eq!(pixels(&reopened), authored);
    assert_eq!(reopened.path(), Some(package_path.as_path()));
    assert!(!reopened.is_dirty());
}

#[test]
fn failed_save_as_preserves_the_previous_path_and_saved_state() {
    let directory = tempdir().unwrap();
    let first_source = directory.path().join("first.png");
    let second_source = directory.path().join("second.png");
    let installed_path = directory.path().join("installed.depthsprite");
    write_rgba_png(&first_source, 1, 1, &[[1, 2, 3, 251]]);
    write_rgba_png(&second_source, 1, 1, &[[5, 6, 7, 252]]);
    let mut document = EditorDocument::new(Bounds::new(1, 1, 1).unwrap(), VIEW);
    document.import_source_png(VIEW, &first_source).unwrap();
    document.save_as(&installed_path).unwrap();
    document.import_source_png(VIEW, &second_source).unwrap();
    assert!(document.is_dirty());

    let invalid_path = directory.path().join("missing").join("failed.depthsprite");
    assert!(document.save_as(&invalid_path).is_err());

    assert_eq!(document.path(), Some(installed_path.as_path()));
    assert!(document.is_dirty());
    assert_eq!(pixels(&document), [[5, 6, 7, 252]]);
    document.save().unwrap();
    assert!(!document.is_dirty());
    assert_eq!(
        pixels(&EditorDocument::open(&installed_path).unwrap()),
        [[5, 6, 7, 252]]
    );
}

#[test]
fn save_without_a_path_fails_without_changing_document_state() {
    let mut document = EditorDocument::new(Bounds::new(1, 1, 1).unwrap(), VIEW);
    let before_pixels = pixels(&document);
    let before_revision = document.revision();

    assert!(document.save().is_err());

    assert_eq!(document.path(), None);
    assert_eq!(pixels(&document), before_pixels);
    assert_eq!(document.revision(), before_revision);
    assert!(!document.is_dirty());
}
