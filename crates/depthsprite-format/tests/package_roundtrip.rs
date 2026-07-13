use std::io::{Cursor, Read};

use depthsprite_format::{
    DepthSpriteModel, ManifestV1, PackageError, load_path, load_reader, save_path_atomic,
    save_writer,
};
use relief_core::{Bounds, CanonicalView, Chart, DecodedTexel};
use tempfile::tempdir;
use zip::ZipArchive;

fn chart(bounds: Bounds, view: CanonicalView, pixels: Vec<[u8; 4]>) -> Chart {
    let (width, height) = view.dimensions(bounds);
    Chart::from_rgba(bounds, view, width, height, pixels).unwrap()
}

fn saved(model: &DepthSpriteModel) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    save_writer(model, &mut output).unwrap();
    output.into_inner()
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn assert_canonical_zip32_envelope(bytes: &[u8]) {
    let eocd = bytes.len() - 22;
    assert_eq!(&bytes[eocd..eocd + 4], b"PK\x05\x06");
    assert_eq!(u16_at(bytes, eocd + 4), 0);
    assert_eq!(u16_at(bytes, eocd + 6), 0);
    assert_eq!(u16_at(bytes, eocd + 8), u16_at(bytes, eocd + 10));
    assert_eq!(u16_at(bytes, eocd + 20), 0);
    let count = u16_at(bytes, eocd + 10) as usize;
    let central_start = u32_at(bytes, eocd + 16) as usize;
    assert_eq!(central_start + u32_at(bytes, eocd + 12) as usize, eocd);

    let mut central = central_start;
    let mut expected_local = 0_usize;
    for _ in 0..count {
        assert_eq!(&bytes[central..central + 4], b"PK\x01\x02");
        assert!(matches!(u16_at(bytes, central + 4), 20 | 0x0314));
        assert_eq!(u16_at(bytes, central + 6), 20);
        assert_eq!(u16_at(bytes, central + 8), 0);
        assert_eq!(u16_at(bytes, central + 10), 8);
        assert_eq!(u16_at(bytes, central + 12), 0);
        assert_eq!(u16_at(bytes, central + 14), 33);
        assert_eq!(u16_at(bytes, central + 30), 0);
        assert_eq!(u16_at(bytes, central + 32), 0);
        assert_eq!(u16_at(bytes, central + 34), 0);
        assert_eq!(u16_at(bytes, central + 36), 0);
        assert_eq!(u32_at(bytes, central + 38), 0o100644 << 16);
        let name_len = u16_at(bytes, central + 28) as usize;
        let local = u32_at(bytes, central + 42) as usize;
        assert_eq!(local, expected_local);
        assert_eq!(&bytes[local..local + 4], b"PK\x03\x04");
        assert_eq!(u16_at(bytes, local + 4), 20);
        assert_eq!(u16_at(bytes, local + 6), 0);
        assert_eq!(u16_at(bytes, local + 8), 8);
        assert_eq!(u16_at(bytes, local + 10), 0);
        assert_eq!(u16_at(bytes, local + 12), 33);
        assert_eq!(u16_at(bytes, local + 28), 0);
        assert_eq!(u16_at(bytes, local + 26) as usize, name_len);
        assert_eq!(
            &bytes[local + 30..local + 30 + name_len],
            &bytes[central + 46..central + 46 + name_len]
        );
        assert_eq!(u32_at(bytes, local + 14), u32_at(bytes, central + 16));
        assert_eq!(u32_at(bytes, local + 18), u32_at(bytes, central + 20));
        assert_eq!(u32_at(bytes, local + 22), u32_at(bytes, central + 24));
        expected_local = local + 30 + name_len + u32_at(bytes, local + 18) as usize;
        central += 46 + name_len;
    }
    assert_eq!(expected_local, central_start);
    assert_eq!(central, eocd);
}

#[test]
fn canonical_save_is_byte_identical_after_round_trip() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let model = DepthSpriteModel::new(
        bounds,
        vec![chart(bounds, CanonicalView::Front, vec![[7, 8, 9, 255]])],
    )
    .unwrap();

    let first = saved(&model);
    let loaded = load_reader(Cursor::new(&first)).unwrap();
    let second = saved(&loaded);

    assert_eq!(first, second);
}

#[test]
fn canonical_writer_rank_order_is_accepted_for_every_view() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let views = [
        CanonicalView::Front,
        CanonicalView::Right,
        CanonicalView::Back,
        CanonicalView::Left,
        CanonicalView::Top,
        CanonicalView::Bottom,
    ];
    let model = DepthSpriteModel::new(
        bounds,
        views
            .into_iter()
            .map(|view| chart(bounds, view, vec![[7, 8, 9, 255]]))
            .collect(),
    )
    .unwrap();
    let bytes = saved(&model);

    assert_canonical_zip32_envelope(&bytes);
    assert_eq!(load_reader(Cursor::new(bytes)).unwrap(), model);
}

