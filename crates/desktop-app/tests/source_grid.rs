use std::{fs::File, path::Path};

use desktop_app::{
    layout::CANONICAL_SOURCE_ORDER,
    source_grid::{
        SlotMode, add_next_source, card_header, remove_source, replace_source_from_png, slot_modes,
    },
};
use editor_core::EditorDocument;
use png::{BitDepth, ColorType, Encoder};
use relief_core::{Bounds, CanonicalView};
use tempfile::tempdir;

const FRONT: CanonicalView = CanonicalView::Front;

fn document() -> EditorDocument {
    EditorDocument::new(Bounds::new(2, 1, 1).unwrap(), FRONT)
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
fn only_the_next_empty_canonical_position_offers_add_sprite() {
    let mut document = document();
    assert_eq!(
        slot_modes(&document),
        [
            SlotMode::Authored,
            SlotMode::AddSprite,
            SlotMode::Hidden,
            SlotMode::Hidden,
            SlotMode::Hidden,
            SlotMode::Hidden,
        ]
    );

    add_next_source(&mut document).unwrap();
    assert_eq!(
        slot_modes(&document),
        [
            SlotMode::Authored,
            SlotMode::Authored,
            SlotMode::AddSprite,
            SlotMode::Hidden,
            SlotMode::Hidden,
            SlotMode::Hidden,
        ]
    );
    assert_eq!(
        document
            .sources()
            .map(|source| source.view())
            .collect::<Vec<_>>(),
        vec![CANONICAL_SOURCE_ORDER[0], CANONICAL_SOURCE_ORDER[1]]
    );
}

#[test]
fn add_import_replace_and_remove_are_undoable_document_commands() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("replacement.png");
    let replacement = [[11, 22, 33, 44], [55, 66, 77, 88]];
    write_png(&path, &replacement);
    let mut document = document();

    let added = add_next_source(&mut document).unwrap();
    assert_eq!(added, CanonicalView::Right);
    assert!(document.source(added).is_some());
    assert!(document.undo());
    assert!(document.source(added).is_none());

    replace_source_from_png(&mut document, FRONT, &path).unwrap();
    assert_eq!(document.source(FRONT).unwrap().rgba(), replacement);
    assert!(document.undo());
    assert_eq!(document.source(FRONT).unwrap().rgba(), &[[0, 0, 0, 0]; 2]);

    add_next_source(&mut document).unwrap();
    remove_source(&mut document, CanonicalView::Right).unwrap();
    assert!(document.source(CanonicalView::Right).is_none());
    assert!(document.undo());
    assert!(document.source(CanonicalView::Right).is_some());
}

#[test]
fn headers_report_fallback_assignment_and_update_after_override() {
    let mut document = document();

    let fallback = card_header(&document, FRONT).unwrap();
    assert_eq!(fallback.label, "Front → Back");

    document.add_source(CanonicalView::Back).unwrap();
    let overridden = card_header(&document, FRONT).unwrap();
    assert_eq!(overridden.label, "Front");
    assert_eq!(
        card_header(&document, CanonicalView::Back).unwrap().label,
        "Back"
    );
}
