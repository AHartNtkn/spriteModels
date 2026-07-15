use relief_core::{
    AuthoredModel, Bounds, CanonicalFrame, CanonicalView, Chart, EMPTY_RGBA, ModelError,
};

fn rgba(relief: u8, rgb: [u8; 3]) -> [u8; 4] {
    [rgb[0], rgb[1], rgb[2], 255 - relief]
}

fn chart(bounds: Bounds, view: CanonicalView, pixel: [u8; 4]) -> Chart {
    let (width, height) = view.dimensions(bounds);
    Chart::from_rgba(view, width, height, vec![pixel; (width * height) as usize]).unwrap()
}

#[test]
fn bounds_are_limited_to_the_fixed_scale_encodable_range() {
    assert!(Bounds::new(1, 63, 1).is_ok());
    for dimensions in [
        (0, 1, 1),
        (64, 1, 1),
        (1, 0, 1),
        (1, 64, 1),
        (1, 1, 0),
        (1, 1, 64),
    ] {
        assert!(Bounds::new(dimensions.0, dimensions.1, dimensions.2).is_err());
    }
}

#[test]
fn canonical_frames_define_each_signed_source_plane() {
    let bounds = Bounds::new(10, 12, 14).unwrap();
    let cases = [
        (
            CanonicalView::Front,
            CanonicalFrame {
                origin: [0, 0, 0],
                source_u: [1, 0, 0],
                source_v: [0, 1, 0],
                inward: [0, 0, 1],
            },
        ),
        (
            CanonicalView::Back,
            CanonicalFrame {
                origin: [10, 0, 14],
                source_u: [-1, 0, 0],
                source_v: [0, 1, 0],
                inward: [0, 0, -1],
            },
        ),
        (
            CanonicalView::Left,
            CanonicalFrame {
                origin: [0, 0, 0],
                source_u: [0, 0, 1],
                source_v: [0, 1, 0],
                inward: [1, 0, 0],
            },
        ),
        (
            CanonicalView::Right,
            CanonicalFrame {
                origin: [10, 0, 14],
                source_u: [0, 0, -1],
                source_v: [0, 1, 0],
                inward: [-1, 0, 0],
            },
        ),
        (
            CanonicalView::Top,
            CanonicalFrame {
                origin: [0, 0, 0],
                source_u: [1, 0, 0],
                source_v: [0, 0, 1],
                inward: [0, 1, 0],
            },
        ),
        (
            CanonicalView::Bottom,
            CanonicalFrame {
                origin: [0, 12, 14],
                source_u: [1, 0, 0],
                source_v: [0, 0, -1],
                inward: [0, -1, 0],
            },
        ),
    ];

    for (view, expected) in cases {
        assert_eq!(view.frame(bounds), expected);
        assert_eq!(view.opposite().opposite(), view);
    }
}

#[test]
fn maximum_inward_depth_is_half_the_opposing_axis() {
    let bounds = Bounds::new(10, 12, 14).unwrap();
    assert_eq!(CanonicalView::Front.maximum_inward_depth(bounds), 56);
    assert_eq!(CanonicalView::Left.maximum_inward_depth(bounds), 40);
    assert_eq!(CanonicalView::Top.maximum_inward_depth(bounds), 48);
}

#[test]
fn model_rejects_invalid_chart_counts_before_other_chart_validation() {
    let bounds = Bounds::new(2, 3, 4).unwrap();
    assert_eq!(
        AuthoredModel::new(bounds, vec![]),
        Err(ModelError::ChartCount(0))
    );

    let front = chart(bounds, CanonicalView::Front, rgba(0, [1, 2, 3]));
    assert_eq!(
        AuthoredModel::new(bounds, vec![front; 7]),
        Err(ModelError::ChartCount(7))
    );
}

