use editor_core::{EditorDocument, SourceSprite, opposite};
use relief_core::{Bounds, CanonicalView, Chart};

fn document_with_front_pixels(pixels: Vec<[u8; 4]>) -> EditorDocument {
    let bounds = Bounds::new(2, 1, 1).unwrap();
    let chart = Chart::from_rgba(CanonicalView::Front, 2, 1, pixels).unwrap();
    let model = depthsprite_format::DepthSpriteModel::new(bounds, vec![chart]).unwrap();
    EditorDocument::from_model(model, None).unwrap()
}

#[test]
fn opposite_maps_each_canonical_pair_in_both_directions() {
    assert_eq!(opposite(CanonicalView::Front), CanonicalView::Back);
    assert_eq!(opposite(CanonicalView::Back), CanonicalView::Front);
    assert_eq!(opposite(CanonicalView::Left), CanonicalView::Right);
    assert_eq!(opposite(CanonicalView::Right), CanonicalView::Left);
    assert_eq!(opposite(CanonicalView::Top), CanonicalView::Bottom);
    assert_eq!(opposite(CanonicalView::Bottom), CanonicalView::Top);
}

#[test]
fn one_front_source_resolves_as_front_and_back_with_exact_raw_pixels() {
    let pixels = vec![[17, 31, 47, 0], [71, 83, 97, 1]];
    let document = document_with_front_pixels(pixels.clone());

    let resolved = document.resolved_charts().unwrap();

    assert_eq!(resolved.len(), 2);
    assert_eq!(resolved[0].view(), CanonicalView::Front);
    assert_eq!(resolved[0].rgba(), pixels);
    assert_eq!(resolved[1].view(), CanonicalView::Back);
    assert_eq!(resolved[1].rgba(), pixels);
    assert_eq!(document.to_model().unwrap().charts().len(), 1);
}

#[test]
fn authored_back_replaces_only_the_back_fallback() {
    let front_pixels = vec![[1, 2, 3, 4], [5, 6, 7, 8]];
    let back_pixels = vec![[101, 102, 103, 104], [105, 106, 107, 108]];
    let mut document = document_with_front_pixels(front_pixels.clone());
    document.add_source(CanonicalView::Back).unwrap();
    document
        .replace_source(
            SourceSprite::from_rgba(CanonicalView::Back, 2, 1, back_pixels.clone()).unwrap(),
        )
        .unwrap();

    let resolved = document.resolved_charts().unwrap();

    assert_eq!(resolved.len(), 2);
    assert_eq!(resolved[0].view(), CanonicalView::Front);
    assert_eq!(resolved[0].rgba(), front_pixels);
    assert_eq!(resolved[1].view(), CanonicalView::Back);
    assert_eq!(resolved[1].rgba(), back_pixels);
}

#[test]
fn removing_authored_back_immediately_restores_front_fallback() {
    let front_pixels = vec![[11, 12, 13, 14], [15, 16, 17, 18]];
    let mut document = document_with_front_pixels(front_pixels.clone());
    document.add_source(CanonicalView::Back).unwrap();

    document.remove_source(CanonicalView::Back).unwrap();

    assert!(document.source(CanonicalView::Back).is_none());
    let resolved = document.resolved_charts().unwrap();
    let back = resolved
        .iter()
        .find(|chart| chart.view() == CanonicalView::Back)
        .unwrap();
    assert_eq!(back.rgba(), front_pixels);
}
