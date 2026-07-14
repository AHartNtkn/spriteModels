use std::{collections::BTreeSet, fs, io::Cursor, path::PathBuf};

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

#[derive(Debug)]
struct FrameEvidence {
    front_positions: BTreeSet<(u32, u32)>,
    top_positions: BTreeSet<(u32, u32)>,
    front_reliefs: BTreeSet<u8>,
    top_reliefs: BTreeSet<u8>,
    colors: BTreeSet<[u8; 3]>,
    opaque_bounds: (u32, u32, u32, u32),
}

fn summarize_attached_evidence(
    document: &Document,
    frame: &relief_render::FrameBuffer,
) -> FrameEvidence {
    let mut front_positions = BTreeSet::new();
    let mut top_positions = BTreeSet::new();
    let mut front_reliefs = BTreeSet::new();
    let mut top_reliefs = BTreeSet::new();
    let mut colors = BTreeSet::new();
    let mut covered = Vec::new();

    for y in 0..frame.height() {
        for x in 0..frame.width() {
            let Some(owner) = frame.owner_at(x, y) else {
                continue;
            };
            covered.push((x, y));
            let chart = document
                .model()
                .charts()
                .iter()
                .find(|chart| chart.view() == owner.view)
                .expect("every rendered owner names an authored chart");
            let Some(DecodedTexel::Relief { rgb, eighths }) =
                chart.texel(owner.source_x, owner.source_y)
            else {
                panic!("every rendered owner names an authored foreground texel");
            };
            assert_eq!(frame.rgba_at(x, y), [rgb[0], rgb[1], rgb[2], 255]);
            colors.insert(rgb);
            match owner.view {
                CanonicalView::Front => {
                    front_positions.insert((owner.source_x, owner.source_y));
                    front_reliefs.insert(eighths);
                }
                CanonicalView::Top => {
                    top_positions.insert((owner.source_x, owner.source_y));
                    top_reliefs.insert(eighths);
                }
                other => panic!("the two-chart bowl cannot produce {other:?} ownership"),
            }
        }
    }

    FrameEvidence {
        front_positions,
        top_positions,
        front_reliefs,
        top_reliefs,
        colors,
        opaque_bounds: (
            covered.iter().map(|(x, _)| *x).min().unwrap(),
            covered.iter().map(|(_, y)| *y).min().unwrap(),
            covered.iter().map(|(x, _)| *x).max().unwrap(),
            covered.iter().map(|(_, y)| *y).max().unwrap(),
        ),
    }
}

#[test]
fn bowl_v1_sector_preserves_rounded_evidence_and_an_honest_inner_gap() {
    let document = Document::open(bowl_asset()).unwrap();
    let sheet = SheetRequest::new(DirectionCount::Sixteen, 1, 0, 1).unwrap();
    let mut evidence = Vec::new();

    // These are the three adjacent public v1 directions centered on Front. The exhaustive
    // fixture test proves every source radial/profile sample; this receipt proves that broad,
    // symmetric evidence survives real rendering rather than inferring a bowl from one pixel.
    for direction in [15, 0, 1] {
        let request = RenderRequest::new(96, 96, sheet.target_view(direction).unwrap());
        let first = render_model(document.model().charts(), &request).unwrap();
        let second = render_model(document.model().charts(), &request).unwrap();
        assert_eq!(
            first, second,
            "v1 direction {direction} must be deterministic"
        );

        let summary = summarize_attached_evidence(&document, &first);
        assert!(summary.front_positions.len() >= 80, "direction {direction}");
        assert!(summary.top_positions.len() >= 150, "direction {direction}");
        assert!(summary.front_reliefs.len() >= 9, "direction {direction}");
        assert!(
            [0, 24, 48, 60, 72]
                .into_iter()
                .all(|relief| summary.front_reliefs.contains(&relief)),
            "direction {direction}: {:?}",
            summary.front_reliefs
        );
        assert!(
            [0, 24, 40, 61]
                .into_iter()
                .all(|relief| summary.top_reliefs.contains(&relief)),
            "direction {direction}: {:?}",
            summary.top_reliefs
        );
        assert!(summary.top_reliefs.iter().copied().max().unwrap() >= 61);
        assert_eq!(summary.colors, BTreeSet::from([FRONT_RGB, TOP_RGB]));
        evidence.push((direction, first, summary));
    }

    let mirrored_front = evidence[0]
        .2
        .front_positions
        .iter()
        .map(|(x, y)| (31 - x, *y))
        .collect::<BTreeSet<_>>();
    let mirrored_top = evidence[0]
        .2
        .top_positions
        .iter()
        .map(|(x, y)| (31 - x, *y))
        .collect::<BTreeSet<_>>();
    assert_eq!(mirrored_front, evidence[2].2.front_positions);
    assert_eq!(mirrored_top, evidence[2].2.top_positions);

    let (_, right_frame, right_evidence) = &evidence[2];
    assert_eq!(right_evidence.opaque_bounds, (36, 39, 57, 55));
    let gap = (37, 44);
    let (min_x, min_y, max_x, max_y) = right_evidence.opaque_bounds;
    assert!(gap.0 > min_x && gap.0 < max_x && gap.1 > min_y && gap.1 < max_y);
    assert_eq!(right_frame.rgba_at(gap.0, gap.1), [0, 0, 0, 0]);
    assert_eq!(right_frame.owner_at(gap.0, gap.1), None);
    let left = right_frame.owner_at(gap.0 - 1, gap.1).unwrap();
    let right = right_frame.owner_at(gap.0 + 1, gap.1).unwrap();
    assert_eq!(
        (left.view, left.source_x, left.source_y),
        (CanonicalView::Front, 0, 2)
    );
    assert_eq!(
        (right.view, right.source_x, right.source_y),
        (CanonicalView::Top, 4, 9)
    );
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