#[test]
fn model_rejects_duplicates_dimension_mismatches_and_relief_beyond_the_midpoint() {
    let bounds = Bounds::new(2, 3, 4).unwrap();
    let front = chart(bounds, CanonicalView::Front, rgba(16, [1, 2, 3]));
    assert!(AuthoredModel::new(bounds, vec![front.clone()]).is_ok());
    assert!(matches!(
        AuthoredModel::new(bounds, vec![front.clone(), front]),
        Err(ModelError::DuplicateView(CanonicalView::Front))
    ));

    let wrong_dimensions =
        Chart::from_rgba(CanonicalView::Front, 1, 3, vec![rgba(0, [1, 2, 3]); 3]).unwrap();
    assert!(matches!(
        AuthoredModel::new(bounds, vec![wrong_dimensions]),
        Err(ModelError::DimensionMismatch {
            view: CanonicalView::Front,
            expected: (2, 3),
            actual: (1, 3),
        })
    ));

    let too_deep = chart(bounds, CanonicalView::Front, rgba(17, [1, 2, 3]));
    assert!(matches!(
        AuthoredModel::new(bounds, vec![too_deep]),
        Err(ModelError::ReliefBeyondMaximum {
            view: CanonicalView::Front,
            actual: 17,
            maximum: 16,
            ..
        })
    ));

    let transparent = chart(bounds, CanonicalView::Front, [1, 2, 3, 0]);
    assert!(AuthoredModel::new(bounds, vec![transparent]).is_ok());
}

#[test]
fn opposite_maxima_have_the_same_midpoint_coordinate() {
    let bounds = Bounds::new(3, 5, 7).unwrap();
    let axis = i64::from(bounds.depth());
    let inward = i64::from(CanonicalView::Front.maximum_inward_depth(bounds));
    assert_eq!(
        num_rational::Ratio::new(inward, 8),
        num_rational::Ratio::new(axis, 2),
    );
    assert_eq!(
        num_rational::Ratio::from_integer(axis) - num_rational::Ratio::new(inward, 8),
        num_rational::Ratio::new(axis, 2),
    );
}

#[test]
fn authored_charts_are_kept_in_canonical_rank_order() {
    let bounds = Bounds::new(2, 2, 2).unwrap();
    let back = chart(bounds, CanonicalView::Back, rgba(0, [3, 3, 3]));
    let bottom = chart(bounds, CanonicalView::Bottom, rgba(0, [6, 6, 6]));
    let front = chart(bounds, CanonicalView::Front, rgba(0, [1, 1, 1]));
    let mut model = AuthoredModel::new(bounds, vec![bottom, back]).unwrap();
    model.add_chart(front).unwrap();

    assert_eq!(
        model.charts().iter().map(Chart::view).collect::<Vec<_>>(),
        [
            CanonicalView::Front,
            CanonicalView::Back,
            CanonicalView::Bottom,
        ]
    );
}

#[test]
fn one_source_resolves_only_its_explicit_side_assignment() {
    let bounds = Bounds::new(2, 2, 2).unwrap();
    let front_pixels = vec![
        rgba(0, [200, 10, 20]),
        rgba(1, [201, 11, 21]),
        rgba(2, [202, 12, 22]),
        rgba(3, [203, 13, 23]),
    ];
    let front = Chart::from_rgba(CanonicalView::Front, 2, 2, front_pixels.clone()).unwrap();
    let model = AuthoredModel::new(bounds, vec![front]).unwrap();
    let resolved = model.resolve();
    assert_eq!(resolved.bounds(), bounds);
    assert_eq!(resolved.charts().len(), 1);
    assert!(resolved.chart(CanonicalView::Back).is_none());
}

#[test]
fn one_source_can_be_explicitly_assigned_to_an_opposite_pair() {
    let bounds = Bounds::new(2, 2, 2).unwrap();
    let front_pixels = vec![rgba(0, [200, 10, 20]); 4];
    let front = Chart::from_rgba(CanonicalView::Front, 2, 2, front_pixels.clone())
        .unwrap()
        .with_opposite_assignment();
    let model = AuthoredModel::new(bounds, vec![front]).unwrap();
    let resolved = model.resolve();

    assert_eq!(resolved.charts().len(), 2);
    assert_eq!(
        resolved.chart(CanonicalView::Front).unwrap().rgba(),
        front_pixels
    );
    assert_eq!(
        resolved.chart(CanonicalView::Back).unwrap().rgba(),
        front_pixels
    );
}

