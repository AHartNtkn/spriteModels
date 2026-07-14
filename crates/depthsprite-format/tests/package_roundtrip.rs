use std::io::Cursor;

use depthsprite_format::{
    DepthSpriteModel, PackageError, load_path, load_reader, save_path_atomic, save_writer,
};
use relief_core::{Bounds, CanonicalView, Chart, DecodedTexel};
use tempfile::tempdir;

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
fn save_and_load_preserve_model_semantics() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let model = DepthSpriteModel::new(
        bounds,
        vec![
            chart(bounds, CanonicalView::Top, vec![[0, 0, 255, 1]]),
            chart(bounds, CanonicalView::Front, vec![[99, 88, 77, 0]]),
        ],
    )
    .unwrap();

    let loaded = load_reader(Cursor::new(saved(&model))).unwrap();

    assert_eq!(loaded, model);
    assert_eq!(
        loaded.charts()[0].texel(0, 0),
        Some(DecodedTexel::Background)
    );
    assert_eq!(
        loaded.charts()[1].texel(0, 0),
        Some(DecodedTexel::Relief {
            rgb: [0, 0, 255],
            eighths: 254,
        })
    );
}

#[test]
fn model_is_a_sorted_unique_shared_bounds_bundle() {
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
            CanonicalView::Bottom,
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
                chart(bounds, CanonicalView::Front, vec![[4, 5, 6, 255]]),
            ]
        ),
        Err(PackageError::DuplicateView(_))
    ));
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
