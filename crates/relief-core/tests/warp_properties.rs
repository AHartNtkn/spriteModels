use num_rational::Ratio;
use relief_core::{CanonicalView, Chart, ReliefField, SourcePoint, WarpCoefficients};

fn alpha(relief_eighths: u8) -> u8 {
    255 - relief_eighths
}

#[test]
fn direct_warp_is_flat_transform_plus_relief_parallax() {
    let warp = WarpCoefficients::new([[1, 0, 10], [0, 1, 20]], [2, -1], [0, 0, 1], 3);
    let sample = warp.apply(
        SourcePoint::new(Ratio::from_integer(4), Ratio::from_integer(5)),
        Ratio::from_integer(8),
    );

    assert_eq!(sample.screen_x, Ratio::from_integer(30));
    assert_eq!(sample.screen_y, Ratio::from_integer(17));
    assert_eq!(sample.depth, Ratio::from_integer(25));
}

#[test]
fn direct_warp_keeps_fractional_source_and_relief_exact() {
    let warp = WarpCoefficients::new([[3, -2, 1], [-1, 4, -3]], [-2, 5], [2, 1, 7], -3);
    let sample = warp.apply(
        SourcePoint::new(Ratio::new(1, 3), Ratio::new(5, 2)),
        Ratio::new(7, 4),
    );

    assert_eq!(sample.screen_x, Ratio::new(-13, 2));
    assert_eq!(sample.screen_y, Ratio::new(185, 12));
    assert_eq!(sample.depth, Ratio::new(59, 12));
}

#[test]
fn owning_foreground_cell_has_an_exact_one_sided_closure_limit() {
    let chart = Chart::from_rgba(
        CanonicalView::Front,
        2,
        1,
        vec![[1, 2, 3, alpha(6)], [0, 0, 0, 0]],
    )
    .unwrap();
    let field = ReliefField::new(&chart);
    let public_boundary = SourcePoint::new(Ratio::from_integer(1), Ratio::from_integer(1));

    assert_eq!(field.sample(public_boundary.x, public_boundary.y), None);
    assert_eq!(
        field
            .foreground_cell(0, 0)
            .unwrap()
            .sample_closure(public_boundary),
        Some(Ratio::from_integer(6))
    );
}

#[test]
fn closure_limit_uses_only_the_owning_foreground_component() {
    let chart = Chart::from_rgba(
        CanonicalView::Front,
        3,
        1,
        vec![[1, 0, 0, alpha(2)], [0, 0, 0, 0], [2, 0, 0, alpha(30)]],
    )
    .unwrap();
    let field = ReliefField::new(&chart);

    assert_eq!(
        field
            .foreground_cell(0, 0)
            .unwrap()
            .sample_closure(SourcePoint::new(Ratio::from_integer(1), Ratio::new(1, 2),)),
        Some(Ratio::from_integer(2))
    );
    assert_eq!(
        field
            .foreground_cell(2, 0)
            .unwrap()
            .sample_closure(SourcePoint::new(Ratio::from_integer(2), Ratio::new(1, 2),)),
        Some(Ratio::from_integer(30))
    );
}

#[test]
fn direct_warp_accepts_exact_rational_camera_coefficients() {
    let zero = Ratio::from_integer(0);
    let warp = WarpCoefficients::from_rational(
        [
            [Ratio::new(1, 2), zero, Ratio::new(3, 2)],
            [zero, Ratio::new(1, 4), Ratio::from_integer(-2)],
        ],
        [Ratio::new(1, 8), Ratio::new(-1, 8)],
        [zero, zero, Ratio::from_integer(3)],
        Ratio::new(1, 8),
    );

    let sample = warp.apply(
        SourcePoint::new(Ratio::from_integer(2), Ratio::from_integer(4)),
        Ratio::from_integer(8),
    );

    assert_eq!(sample.screen_x, Ratio::new(7, 2));
    assert_eq!(sample.screen_y, Ratio::from_integer(-2));
    assert_eq!(sample.depth, Ratio::from_integer(4));
}

