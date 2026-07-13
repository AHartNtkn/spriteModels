use relief_core::{Bounds, CanonicalView, Chart, ChartError, DecodedTexel, decode_rgba};

#[test]
fn alpha_is_background_or_exact_eighth_pixel_relief() {
    assert_eq!(decode_rgba([9, 8, 7, 0]), DecodedTexel::Background);
    assert_eq!(
        decode_rgba([9, 8, 7, 255]),
        DecodedTexel::Relief {
            rgb: [9, 8, 7],
            eighths: 0
        }
    );
    assert_eq!(
        decode_rgba([9, 8, 7, 1]),
        DecodedTexel::Relief {
            rgb: [9, 8, 7],
            eighths: 254
        }
    );
}

#[test]
fn canonical_dimensions_come_only_from_integer_bounds() {
    let bounds = Bounds::new(32, 16, 24).unwrap();
    assert_eq!(CanonicalView::Front.dimensions(bounds), (32, 16));
    assert_eq!(CanonicalView::Left.dimensions(bounds), (24, 16));
    assert_eq!(CanonicalView::Top.dimensions(bounds), (32, 24));
}

#[test]
fn chart_rejects_dimensions_that_disagree_with_bounds() {
    let bounds = Bounds::new(2, 1, 3).unwrap();
    let error =
        Chart::from_rgba(bounds, CanonicalView::Top, 2, 2, vec![[0, 0, 0, 0]; 4]).unwrap_err();
    assert_eq!(
        error,
        ChartError::DimensionMismatch {
            expected: (2, 3),
            actual: (2, 2)
        }
    );
}
