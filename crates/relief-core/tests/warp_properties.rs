use num_rational::Ratio;
use relief_core::{Bounds, CanonicalView, Chart, ReliefField, SourcePoint, WarpCoefficients};

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
        Bounds::new(2, 1, 1).unwrap(),
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
        Bounds::new(3, 1, 1).unwrap(),
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