#[test]
fn inverse_warp_line_recovers_a_known_preimage_with_its_selected_parameter() {
    let warp = WarpCoefficients::new([[2, 1, 3], [-1, 2, 5]], [3, -2], [4, -3, 7], 5);
    let source = SourcePoint::new(Ratio::new(3, 2), Ratio::new(5, 4));
    let relief = Ratio::new(7, 3);
    let warped = warp.apply(source.clone(), relief);

    let line = warp
        .prepare_inverse()
        .expect("front-facing affine chart transform is invertible")
        .inverse_line(warped.screen_x, warped.screen_y);

    assert_eq!(line.source_at(source.x), source);
    assert_eq!(line.relief_at(source.x), relief);
    assert_eq!(line.depth_at(source.x), warped.depth);
}

#[test]
fn inverse_warp_line_uses_source_y_as_parameter_when_x_and_relief_span_screen() {
    let warp = WarpCoefficients::new([[2, 0, 3], [0, 0, -4]], [0, 5], [7, 11, 13], 17);
    let source = SourcePoint::new(Ratio::new(3, 2), Ratio::new(5, 4));
    let relief = Ratio::new(7, 3);
    let warped = warp.apply(source.clone(), relief);

    let line = warp
        .prepare_inverse()
        .expect("source x and relief form a rank-two projected map")
        .inverse_line(warped.screen_x, warped.screen_y);

    assert_eq!(line.source_at(source.y), source);
    assert_eq!(line.depth_at(source.y), warped.depth);
}

#[test]
fn inverse_warp_line_uses_source_x_as_parameter_at_exact_canonical_edge_on() {
    let warp = WarpCoefficients::new([[0, 0, 3], [0, 4, -4]], [5, 0], [7, 11, 13], 17);
    let source = SourcePoint::new(Ratio::new(3, 2), Ratio::new(5, 4));
    let relief = Ratio::new(7, 3);
    let warped = warp.apply(source.clone(), relief);

    let line = warp
        .prepare_inverse()
        .expect("source y and relief form a rank-two projected map")
        .inverse_line(warped.screen_x, warped.screen_y);

    assert_eq!(line.source_at(source.x), source);
    assert_eq!(line.depth_at(source.x), warped.depth);
}

#[test]
fn inverse_warp_line_rejects_a_rank_one_projected_map() {
    let warp = WarpCoefficients::new([[2, 4, 3], [0, 0, -4]], [6, 0], [7, 11, 13], 17);

    // Singularity is a property of the matrix alone, so it is rejected once at
    // preparation time rather than per pixel.
    assert!(warp.prepare_inverse().is_none());
}

#[test]
fn inverse_warp_line_exposes_every_affine_coordinate_and_transient_depth() {
    let warp = WarpCoefficients::new([[2, 0, 3], [0, 0, -4]], [0, 5], [7, 11, 13], 17);
    let source = SourcePoint::new(Ratio::new(3, 2), Ratio::new(5, 4));
    let relief = Ratio::new(7, 3);
    let warped = warp.apply(source, relief);
    let line = warp
        .prepare_inverse()
        .expect("source x and relief form a rank-two projected map")
        .inverse_line(warped.screen_x, warped.screen_y);

    assert_eq!(
        line.variable_coefficients(),
        [
            [Ratio::new(3, 2), Ratio::from_integer(0)],
            [Ratio::from_integer(0), Ratio::from_integer(1)],
            [Ratio::new(7, 3), Ratio::from_integer(0)],
        ]
    );
    assert_eq!(
        line.depth_coefficients(),
        [Ratio::new(379, 6), Ratio::from_integer(11)]
    );
    assert_eq!(line.relief_at(Ratio::new(5, 4)), Ratio::new(7, 3));
}

#[test]
fn inverse_warp_line_selects_the_largest_determinant_with_stable_ties() {
    let warp = WarpCoefficients::new([[1, 0, 0], [0, 1, 0]], [100, 100], [0, 0, 0], 1);
    let warped = warp.apply(
        SourcePoint::new(Ratio::from_integer(2), Ratio::from_integer(3)),
        Ratio::from_integer(5),
    );
    let line = warp
        .prepare_inverse()
        .expect("every projected column pair has rank two")
        .inverse_line(warped.screen_x, warped.screen_y);

    assert_eq!(
        line.variable_coefficients(),
        [
            [Ratio::from_integer(-1), Ratio::from_integer(1)],
            [Ratio::from_integer(0), Ratio::from_integer(1)],
            [Ratio::new(503, 100), Ratio::new(-1, 100)],
        ],
        "(x,h) must beat the smaller (x,y) determinant and win its tie with (y,h)"
    );
}
