use num_rational::Ratio;
use relief_core::{Bounds, CanonicalView, SourcePoint};
use relief_render::TargetView;

#[test]
fn front_is_identity_for_front_and_culls_edge_on_and_back_facing_charts() {
    let bounds = Bounds::new(2, 3, 4).unwrap();
    let target = TargetView::front();
    let warp = target
        .warp_coefficients(CanonicalView::Front, bounds)
        .unwrap();
    let sample = warp.apply(
        SourcePoint::new(Ratio::new(1, 2), Ratio::new(3, 2)),
        Ratio::from_integer(8),
    );

    assert_eq!(sample.screen_x, Ratio::new(1, 2));
    assert_eq!(sample.screen_y, Ratio::new(3, 2));
    assert_eq!(sample.depth, Ratio::from_integer(1));
    assert!(!target.is_front_facing(CanonicalView::Right));
    assert!(!target.is_front_facing(CanonicalView::Back));
}

#[test]
fn right_and_top_views_compose_signed_chart_frames() {
    let bounds = Bounds::new(2, 3, 4).unwrap();
    let source = SourcePoint::new(Ratio::from_integer(1), Ratio::new(1, 2));

    let right = TargetView::right()
        .warp_coefficients(CanonicalView::Right, bounds)
        .unwrap()
        .apply(source.clone(), Ratio::from_integer(8));
    assert_eq!(right.screen_x, Ratio::from_integer(-3));
    assert_eq!(right.screen_y, Ratio::new(1, 2));
    assert_eq!(right.depth, Ratio::from_integer(-1));

    let top = TargetView::top()
        .warp_coefficients(CanonicalView::Top, bounds)
        .unwrap()
        .apply(source, Ratio::from_integer(8));
    assert_eq!(top.screen_x, Ratio::from_integer(1));
    assert_eq!(top.screen_y, Ratio::new(1, 2));
    assert_eq!(top.depth, Ratio::from_integer(1));
}

#[test]
fn isometric_basis_is_exact_and_exposes_front_right_top_only() {
    let bounds = Bounds::new(4, 4, 4).unwrap();
    let target = TargetView::isometric();
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
fn bowl_acceptance_depth_is_the_cross_product_of_its_projection_rows() {
    let bounds = Bounds::new(32, 16, 32).unwrap();
    let target = TargetView::bowl_acceptance();
    let warp = target
        .warp_coefficients(CanonicalView::Front, bounds)
        .unwrap();
    let origin = warp.apply(
        SourcePoint::new(Ratio::from_integer(0), Ratio::from_integer(0)),
        Ratio::from_integer(0),
    );
    let world_x = warp.apply(
        SourcePoint::new(Ratio::from_integer(1), Ratio::from_integer(0)),
        Ratio::from_integer(0),
    );
    let world_y = warp.apply(
        SourcePoint::new(Ratio::from_integer(0), Ratio::from_integer(1)),
        Ratio::from_integer(0),
    );
    let world_z = warp.apply(
        SourcePoint::new(Ratio::from_integer(0), Ratio::from_integer(0)),
        Ratio::from_integer(8),
    );
    let screen_right = [
        world_x.screen_x - origin.screen_x,
        world_y.screen_x - origin.screen_x,
        world_z.screen_x - origin.screen_x,
    ];
    let screen_down = [
        world_x.screen_y - origin.screen_y,
        world_y.screen_y - origin.screen_y,
        world_z.screen_y - origin.screen_y,
    ];
    let depth = [
        world_x.depth - origin.depth,
        world_y.depth - origin.depth,
        world_z.depth - origin.depth,
    ];
    let cross = [
        screen_right[1] * screen_down[2] - screen_right[2] * screen_down[1],
        screen_right[2] * screen_down[0] - screen_right[0] * screen_down[2],
        screen_right[0] * screen_down[1] - screen_right[1] * screen_down[0],
    ];

    for index in 0..3 {
        assert_eq!(depth[index], cross[index] * 4);
    }
    assert!(target.is_front_facing(CanonicalView::Front));
    assert!(target.is_front_facing(CanonicalView::Top));
}

#[test]
fn back_left_and_bottom_presets_have_the_expected_mirroring() {
    let bounds = Bounds::new(2, 3, 4).unwrap();
    let source = SourcePoint::new(Ratio::from_integer(1), Ratio::from_integer(2));
    let relief = Ratio::from_integer(8);

    let back = TargetView::back()
        .warp_coefficients(CanonicalView::Back, bounds)
        .unwrap()
        .apply(source.clone(), relief);
    assert_eq!(back.screen_x, Ratio::from_integer(-1));
    assert_eq!(back.screen_y, Ratio::from_integer(2));
    assert_eq!(back.depth, Ratio::from_integer(-3));

    let left = TargetView::left()
        .warp_coefficients(CanonicalView::Left, bounds)
        .unwrap()
        .apply(source.clone(), relief);
    assert_eq!(left.screen_x, Ratio::from_integer(1));
    assert_eq!(left.screen_y, Ratio::from_integer(2));
    assert_eq!(left.depth, Ratio::from_integer(1));

    let bottom = TargetView::bottom()
        .warp_coefficients(CanonicalView::Bottom, bounds)
        .unwrap()
        .apply(source, relief);
    assert_eq!(bottom.screen_x, Ratio::from_integer(1));
    assert_eq!(bottom.screen_y, Ratio::from_integer(-2));
    assert_eq!(bottom.depth, Ratio::from_integer(-2));
}