#[test]
fn mirror_assignment_defaults_false_and_survives_disabling_opposite() {
    let bounds = Bounds::new(2, 2, 2).unwrap();
    let front = chart(bounds, CanonicalView::Front, rgba(0, [1, 2, 3]));
    let mut model = AuthoredModel::new(bounds, vec![front]).unwrap();

    assert!(
        !model
            .chart(CanonicalView::Front)
            .unwrap()
            .supplies_opposite()
    );
    assert!(
        !model
            .chart(CanonicalView::Front)
            .unwrap()
            .mirrors_opposite()
    );

    model
        .set_opposite_mirror(CanonicalView::Front, true)
        .unwrap();
    assert!(
        model
            .chart(CanonicalView::Front)
            .unwrap()
            .mirrors_opposite()
    );
    assert!(model.resolve().chart(CanonicalView::Back).is_none());

    model
        .set_opposite_assignment(CanonicalView::Front, true)
        .unwrap();
    model
        .set_opposite_assignment(CanonicalView::Front, false)
        .unwrap();

    assert!(
        !model
            .chart(CanonicalView::Front)
            .unwrap()
            .supplies_opposite()
    );
    assert!(
        model
            .chart(CanonicalView::Front)
            .unwrap()
            .mirrors_opposite()
    );
    assert!(model.resolve().chart(CanonicalView::Back).is_none());
}

#[test]
fn mirror_resolution_reverses_the_canonical_frame_axis_for_every_pair() {
    let bounds = Bounds::new(2, 2, 2).unwrap();
    let authored = vec![
        rgba(0, [10, 1, 1]),
        rgba(1, [20, 2, 2]),
        rgba(2, [30, 3, 3]),
        rgba(3, [40, 4, 4]),
    ];
    let horizontal = vec![authored[1], authored[0], authored[3], authored[2]];
    let vertical = vec![authored[2], authored[3], authored[0], authored[1]];

    for view in [
        CanonicalView::Front,
        CanonicalView::Back,
        CanonicalView::Left,
        CanonicalView::Right,
        CanonicalView::Top,
        CanonicalView::Bottom,
    ] {
        let source = Chart::from_rgba(view, 2, 2, authored.clone())
            .unwrap()
            .with_opposite_assignment()
            .with_mirrored_opposite();
        let model = AuthoredModel::new(bounds, vec![source]).unwrap();
        let resolved = model.resolve();
        let expected = match view {
            CanonicalView::Front
            | CanonicalView::Back
            | CanonicalView::Left
            | CanonicalView::Right => &horizontal,
            CanonicalView::Top | CanonicalView::Bottom => &vertical,
        };

        assert_eq!(resolved.chart(view).unwrap().rgba(), authored);
        assert_eq!(resolved.chart(view.opposite()).unwrap().rgba(), expected);
    }
}

#[test]
fn mirror_disabled_keeps_direct_opposite_rgba_for_every_pair() {
    let bounds = Bounds::new(2, 2, 2).unwrap();
    let authored = vec![
        rgba(0, [10, 1, 1]),
        rgba(1, [20, 2, 2]),
        rgba(2, [30, 3, 3]),
        rgba(3, [40, 4, 4]),
    ];

    for view in [
        CanonicalView::Front,
        CanonicalView::Back,
        CanonicalView::Left,
        CanonicalView::Right,
        CanonicalView::Top,
        CanonicalView::Bottom,
    ] {
        let source = Chart::from_rgba(view, 2, 2, authored.clone())
            .unwrap()
            .with_opposite_assignment();
        let model = AuthoredModel::new(bounds, vec![source]).unwrap();

        assert_eq!(
            model.resolve().chart(view.opposite()).unwrap().rgba(),
            authored
        );
    }
}

#[test]
fn editing_pixels_preserves_both_explicit_assignment_bits() {
    let bounds = Bounds::new(2, 2, 2).unwrap();
    let front = chart(bounds, CanonicalView::Front, rgba(0, [1, 2, 3]))
        .with_opposite_assignment()
        .with_mirrored_opposite();
    let mut model = AuthoredModel::new(bounds, vec![front]).unwrap();
    let replacement = vec![rgba(0, [9, 8, 7]); 4];

    model
        .set_rgba(CanonicalView::Front, replacement.clone())
        .unwrap();

    assert!(
        model
            .chart(CanonicalView::Front)
            .unwrap()
            .supplies_opposite()
    );
    assert!(
        model
            .chart(CanonicalView::Front)
            .unwrap()
            .mirrors_opposite()
    );
    assert_eq!(
        model.resolve().chart(CanonicalView::Back).unwrap().rgba(),
        replacement
    );
}

