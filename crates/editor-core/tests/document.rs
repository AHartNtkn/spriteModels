use std::path::PathBuf;

use editor_core::{EditorDocument, EditorError};
use relief_core::{AuthoredModel, Bounds, CanonicalView, Chart, EMPTY_RGBA, ModelError};

fn bounds() -> Bounds {
    Bounds::new(2, 3, 4).unwrap()
}

#[test]
fn new_document_owns_one_empty_source_and_is_clean() {
    let document = EditorDocument::new(bounds(), CanonicalView::Front);

    let sources: Vec<_> = document.sources().collect();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].view(), CanonicalView::Front);
    assert_eq!(sources[0].dimensions(), (2, 3));
    assert_eq!(sources[0].rgba(), &[EMPTY_RGBA; 6]);
    assert!(!sources[0].supplies_opposite());
    assert!(!sources[0].mirrors_opposite());
    assert_eq!(document.model(), &document.to_model());
    assert_eq!(document.selected_view(), CanonicalView::Front);
    assert!(!document.is_dirty());
}

#[test]
fn sources_stay_in_canonical_order_and_a_seventh_is_refused() {
    let mut document = EditorDocument::new(bounds(), CanonicalView::Front);
    for view in [
        CanonicalView::Bottom,
        CanonicalView::Back,
        CanonicalView::Top,
        CanonicalView::Right,
        CanonicalView::Left,
    ] {
        document.add_source(view).unwrap();
    }

    let views: Vec<_> = document.sources().map(Chart::view).collect();
    assert_eq!(
        views,
        [
            CanonicalView::Front,
            CanonicalView::Right,
            CanonicalView::Back,
            CanonicalView::Left,
            CanonicalView::Top,
            CanonicalView::Bottom,
        ]
    );
    assert!(matches!(
        document.add_source(CanonicalView::Front),
        Err(EditorError::Model(ModelError::ChartCount(7)))
    ));
}

#[test]
fn replacing_a_source_rejects_dimensions_that_do_not_match_model_bounds() {
    let mut document = EditorDocument::new(bounds(), CanonicalView::Front);
    let wrong_size = Chart::from_rgba(CanonicalView::Front, 1, 3, vec![[1, 2, 3, 255]; 3]).unwrap();

    let error = document.replace_source(wrong_size).unwrap_err();

    assert!(matches!(
        error,
        EditorError::Model(ModelError::DimensionMismatch {
            view: CanonicalView::Front,
            expected: (2, 3),
            actual: (1, 3),
        })
    ));
}

#[test]
fn removing_the_only_source_returns_last_source_without_changing_the_document() {
    let mut document = EditorDocument::new(bounds(), CanonicalView::Front);
    let before_sources: Vec<_> = document.sources().cloned().collect();
    let before_selection = document.selected_view();
    let before_revision = document.revision();

    let error = document.remove_source(CanonicalView::Front).unwrap_err();

    assert!(matches!(error, EditorError::Model(ModelError::LastChart)));
    assert_eq!(
        document.sources().cloned().collect::<Vec<_>>(),
        before_sources
    );
    assert_eq!(document.selected_view(), before_selection);
    assert_eq!(document.revision(), before_revision);
    assert!(!document.is_dirty());
    assert!(!document.can_undo());
    assert!(!document.can_redo());
}

#[test]
fn model_conversion_preserves_authored_rgba_order_and_clean_baseline() {
    let top_pixels = vec![[20, 21, 22, 243]; 8];
    let front_pixels = vec![
        [7, 8, 9, 0],
        [10, 11, 12, 255],
        [13, 14, 15, 251],
        [16, 17, 18, 250],
        [20, 21, 22, 249],
        [24, 25, 26, 248],
    ];
    let model = AuthoredModel::new(
        bounds(),
        vec![
            Chart::from_rgba(CanonicalView::Top, 2, 4, top_pixels.clone()).unwrap(),
            Chart::from_rgba(CanonicalView::Front, 2, 3, front_pixels.clone()).unwrap(),
        ],
    )
    .unwrap();
    let path = PathBuf::from("model.depthsprite");

    let document = EditorDocument::from_model(model.clone(), Some(path.clone()));
    let round_trip = document.to_model();

    assert_eq!(document.path(), Some(path.as_path()));
    assert!(!document.is_dirty());
    assert_eq!(document.model(), &model);
    assert_eq!(round_trip.charts().len(), 2);
    assert_eq!(round_trip.charts()[0].view(), CanonicalView::Front);
    assert_eq!(round_trip.charts()[0].rgba(), front_pixels);
    assert_eq!(round_trip.charts()[1].view(), CanonicalView::Top);
    assert_eq!(round_trip.charts()[1].rgba(), top_pixels);
}
