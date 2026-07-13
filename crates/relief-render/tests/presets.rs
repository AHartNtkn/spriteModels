use num_rational::Ratio;
use relief_core::{Bounds, CanonicalView, SourcePoint};
use relief_render::{CameraBasis, TargetView};

fn integer_vector(x: i64, y: i64, z: i64) -> [Ratio<i64>; 3] {
    [
        Ratio::from_integer(x),
        Ratio::from_integer(y),
        Ratio::from_integer(z),
    ]
}

#[test]
fn v1_front_is_identity_for_front_and_culls_edge_on_and_back_facing_charts() {
    let bounds = Bounds::new(2, 3, 4).unwrap();
    let target = TargetView::front_v1();
    let warp = target
        .warp_coefficients(CanonicalView::Front, bounds)
        .unwrap();
    let sample = warp.apply(
        SourcePoint::new(Ratio::new(1, 2), Ratio::new(3, 2)),
        Ratio::from_integer(8),
    );

    assert_eq!(target.preset_version(), 1);
    assert_eq!(sample.screen_x, Ratio::new(1, 2));
    assert_eq!(sample.screen_y, Ratio::new(3, 2));
    assert_eq!(sample.depth, Ratio::from_integer(1));
    assert!(!target.is_front_facing(CanonicalView::Right));
    assert!(!target.is_front_facing(CanonicalView::Back));
}

#[test]
fn v1_right_and_top_presets_compose_signed_chart_frames() {
    let bounds = Bounds::new(2, 3, 4).unwrap();
    let source = SourcePoint::new(Ratio::from_integer(1), Ratio::new(1, 2));

    let right = TargetView::right_v1()
        .warp_coefficients(CanonicalView::Right, bounds)
        .unwrap()
        .apply(source.clone(), Ratio::from_integer(8));
    assert_eq!(right.screen_x, Ratio::from_integer(-3));
    assert_eq!(right.screen_y, Ratio::new(1, 2));
    assert_eq!(right.depth, Ratio::from_integer(-1));

    let top = TargetView::top_v1()
        .warp_coefficients(CanonicalView::Top, bounds)
        .unwrap()
        .apply(source, Ratio::from_integer(8));
    assert_eq!(top.screen_x, Ratio::from_integer(1));
    assert_eq!(top.screen_y, Ratio::new(1, 2));
    assert_eq!(top.depth, Ratio::from_integer(1));
}

#[test]
fn v1_isometric_basis_is_exact_and_exposes_front_right_top_only() {
    let bounds = Bounds::new(4, 4, 4).unwrap();
    let target = TargetView::isometric_v1();
    let front = target
        .warp_coefficients(CanonicalView::Front, bounds)
        .unwrap()
        .apply(
            SourcePoint::new(Ratio::from_integer(2), Ratio::from_integer(4)),
            Ratio::from_integer(8),
        );

    assert_eq!(front.screen_x, Ratio::new(3, 2));
    assert_eq!(front.screen_y, Ratio::new(9, 4));
    assert_eq!(front.depth, Ratio::from_integer(1));
    assert!(target.is_front_facing(CanonicalView::Front));
    assert!(target.is_front_facing(CanonicalView::Right));
    assert!(target.is_front_facing(CanonicalView::Top));
    assert!(!target.is_front_facing(CanonicalView::Back));
    assert!(!target.is_front_facing(CanonicalView::Left));
    assert!(!target.is_front_facing(CanonicalView::Bottom));
}

#[test]
fn rational_camera_factory_derives_back_left_and_bottom_mirroring() {
    let bounds = Bounds::new(2, 3, 4).unwrap();
    let source = SourcePoint::new(Ratio::from_integer(1), Ratio::from_integer(2));
    let relief = Ratio::from_integer(8);

    let back = TargetView::from_camera(CameraBasis::new(
        integer_vector(-1, 0, 0),
        integer_vector(0, 1, 0),
        integer_vector(0, 0, -1),
    ))
    .warp_coefficients(CanonicalView::Back, bounds)
    .unwrap()
    .apply(source.clone(), relief);
    assert_eq!(back.screen_x, Ratio::from_integer(-1));
    assert_eq!(back.screen_y, Ratio::from_integer(2));
    assert_eq!(back.depth, Ratio::from_integer(-3));

    let left = TargetView::from_camera(CameraBasis::new(
        integer_vector(0, 0, 1),
        integer_vector(0, 1, 0),
        integer_vector(1, 0, 0),
    ))
    .warp_coefficients(CanonicalView::Left, bounds)
    .unwrap()
    .apply(source.clone(), relief);
    assert_eq!(left.screen_x, Ratio::from_integer(1));
    assert_eq!(left.screen_y, Ratio::from_integer(2));
    assert_eq!(left.depth, Ratio::from_integer(1));

    let bottom = TargetView::from_camera(CameraBasis::new(
        integer_vector(1, 0, 0),
        integer_vector(0, 0, -1),
        integer_vector(0, -1, 0),
    ))
    .warp_coefficients(CanonicalView::Bottom, bounds)
    .unwrap()
    .apply(source, relief);
    assert_eq!(bottom.screen_x, Ratio::from_integer(1));
    assert_eq!(bottom.screen_y, Ratio::from_integer(-2));
    assert_eq!(bottom.depth, Ratio::from_integer(-2));
}
