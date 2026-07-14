use editor_core::EditorDocument;
use relief_core::{AuthoredModel, Bounds, CanonicalView, Chart};

fn document_with_front_pixels(pixels: Vec<[u8; 4]>) -> EditorDocument {
    let bounds = Bounds::new(2, 1, 63).unwrap();
    let chart = Chart::from_rgba(CanonicalView::Front, 2, 1, pixels).unwrap();
    let model = AuthoredModel::new(bounds, vec![chart]).unwrap();
    EditorDocument::from_model(model, None)
}

#[test]
fn preview_resolution_derives_back_directly_from_the_document_model() {
    let pixels = vec![[17, 31, 47, 0], [71, 83, 97, 251]];
    let document = document_with_front_pixels(pixels.clone());

    let resolved = document.model().resolve();

    assert_eq!(resolved.charts().len(), 2);
    assert_eq!(resolved.chart(CanonicalView::Front).unwrap().rgba(), pixels);
    assert_eq!(resolved.chart(CanonicalView::Back).unwrap().rgba(), pixels);
    assert_eq!(document.model().charts().len(), 1);
}

#[test]
fn preview_resolution_prefers_an_explicit_opposite_chart() {
    let front_pixels = vec![[1, 2, 3, 251], [5, 6, 7, 252]];
    let back_pixels = vec![[101, 102, 103, 253], [105, 106, 107, 254]];
    let mut document = document_with_front_pixels(front_pixels.clone());
    document.add_source(CanonicalView::Back).unwrap();
    document
        .replace_source(Chart::from_rgba(CanonicalView::Back, 2, 1, back_pixels.clone()).unwrap())
        .unwrap();

    let resolved = document.model().resolve();

    assert_eq!(
        resolved.chart(CanonicalView::Front).unwrap().rgba(),
        front_pixels
    );
    assert_eq!(
        resolved.chart(CanonicalView::Back).unwrap().rgba(),
        back_pixels
    );
}
