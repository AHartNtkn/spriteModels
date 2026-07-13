use std::io::{Cursor, Read, Write};

use depthsprite_format::{
    DepthSpriteModel, ManifestV1, PackageError, load_path, load_reader, save_path_atomic,
    save_writer,
};
use relief_core::{Bounds, CanonicalView, Chart, DecodedTexel};
use tempfile::tempdir;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

fn chart(bounds: Bounds, view: CanonicalView, pixels: Vec<[u8; 4]>) -> Chart {
    let (width, height) = view.dimensions(bounds);
    Chart::from_rgba(bounds, view, width, height, pixels).unwrap()
}

fn saved(model: &DepthSpriteModel) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    save_writer(model, &mut output).unwrap();
    output.into_inner()
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
fn input_entry_order_compression_and_manifest_order_do_not_change_canonical_output() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let model = DepthSpriteModel::new(
        bounds,
        vec![
            chart(bounds, CanonicalView::Front, vec![[1, 2, 3, 4]]),
            chart(bounds, CanonicalView::Top, vec![[5, 6, 7, 8]]),
        ],
    )
    .unwrap();
    let canonical = saved(&model);
    let mut source = ZipArchive::new(Cursor::new(&canonical)).unwrap();
    let mut front = Vec::new();
    source
        .by_name("views/front.png")
        .unwrap()
        .read_to_end(&mut front)
        .unwrap();
    let mut top = Vec::new();
    source
        .by_name("views/top.png")
        .unwrap()
        .read_to_end(&mut top)
        .unwrap();

    let mut noncanonical = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(&mut noncanonical);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    writer.start_file("views/top.png", options).unwrap();
    writer.write_all(&top).unwrap();
    writer.start_file("manifest.json", options).unwrap();
    writer
        .write_all(
            br#"{"format":"depthsprite","version":1,"bounds_pixels":[1,1,1],"views":["top","front"]}
"#,
        )
        .unwrap();
    writer.start_file("views/front.png", options).unwrap();
    writer.write_all(&front).unwrap();
    writer.finish().unwrap();

    let loaded = load_reader(Cursor::new(noncanonical.into_inner())).unwrap();
    assert_eq!(saved(&loaded), canonical);
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
