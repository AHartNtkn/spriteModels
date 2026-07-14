use std::{fs::File, path::Path};

use desktop_app::source_grid::{add_source, card_header, remove_source, replace_source_from_png};
use editor_core::EditorDocument;
use png::{BitDepth, ColorType, Encoder};
use relief_core::{Bounds, CanonicalView, EMPTY_RGBA};
use tempfile::tempdir;

const FRONT: CanonicalView = CanonicalView::Front;

fn document() -> EditorDocument {
    EditorDocument::new(Bounds::new(2, 1, 63).unwrap(), FRONT)
}

fn write_png(path: &Path, pixels: &[[u8; 4]]) {
    let bytes = pixels
        .iter()
        .flat_map(|pixel| pixel.iter().copied())
        .collect::<Vec<_>>();
    let mut encoder = Encoder::new(File::create(path).unwrap(), pixels.len() as u32, 1);
    encoder.set_color(ColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&bytes).unwrap();
    writer.finish().unwrap();
}

#[test]
fn add_action_accepts_an_explicit_unoccupied_side() {
    let mut document = document();
    add_source(&mut document, CanonicalView::Back).unwrap();

    assert!(document.source(CanonicalView::Back).is_some());
    assert!(document.source(CanonicalView::Right).is_none());
}

#[test]
fn add_import_replace_and_remove_are_undoable_document_commands() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("replacement.png");
    let replacement = [[11, 22, 33, 44], [55, 66, 77, 88]];
    write_png(&path, &replacement);
    let mut document = document();

    add_source(&mut document, CanonicalView::Back).unwrap();
    assert!(document.source(CanonicalView::Back).is_some());
    assert!(document.undo());
    assert!(document.source(CanonicalView::Back).is_none());

    replace_source_from_png(&mut document, FRONT, &path).unwrap();
    assert_eq!(document.source(FRONT).unwrap().rgba(), replacement);
    assert!(document.undo());
    assert_eq!(document.source(FRONT).unwrap().rgba(), &[EMPTY_RGBA; 2]);

    add_source(&mut document, CanonicalView::Back).unwrap();
    remove_source(&mut document, CanonicalView::Back).unwrap();
    assert!(document.source(CanonicalView::Back).is_none());
    assert!(document.undo());
    assert!(document.source(CanonicalView::Back).is_some());
}

#[test]
fn card_headers_only_name_the_assigned_side() {
    let document = document();

    assert_eq!(
        card_header(&document, CanonicalView::Front).unwrap().label,
        "Front"
    );
}
