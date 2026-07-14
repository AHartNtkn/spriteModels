use std::{fs, io::Cursor, path::PathBuf};

use desktop_app::document::Document;
use relief_core::{Bounds, CanonicalView, DecodedTexel};
use relief_render::{DirectionCount, RenderRequest, SheetRequest, TargetView, render_model};
use zip::ZipArchive;

const FRONT_RGB: [u8; 3] = [144, 76, 52];
const TOP_RGB: [u8; 3] = [216, 156, 85];

fn bowl_asset() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/examples/bowl.depthsprite")
}

fn assert_exact_bowl_package(bytes: Vec<u8>) {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    assert_eq!(archive.len(), 3);
    let entries = (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().name().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        entries,
        vec!["manifest.json", "views/front.png", "views/top.png"]
    );
}

fn assert_critical_only_rgba8_png(bytes: &[u8]) {
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let mut offset = 8;
    let mut chunks = Vec::new();
    while offset < bytes.len() {
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = bytes[offset + 4..offset + 8].try_into().unwrap();
        let data = &bytes[offset + 8..offset + 8 + length];
        chunks.push((kind, data));
        offset += 12 + length;
    }
    assert_eq!(offset, bytes.len());
    assert_eq!(chunks.first().unwrap().0, *b"IHDR");
    assert_eq!(chunks.last().unwrap().0, *b"IEND");
    assert!(
        chunks
            .iter()
            .all(|(kind, _)| matches!(kind, b"IHDR" | b"IDAT" | b"IEND"))
    );
    let ihdr = chunks[0].1;
    assert_eq!(u32::from_be_bytes(ihdr[0..4].try_into().unwrap()), 1600);
    assert_eq!(u32::from_be_bytes(ihdr[4..8].try_into().unwrap()), 100);
    assert_eq!(ihdr[8], 8);
    assert_eq!(ihdr[9], 6);
}

#[test]
fn bowl_open_render_save_reopen_export_is_reproducible() {
    let temp = tempfile::tempdir().unwrap();
    let model_path = temp.path().join("bowl-copy.depthsprite");
    let first_sheet = temp.path().join("first.png");
    let second_sheet = temp.path().join("second.png");
    let original_bytes = fs::read(bowl_asset()).unwrap();
    assert_exact_bowl_package(original_bytes.clone());

    let mut document = Document::open(bowl_asset()).unwrap();
    assert_eq!(document.model().bounds(), Bounds::new(32, 16, 32).unwrap());
    assert_eq!(
        document
            .model()
            .charts()
            .iter()
            .map(|chart| chart.view())
            .collect::<Vec<_>>(),
        vec![CanonicalView::Front, CanonicalView::Top]
    );

    let frame = render_model(
        document.model().charts(),
        &RenderRequest::new(96, 96, TargetView::bowl_acceptance()),
    )
    .unwrap();
    let rim = frame.owner_at(48, 67).expect("rounded Front rim");
    let basin = frame.owner_at(48, 48).expect("central Top basin");
    assert_eq!(
        (rim.view, rim.source_x, rim.source_y),
        (CanonicalView::Front, 27, 2)
    );
    assert_eq!(
        (basin.view, basin.source_x, basin.source_y),
        (CanonicalView::Top, 16, 16)
    );
    assert_eq!(
        document.model().charts()[0].texel(rim.source_x, rim.source_y),
        Some(DecodedTexel::Relief {
            rgb: FRONT_RGB,
            eighths: 40,
        })
    );
    assert_eq!(
        document.model().charts()[1].texel(basin.source_x, basin.source_y),
        Some(DecodedTexel::Relief {
            rgb: TOP_RGB,
            eighths: 64,
        })
    );
    assert_eq!(frame.rgba_at(48, 67), [144, 76, 52, 255]);
    assert_eq!(frame.rgba_at(48, 48), [216, 156, 85, 255]);
    assert_eq!(frame.rgba_at(0, 0), [0, 0, 0, 0]);

    let original_hash = document.model_hash();
    document.save_as(&model_path).unwrap();
    let reopened = Document::open(&model_path).unwrap();
    assert_eq!(document.model_hash(), original_hash);
    assert_eq!(reopened.model_hash(), original_hash);
    let saved_bytes = fs::read(&model_path).unwrap();
    assert_eq!(saved_bytes, original_bytes);
    assert_exact_bowl_package(saved_bytes);

    let request = SheetRequest::new(DirectionCount::Sixteen, 1, 2, 1).unwrap();
    document.export_sheet(&first_sheet, &request).unwrap();
    reopened.export_sheet(&second_sheet, &request).unwrap();
    let first_png = fs::read(first_sheet).unwrap();
    let second_png = fs::read(second_sheet).unwrap();
    assert_eq!(first_png, second_png);
    assert_critical_only_rgba8_png(&first_png);
}