#[test]
fn model_is_the_sorted_unique_shared_bounds_bundle() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let model = DepthSpriteModel::new(
        bounds,
        vec![
            chart(bounds, CanonicalView::Bottom, vec![[1, 2, 3, 254]]),
            chart(bounds, CanonicalView::Front, vec![[4, 5, 6, 253]]),
            chart(bounds, CanonicalView::Right, vec![[7, 8, 9, 252]]),
        ],
    )
    .unwrap();

    assert_eq!(model.bounds(), bounds);
    assert_eq!(
        model.charts().iter().map(Chart::view).collect::<Vec<_>>(),
        vec![
            CanonicalView::Front,
            CanonicalView::Right,
            CanonicalView::Bottom
        ]
    );

    assert!(matches!(
        DepthSpriteModel::new(bounds, vec![]),
        Err(PackageError::EmptyModel)
    ));
    assert!(matches!(
        DepthSpriteModel::new(
            bounds,
            vec![
                chart(bounds, CanonicalView::Front, vec![[1, 2, 3, 255]]),
                chart(bounds, CanonicalView::Front, vec![[4, 5, 6, 255]])
            ]
        ),
        Err(PackageError::DuplicateView(_))
    ));

    let other_bounds = Bounds::new(2, 1, 1).unwrap();
    assert!(matches!(
        DepthSpriteModel::new(
            bounds,
            vec![chart(
                other_bounds,
                CanonicalView::Front,
                vec![[1, 2, 3, 255], [4, 5, 6, 255]]
            )]
        ),
        Err(PackageError::MixedBounds { .. })
    ));
}

#[test]
fn canonical_archive_has_exact_schema_order_metadata_and_pixels() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let model = DepthSpriteModel::new(
        bounds,
        vec![
            chart(bounds, CanonicalView::Top, vec![[0, 0, 255, 1]]),
            chart(bounds, CanonicalView::Front, vec![[99, 88, 77, 0]]),
        ],
    )
    .unwrap();
    let bytes = saved(&model);
    assert_canonical_zip32_envelope(&bytes);
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();

    assert_eq!(archive.len(), 3);
    assert_eq!(archive.by_index(0).unwrap().name(), "manifest.json");
    assert_eq!(archive.by_index(1).unwrap().name(), "views/front.png");
    assert_eq!(archive.by_index(2).unwrap().name(), "views/top.png");

    for index in 0..archive.len() {
        let file = archive.by_index(index).unwrap();
        assert_eq!(file.compression(), zip::CompressionMethod::Deflated);
        assert_eq!(file.unix_mode(), Some(0o100644));
        let time = file.last_modified().unwrap();
        assert_eq!(
            (
                time.year(),
                time.month(),
                time.day(),
                time.hour(),
                time.minute(),
                time.second()
            ),
            (1980, 1, 1, 0, 0, 0)
        );
    }

    let mut manifest_bytes = Vec::new();
    archive
        .by_name("manifest.json")
        .unwrap()
        .read_to_end(&mut manifest_bytes)
        .unwrap();
    assert_eq!(
        manifest_bytes,
        br#"{"format":"depthsprite","version":1,"bounds_pixels":[1,1,1],"views":["front","top"]}
"#
    );
    let manifest: ManifestV1 = serde_json::from_slice(&manifest_bytes).unwrap();
    assert_eq!(manifest.format, "depthsprite");
    assert_eq!(manifest.version, 1);

    let loaded = load_reader(Cursor::new(archive.into_inner().into_inner())).unwrap();
    assert_eq!(
        loaded.charts()[0].texel(0, 0),
        Some(DecodedTexel::Background)
    );
    assert_eq!(
        loaded.charts()[1].texel(0, 0),
        Some(DecodedTexel::Relief {
            rgb: [0, 0, 255],
            eighths: 254
        })
    );
}

#[test]
fn atomic_save_replaces_existing_destination_and_round_trips() {
    let directory = tempdir().unwrap();
    let destination = directory.path().join("sprite.depthsprite");
    std::fs::write(&destination, b"old complete package").unwrap();

    let bounds = Bounds::new(1, 1, 1).unwrap();
    let model = DepthSpriteModel::new(
        bounds,
        vec![chart(bounds, CanonicalView::Front, vec![[7, 8, 9, 255]])],
    )
    .unwrap();
    save_path_atomic(&model, &destination).unwrap();

    assert_eq!(load_path(&destination).unwrap().charts(), model.charts());
    assert!(!directory.path().join("sprite.depthsprite.tmp").exists());
}

#[test]
fn pre_replace_failure_preserves_destination_and_existing_temp_state() {
    let directory = tempdir().unwrap();
    let destination = directory.path().join("sprite.depthsprite");
    let temporary = directory.path().join("sprite.depthsprite.tmp");
    std::fs::write(&destination, b"old complete package").unwrap();
    std::fs::create_dir(&temporary).unwrap();

    let bounds = Bounds::new(1, 1, 1).unwrap();
    let model = DepthSpriteModel::new(
        bounds,
        vec![chart(bounds, CanonicalView::Front, vec![[7, 8, 9, 255]])],
    )
    .unwrap();
    assert!(matches!(
        save_path_atomic(&model, &destination),
        Err(PackageError::Io(_))
    ));

    assert_eq!(
        std::fs::read(&destination).unwrap(),
        b"old complete package"
    );
    assert!(temporary.is_dir());
}
