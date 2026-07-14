use relief_core::{
    Bounds, CanonicalView, Chart, ChartError, DecodedTexel, RELIEF_UNITS_PER_PIXEL, decode_rgba,
};

#[test]
fn alpha_is_background_or_exact_eighth_pixel_relief() {
    assert_eq!(RELIEF_UNITS_PER_PIXEL, 8);
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
fn bounds_exposes_validated_dimensions_read_only() {
    let bounds = Bounds::new(32, 16, 24).unwrap();

    assert_eq!(bounds.width(), 32);
    assert_eq!(bounds.height(), 16);
    assert_eq!(bounds.depth(), 24);
}

#[test]
fn bounds_rejects_zero_in_every_dimension() {
    for dimensions in [(0, 1, 1), (1, 0, 1), (1, 1, 0)] {
        assert_eq!(
            Bounds::new(dimensions.0, dimensions.1, dimensions.2),
            Err(ChartError::ZeroBounds)
        );
    }
}

#[test]
fn canonical_dimensions_come_only_from_integer_bounds() {
    let bounds = Bounds::new(32, 16, 24).unwrap();

    assert_eq!(CanonicalView::Front.dimensions(bounds), (32, 16));
    assert_eq!(CanonicalView::Back.dimensions(bounds), (32, 16));
    assert_eq!(CanonicalView::Left.dimensions(bounds), (24, 16));
    assert_eq!(CanonicalView::Right.dimensions(bounds), (24, 16));
    assert_eq!(CanonicalView::Top.dimensions(bounds), (32, 24));
    assert_eq!(CanonicalView::Bottom.dimensions(bounds), (32, 24));
}

#[test]
fn canonical_view_rank_round_trips_for_every_view() {
    let views = [
        CanonicalView::Front,
        CanonicalView::Right,
        CanonicalView::Back,
        CanonicalView::Left,
        CanonicalView::Top,
        CanonicalView::Bottom,
    ];

    for view in views {
        assert_eq!(CanonicalView::from_rank(view.rank()), Some(view));
    }
    assert_eq!(CanonicalView::from_rank(6), None);
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

#[test]
fn chart_rejects_any_non_exact_texel_count() {
    let bounds = Bounds::new(2, 1, 3).unwrap();

    for texel_count in [5, 7] {
        let error = Chart::from_rgba(
            bounds,
            CanonicalView::Top,
            2,
            3,
            vec![[0, 0, 0, 0]; texel_count],
        )
        .unwrap_err();
        assert_eq!(error, ChartError::PixelCount);
    }
}

#[test]
fn chart_preserves_raw_rgba_while_decoding_relief_on_demand() {
    let bounds = Bounds::new(2, 1, 1).unwrap();
    let chart = Chart::from_rgba(
        bounds,
        CanonicalView::Front,
        2,
        1,
        vec![[17, 31, 47, 0], [9, 8, 7, 1]],
    )
    .unwrap();

    assert_eq!(chart.rgba(), &[[17, 31, 47, 0], [9, 8, 7, 1]]);
    assert_eq!(chart.rgba_at(0, 0), Some([17, 31, 47, 0]));
    assert_eq!(chart.rgba_at(2, 0), None);
    assert_eq!(chart.texel_at(0, 0), Some(DecodedTexel::Background));
    assert_eq!(chart.texel_at(2, 0), None);
    assert_eq!(
        chart.texels().collect::<Vec<_>>(),
        vec![
            DecodedTexel::Background,
            DecodedTexel::Relief {
                rgb: [9, 8, 7],
                eighths: 254,
            },
        ]
    );
    assert_eq!(chart.texels().len(), 2);
}