#[test]
fn assigned_sides_cannot_overlap_another_source() {
    let bounds = Bounds::new(2, 2, 2).unwrap();
    let front =
        chart(bounds, CanonicalView::Front, rgba(0, [200, 10, 20])).with_opposite_assignment();
    let back = chart(bounds, CanonicalView::Back, rgba(0, [20, 10, 200]));

    assert_eq!(
        AuthoredModel::new(bounds, vec![front, back]),
        Err(ModelError::DuplicateView(CanonicalView::Back))
    );
}

#[test]
fn removing_an_explicit_opposite_leaves_that_side_absent() {
    let bounds = Bounds::new(2, 2, 2).unwrap();
    let front = chart(bounds, CanonicalView::Front, rgba(0, [200, 10, 20]));
    let back = chart(bounds, CanonicalView::Back, rgba(0, [20, 10, 200]));
    let mut model = AuthoredModel::new(bounds, vec![front, back]).unwrap();

    model.remove_chart(CanonicalView::Back).unwrap();

    assert!(model.resolve().chart(CanonicalView::Back).is_none());
}

#[test]
fn removing_the_last_chart_fails_without_mutation() {
    let bounds = Bounds::new(2, 2, 2).unwrap();
    let front = chart(bounds, CanonicalView::Front, rgba(0, [1, 2, 3]));
    let mut model = AuthoredModel::new(bounds, vec![front]).unwrap();
    let before = model.clone();

    assert_eq!(
        model.remove_chart(CanonicalView::Front),
        Err(ModelError::LastChart)
    );
    assert_eq!(model, before);
}

#[test]
fn failed_mutations_leave_the_entire_model_unchanged() {
    let bounds = Bounds::new(2, 2, 2).unwrap();
    let front = chart(bounds, CanonicalView::Front, rgba(0, [1, 2, 3]));
    let mut model = AuthoredModel::new(bounds, vec![front.clone()]).unwrap();

    let before = model.clone();
    let too_deep_back = chart(bounds, CanonicalView::Back, rgba(9, [7, 8, 9]));
    assert!(matches!(
        model.add_chart(too_deep_back),
        Err(ModelError::ReliefBeyondMaximum { .. })
    ));
    assert_eq!(model, before);

    let wrong_dimensions =
        Chart::from_rgba(CanonicalView::Front, 1, 2, vec![rgba(0, [9, 8, 7]); 2]).unwrap();
    assert!(matches!(
        model.replace_chart(wrong_dimensions),
        Err(ModelError::DimensionMismatch { .. })
    ));
    assert_eq!(model, before);

    assert!(matches!(
        model.set_rgba(CanonicalView::Front, vec![rgba(9, [9, 8, 7]); 4]),
        Err(ModelError::ReliefBeyondMaximum { .. })
    ));
    assert_eq!(model, before);

    assert!(matches!(
        model.set_rgba(CanonicalView::Front, vec![rgba(0, [9, 8, 7]); 3]),
        Err(ModelError::Chart(_))
    ));
    assert_eq!(model, before);
}

#[test]
fn missing_replacement_removal_and_pixel_target_report_the_view() {
    let bounds = Bounds::new(2, 2, 2).unwrap();
    let front = chart(bounds, CanonicalView::Front, rgba(0, [1, 2, 3]));
    let mut model = AuthoredModel::new(bounds, vec![front]).unwrap();
    let back = chart(bounds, CanonicalView::Back, rgba(0, [4, 5, 6]));

    assert_eq!(
        model.replace_chart(back),
        Err(ModelError::MissingView(CanonicalView::Back))
    );
    assert_eq!(
        model.remove_chart(CanonicalView::Back),
        Err(ModelError::MissingView(CanonicalView::Back))
    );
    assert_eq!(
        model.set_rgba(CanonicalView::Back, vec![rgba(0, [4, 5, 6]); 4]),
        Err(ModelError::MissingView(CanonicalView::Back))
    );
}

#[test]
fn empty_charts_use_the_visible_magenta_authoring_sentinel() {
    let bounds = Bounds::new(2, 1, 1).unwrap();
    let mut model = AuthoredModel::with_empty_chart(bounds, CanonicalView::Front).unwrap();
    assert_eq!(
        model.chart(CanonicalView::Front).unwrap().rgba(),
        &[EMPTY_RGBA; 2]
    );

    model.add_empty_chart(CanonicalView::Back).unwrap();
    assert_eq!(
        model.chart(CanonicalView::Back).unwrap().rgba(),
        &[EMPTY_RGBA; 2]
    );
}
