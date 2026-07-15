use std::io::{Cursor, Read, Write};

use depthsprite_format::{PackageError, load_path, load_reader, save_path_atomic, save_writer};
use png::{BitDepth, ColorType, Encoder};
use relief_core::{AuthoredModel, Bounds, CanonicalView, Chart, DecodedTexel, ModelError};
use tempfile::tempdir;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

fn chart(bounds: Bounds, view: CanonicalView, pixels: Vec<[u8; 4]>) -> Chart {
    let (width, height) = view.dimensions(bounds);
    Chart::from_rgba(view, width, height, pixels).unwrap()
}

fn saved(model: &AuthoredModel) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    save_writer(model, &mut output).unwrap();
    output.into_inner()
}

fn manifest(bytes: &[u8]) -> String {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut manifest = String::new();
    archive
        .by_name("manifest.json")
        .unwrap()
        .read_to_string(&mut manifest)
        .unwrap();
    manifest
}

#[test]
fn save_and_load_preserve_model_semantics() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let model = AuthoredModel::new(
        bounds,
        vec![
            chart(bounds, CanonicalView::Top, vec![[0, 0, 255, 251]]),
            chart(bounds, CanonicalView::Front, vec![[99, 88, 77, 0]]),
        ],
    )
    .unwrap();

    let loaded = load_reader(Cursor::new(saved(&model))).unwrap();

    assert_eq!(loaded, model);
    assert_eq!(
        loaded.charts()[0].texel_at(0, 0),
        Some(DecodedTexel::Background)
    );
    assert_eq!(
        loaded.charts()[1].texel_at(0, 0),
        Some(DecodedTexel::Relief {
            rgb: [0, 0, 255],
            eighths: 4,
        })
    );
}

#[test]
fn save_and_load_preserve_both_opposite_assignment_bits() {
    let bounds = Bounds::new(2, 2, 2).unwrap();
    let front = chart(bounds, CanonicalView::Front, vec![[10, 20, 30, 255]; 4])
        .with_opposite_assignment()
        .with_mirrored_opposite();
    let right = chart(bounds, CanonicalView::Right, vec![[70, 80, 90, 255]; 4]);
    let top =
        chart(bounds, CanonicalView::Top, vec![[40, 50, 60, 255]; 4]).with_opposite_assignment();
    let model = AuthoredModel::new(bounds, vec![front, right, top]).unwrap();

    let bytes = saved(&model);
    assert_eq!(
        manifest(&bytes),
        concat!(
            r#"{"format":"depthsprite","version":1,"bounds_pixels":[2,2,2],"sources":["#,
            r#"{"view":"front","opposite":true,"mirror":true},"#,
            r#"{"view":"right","opposite":false,"mirror":false},"#,
            r#"{"view":"top","opposite":true,"mirror":false}]}"#,
            "\n"
        )
    );

    let loaded = load_reader(Cursor::new(bytes)).unwrap();
    let resolved = loaded.resolve();

    let front = loaded.chart(CanonicalView::Front).unwrap();
    let right = loaded.chart(CanonicalView::Right).unwrap();
    let top = loaded.chart(CanonicalView::Top).unwrap();
    assert!(front.supplies_opposite());
    assert!(front.mirrors_opposite());
    assert!(!right.supplies_opposite());
    assert!(!right.mirrors_opposite());
    assert!(top.supplies_opposite());
    assert!(!top.mirrors_opposite());
    assert!(resolved.chart(CanonicalView::Front).is_some());
    assert!(resolved.chart(CanonicalView::Back).is_some());
    assert!(resolved.chart(CanonicalView::Right).is_some());
    assert!(resolved.chart(CanonicalView::Left).is_none());
    assert!(resolved.chart(CanonicalView::Top).is_some());
    assert!(resolved.chart(CanonicalView::Bottom).is_some());
}

#[test]
fn authored_model_is_a_sorted_unique_shared_bounds_bundle() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let model = AuthoredModel::new(
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
            CanonicalView::Bottom,
        ]
    );
    assert!(matches!(
        AuthoredModel::new(bounds, vec![]),
        Err(ModelError::ChartCount(0))
    ));
    assert!(matches!(
        AuthoredModel::new(
            bounds,
            vec![
                chart(bounds, CanonicalView::Front, vec![[1, 2, 3, 255]]),
                chart(bounds, CanonicalView::Front, vec![[4, 5, 6, 255]]),
            ]
        ),
        Err(ModelError::DuplicateView(CanonicalView::Front))
    ));
}

#[test]
fn model_bounds_validate_each_chart_view_dimensions() {
    let bounds = Bounds::new(2, 1, 3).unwrap();
    let chart = Chart::from_rgba(CanonicalView::Top, 2, 2, vec![[0, 0, 0, 0]; 4]).unwrap();

    assert!(matches!(
        AuthoredModel::new(bounds, vec![chart]),
        Err(ModelError::DimensionMismatch {
            view: CanonicalView::Top,
            expected: (2, 3),
            actual: (2, 2),
        })
    ));
}

#[test]
fn atomic_save_replaces_existing_destination_and_round_trips() {
    let directory = tempdir().unwrap();
    let destination = directory.path().join("sprite.depthsprite");
    std::fs::write(&destination, b"old complete package").unwrap();
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let model = AuthoredModel::new(
        bounds,
        vec![chart(bounds, CanonicalView::Front, vec![[7, 8, 9, 255]])],
    )
    .unwrap();

    save_path_atomic(&model, &destination).unwrap();

    assert_eq!(load_path(&destination).unwrap(), model);
    assert!(!directory.path().join("sprite.depthsprite.tmp").exists());
}

#[test]
fn pre_replace_failure_preserves_destination_and_existing_temp() {
    let directory = tempdir().unwrap();
    let destination = directory.path().join("sprite.depthsprite");
    let temporary = directory.path().join("sprite.depthsprite.tmp");
    std::fs::write(&destination, b"old complete package").unwrap();
    std::fs::create_dir(&temporary).unwrap();
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let model = AuthoredModel::new(
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

#[test]
fn loading_rejects_relief_beyond_the_models_view_specific_maximum() {
    let mut png = Cursor::new(Vec::new());
    {
        let mut encoder = Encoder::new(&mut png, 1, 1);
        encoder.set_color(ColorType::Rgba);
        encoder.set_depth(BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[1, 2, 3, 250]).unwrap();
    }

    let mut package = Cursor::new(Vec::new());
    {
        let mut archive = ZipWriter::new(&mut package);
        let options = SimpleFileOptions::default();
        archive.start_file("manifest.json", options).unwrap();
        archive
            .write_all(
                br#"{"format":"depthsprite","version":1,"bounds_pixels":[1,1,1],"sources":[{"view":"front","opposite":false,"mirror":false}]}"#,
            )
            .unwrap();
        archive.start_file("views/front.png", options).unwrap();
        archive.write_all(png.get_ref()).unwrap();
        archive.finish().unwrap();
    }
    package.set_position(0);

    let error = load_reader(package).unwrap_err();

    assert!(matches!(
        error,
        PackageError::Model(ModelError::ReliefBeyondMaximum {
            view: CanonicalView::Front,
            x: 0,
            y: 0,
            actual: 5,
            maximum: 4,
        })
    ));
}
