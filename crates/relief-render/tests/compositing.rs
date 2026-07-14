use num_rational::Ratio;
use relief_core::{Bounds, CanonicalView, Chart};
use relief_render::{CameraBasis, RenderRequest, TargetView, render_model};

fn rgba_with_relief(rgb: [u8; 3], relief_eighths: u8) -> [u8; 4] {
    [rgb[0], rgb[1], rgb[2], 255 - relief_eighths]
}

#[test]
fn exact_overlap_uses_permanent_chart_rank_independent_of_input_order() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let front = Chart::from_rgba(CanonicalView::Front, 1, 1, vec![[255, 0, 0, 255]]).unwrap();
    let right = Chart::from_rgba(CanonicalView::Right, 1, 1, vec![[0, 255, 0, 255]]).unwrap();
    let request = RenderRequest::new(3, 3, TargetView::front_for_test());

    let reversed = render_model(bounds, &[right.clone(), front.clone()], &request).unwrap();
    let canonical = render_model(bounds, &[front, right], &request).unwrap();

    assert_eq!(reversed.pixels(), canonical.pixels());
    assert_eq!(reversed.rgba_at(1, 1), [255, 0, 0, 255]);
}

#[test]
fn uncovered_output_remains_transparent() {
    let frame = render_model(
        Bounds::new(1, 1, 1).unwrap(),
        &[],
        &RenderRequest::new(2, 2, TargetView::front_for_test()),
    )
    .unwrap();

    assert!(frame.pixels().iter().all(|pixel| *pixel == [0, 0, 0, 0]));
}

#[test]
fn a_chart_is_not_rendered_from_its_unsupported_back_side() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let front = Chart::from_rgba(CanonicalView::Front, 1, 1, vec![[5, 6, 7, 255]]).unwrap();
    let frame = render_model(
        bounds,
        &[front],
        &RenderRequest::new(3, 3, TargetView::back_of_front_for_test()),
    )
    .unwrap();

    assert!(frame.pixels().iter().all(|pixel| *pixel == [0, 0, 0, 0]));
}

#[test]
fn a_flat_boundary_cell_covers_its_pixel() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let chart = Chart::from_rgba(CanonicalView::Front, 1, 1, vec![[23, 45, 67, 255]]).unwrap();

    let frame = render_model(
        bounds,
        &[chart],
        &RenderRequest::new(1, 1, TargetView::front_for_test()),
    )
    .unwrap();

    assert_eq!(frame.rgba_at(0, 0), [23, 45, 67, 255]);
}

#[test]
fn explicit_mirrored_camera_still_covers_the_chart() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let chart = Chart::from_rgba(CanonicalView::Front, 1, 1, vec![[70, 80, 90, 255]]).unwrap();
    let target = TargetView::from_camera(CameraBasis::new(
        [
            Ratio::from_integer(-1),
            Ratio::from_integer(0),
            Ratio::from_integer(0),
        ],
        [
            Ratio::from_integer(0),
            Ratio::from_integer(1),
            Ratio::from_integer(0),
        ],
        [
            Ratio::from_integer(0),
            Ratio::from_integer(0),
            Ratio::from_integer(1),
        ],
    ));

    let frame = render_model(bounds, &[chart], &RenderRequest::new(1, 1, target)).unwrap();

    assert_eq!(frame.rgba_at(0, 0), [70, 80, 90, 255]);
}

#[test]
fn exact_shared_source_edge_has_one_stable_owner() {
    let bounds = Bounds::new(2, 1, 1).unwrap();
    let chart = Chart::from_rgba(
        CanonicalView::Front,
        2,
        1,
        vec![[200, 10, 20, 255], [10, 20, 200, 255]],
    )
    .unwrap();

    let frame = render_model(
        bounds,
        &[chart],
        &RenderRequest::new(1, 1, TargetView::front_for_test()),
    )
    .unwrap();

    assert_eq!(frame.rgba_at(0, 0), [10, 20, 200, 255]);
    let owner = frame.owner_at(0, 0).unwrap();
    assert_eq!((owner.source_x, owner.source_y), (1, 0));
}

#[test]
fn overlapping_relief_preimages_compete_by_exact_transient_depth() {
    let bounds = Bounds::new(3, 1, 1).unwrap();
    let chart = Chart::from_rgba(
        CanonicalView::Front,
        3,
        1,
        vec![
            rgba_with_relief([220, 20, 20], 0),
            [0, 0, 0, 0],
            rgba_with_relief([20, 20, 220], 2),
        ],
    )
    .unwrap();
    let screen_right = [
        Ratio::from_integer(1),
        Ratio::from_integer(0),
        Ratio::from_integer(-8),
    ];
    let screen_down = [
        Ratio::from_integer(0),
        Ratio::from_integer(1),
        Ratio::from_integer(0),
    ];
    let flat_nearer = TargetView::from_camera(CameraBasis::new(
        screen_right,
        screen_down,
        [
            Ratio::from_integer(0),
            Ratio::from_integer(0),
            Ratio::from_integer(8),
        ],
    ));
    let displaced_nearer = TargetView::from_camera(CameraBasis::new(
        screen_right,
        screen_down,
        [
            Ratio::from_integer(-10),
            Ratio::from_integer(0),
            Ratio::from_integer(8),
        ],
    ));

    let flat = render_model(
        bounds,
        std::slice::from_ref(&chart),
        &RenderRequest::new(11, 1, flat_nearer),
    )
    .unwrap();
    let displaced = render_model(
        bounds,
        &[chart],
        &RenderRequest::new(11, 1, displaced_nearer),
    )
    .unwrap();

    assert_eq!(flat.rgba_at(8, 0), [220, 20, 20, 255]);
    assert_eq!(displaced.rgba_at(8, 0), [20, 20, 220, 255]);
    assert_eq!(flat.owner_at(8, 0).unwrap().source_x, 0);
    assert_eq!(displaced.owner_at(8, 0).unwrap().source_x, 2);
}

#[test]
fn equal_depth_relief_overlap_has_a_stable_source_owner() {
    let bounds = Bounds::new(3, 1, 1).unwrap();
    let chart = Chart::from_rgba(
        CanonicalView::Front,
        3,
        1,
        vec![
            rgba_with_relief([220, 20, 20], 0),
            rgba_with_relief([20, 220, 20], 16),
            rgba_with_relief([20, 20, 220], 0),
        ],
    )
    .unwrap();
    let target = TargetView::from_camera(CameraBasis::new(
        [
            Ratio::from_integer(1),
            Ratio::from_integer(0),
            Ratio::from_integer(-8),
        ],
        [
            Ratio::from_integer(0),
            Ratio::from_integer(1),
            Ratio::from_integer(0),
        ],
        [
            Ratio::from_integer(-1),
            Ratio::from_integer(0),
            Ratio::from_integer(8),
        ],
    ));

    let frame = render_model(bounds, &[chart], &RenderRequest::new(11, 1, target)).unwrap();

    assert_eq!(frame.rgba_at(7, 0), [220, 20, 20, 255]);
    assert_eq!(frame.owner_at(7, 0).unwrap().source_x, 0);
}
