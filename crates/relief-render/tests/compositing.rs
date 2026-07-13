use num_rational::Ratio;
use relief_core::{Bounds, CanonicalView, Chart};
use relief_render::{CameraBasis, RenderDiagnostic, RenderRequest, TargetView, render_model};

#[test]
fn exact_overlap_uses_permanent_chart_rank_and_keeps_source_color() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let front =
        Chart::from_rgba(bounds, CanonicalView::Front, 1, 1, vec![[255, 0, 0, 255]]).unwrap();
    let right =
        Chart::from_rgba(bounds, CanonicalView::Right, 1, 1, vec![[0, 255, 0, 255]]).unwrap();
    let request = RenderRequest::new(3, 3, TargetView::front_for_test());

    let reversed = render_model(&[right.clone(), front.clone()], &request).unwrap();
    let canonical = render_model(&[front, right], &request).unwrap();

    assert_eq!(reversed.pixels(), canonical.pixels());
    assert_eq!(reversed.diagnostics(), canonical.diagnostics());
    assert_eq!(reversed.rgba_at(1, 1), [255, 0, 0, 255]);
}

#[test]
fn uncovered_output_remains_transparent() {
    let frame = render_model(&[], &RenderRequest::new(2, 2, TargetView::front_for_test())).unwrap();

    assert_eq!(frame.rgba_at(0, 0), [0, 0, 0, 0]);
    assert!(frame.pixels().iter().all(|pixel| *pixel == [0, 0, 0, 0]));
}

#[test]
fn a_chart_is_not_rendered_from_its_unsupported_back_side() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let front = Chart::from_rgba(bounds, CanonicalView::Front, 1, 1, vec![[5, 6, 7, 255]]).unwrap();
    let frame = render_model(
        &[front],
        &RenderRequest::new(3, 3, TargetView::back_of_front_for_test()),
    )
    .unwrap();

    assert!(frame.pixels().iter().all(|pixel| *pixel == [0, 0, 0, 0]));
}

#[test]
fn equal_depth_color_disagreement_is_stable_and_diagnostic() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let front =
        Chart::from_rgba(bounds, CanonicalView::Front, 1, 1, vec![[255, 0, 0, 255]]).unwrap();
    let right =
        Chart::from_rgba(bounds, CanonicalView::Right, 1, 1, vec![[0, 255, 0, 255]]).unwrap();
    let frame = render_model(
        &[right, front],
        &RenderRequest::new(3, 3, TargetView::front_for_test()),
    )
    .unwrap();

    assert!(
        frame
            .diagnostics()
            .iter()
            .any(|item| matches!(item, RenderDiagnostic::EqualDepthColorConflict { .. }))
    );
}

#[test]
fn a_flat_boundary_cell_covers_its_pixel_without_expanding_public_sampling() {
    let chart = Chart::from_rgba(
        Bounds::new(1, 1, 1).unwrap(),
        CanonicalView::Front,
        1,
        1,
        vec![[23, 45, 67, 255]],
    )
    .unwrap();
    let frame = render_model(
        &[chart],
        &RenderRequest::new(1, 1, TargetView::front_for_test()),
    )
    .unwrap();

    assert_eq!(frame.rgba_at(0, 0), [23, 45, 67, 255]);
}

#[test]
fn sparse_foreground_keeps_canonical_model_framing_and_reports_missing_coverage() {
    let chart = chart_with_mask(CanonicalView::Front, 10, &[0]);
    let frame = render_model(
        &[chart],
        &RenderRequest::new(10, 1, TargetView::front_for_test()),
    )
    .unwrap();

    assert_eq!(frame.rgba_at(0, 0), [0, 30, 60, 255]);
    assert!(
        frame
            .diagnostics()
            .contains(&RenderDiagnostic::InsufficientCoverage {
                covered_pixels: 1,
                total_pixels: 10,
            })
    );
}

fn rgba_with_relief(rgb: [u8; 3], relief_eighths: u8) -> [u8; 4] {
    [rgb[0], rgb[1], rgb[2], 255 - relief_eighths]
}

fn chart_with_mask(view: CanonicalView, width: u32, occupied: &[u32]) -> Chart {
    let bounds = Bounds::new(width, 1, width).unwrap();
    let mut rgba = vec![[0, 0, 0, 0]; width as usize];
    for &x in occupied {
        rgba[x as usize] = [x as u8, 30, 60, 255];
    }
    Chart::from_rgba(bounds, view, width, 1, rgba).unwrap()
}

#[test]
fn nearest_texel_half_tie_uses_lowest_source_coordinates_without_blending() {
    let chart = Chart::from_rgba(
        Bounds::new(2, 1, 1).unwrap(),
        CanonicalView::Front,
        2,
        1,
        vec![[200, 10, 20, 255], [10, 20, 200, 255]],
    )
    .unwrap();

    let frame = render_model(
        &[chart],
        &RenderRequest::new(1, 1, TargetView::front_for_test()),
    )
    .unwrap();

    assert_eq!(frame.rgba_at(0, 0), [200, 10, 20, 255]);
}

#[test]
fn every_fold_preimage_competes_by_exact_transient_depth() {
    let chart = Chart::from_rgba(
        Bounds::new(3, 1, 1).unwrap(),
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
            Ratio::from_integer(0),
            Ratio::from_integer(0),
            Ratio::from_integer(8),
        ],
    ));

    let frame = render_model(&[chart], &RenderRequest::new(11, 1, target)).unwrap();

    assert_eq!(frame.rgba_at(8, 0), [220, 20, 20, 255]);
}

#[test]
fn relief_beyond_the_opposing_plane_is_reported_per_source_texel() {
    let chart = Chart::from_rgba(
        Bounds::new(1, 1, 1).unwrap(),
        CanonicalView::Front,
        1,
        1,
        vec![rgba_with_relief([1, 2, 3], 9)],
    )
    .unwrap();

    let frame = render_model(
        &[chart],
        &RenderRequest::new(1, 1, TargetView::front_for_test()),
    )
    .unwrap();

    assert!(
        frame
            .diagnostics()
            .contains(&RenderDiagnostic::ReliefBeyondOpposingPlane {
                view: CanonicalView::Front,
                source_x: 0,
                source_y: 0,
            })
    );
}

#[test]
fn reversed_microtriangles_report_one_deduplicated_fold_per_source_cell() {
    let chart = Chart::from_rgba(
        Bounds::new(2, 1, 1).unwrap(),
        CanonicalView::Front,
        2,
        1,
        vec![
            rgba_with_relief([1, 2, 3], 0),
            rgba_with_relief([4, 5, 6], 16),
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
            Ratio::from_integer(0),
            Ratio::from_integer(0),
            Ratio::from_integer(8),
        ],
    ));

    let frame = render_model(&[chart], &RenderRequest::new(16, 1, target)).unwrap();
    let folds: Vec<_> = frame
        .diagnostics()
        .iter()
        .filter(|diagnostic| matches!(diagnostic, RenderDiagnostic::WarpFold { .. }))
        .collect();

    assert_eq!(
        folds,
        vec![
            &RenderDiagnostic::WarpFold {
                view: CanonicalView::Front,
                source_x: 0,
                source_y: 0,
            },
            &RenderDiagnostic::WarpFold {
                view: CanonicalView::Front,
                source_x: 1,
                source_y: 0,
            }
        ]
    );
}

#[test]
fn chart_overlap_threshold_is_strictly_more_than_twenty_percent() {
    let front_at_twenty = chart_with_mask(CanonicalView::Front, 5, &[0, 1, 2, 3, 4]);
    let right_at_twenty = chart_with_mask(CanonicalView::Right, 5, &[0]);
    let exact = render_model(
        &[front_at_twenty, right_at_twenty],
        &RenderRequest::new(5, 1, TargetView::front_for_test()),
    )
    .unwrap();
    assert!(
        !exact
            .diagnostics()
            .iter()
            .any(|item| matches!(item, RenderDiagnostic::HeavyChartOverlap { .. }))
    );

    let front_over = chart_with_mask(CanonicalView::Front, 4, &[0, 1, 2, 3]);
    let right_over = chart_with_mask(CanonicalView::Right, 4, &[0]);
    let over = render_model(
        &[right_over, front_over],
        &RenderRequest::new(4, 1, TargetView::front_for_test()),
    )
    .unwrap();
    assert!(
        over.diagnostics()
            .contains(&RenderDiagnostic::HeavyChartOverlap {
                covered_pixels: 4,
                conflicting_pixels: 1,
            })
    );
}

#[test]
fn coverage_threshold_is_strictly_fewer_than_seventy_percent() {
    let exactly_seventy = chart_with_mask(CanonicalView::Front, 10, &[0, 1, 2, 3, 4, 5, 9]);
    let exact = render_model(
        &[exactly_seventy],
        &RenderRequest::new(10, 1, TargetView::front_for_test()),
    )
    .unwrap();
    assert!(
        !exact
            .diagnostics()
            .iter()
            .any(|item| matches!(item, RenderDiagnostic::InsufficientCoverage { .. }))
    );

    let under = chart_with_mask(CanonicalView::Front, 10, &[0, 1, 2, 3, 4, 9]);
    let insufficient = render_model(
        &[under],
        &RenderRequest::new(10, 1, TargetView::front_for_test()),
    )
    .unwrap();
    assert!(
        insufficient
            .diagnostics()
            .contains(&RenderDiagnostic::InsufficientCoverage {
                covered_pixels: 6,
                total_pixels: 10,
            })
    );
}

#[test]
fn diagnostics_are_sorted_deduplicated_and_do_not_change_the_winner() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let front = Chart::from_rgba(
        bounds,
        CanonicalView::Front,
        1,
        1,
        vec![rgba_with_relief([200, 0, 0], 9)],
    )
    .unwrap();
    let right = Chart::from_rgba(
        bounds,
        CanonicalView::Right,
        1,
        1,
        vec![rgba_with_relief([0, 200, 0], 9)],
    )
    .unwrap();

    let first = render_model(
        &[right.clone(), front.clone()],
        &RenderRequest::new(1, 1, TargetView::front_for_test()),
    )
    .unwrap();
    let second = render_model(
        &[front, right],
        &RenderRequest::new(1, 1, TargetView::front_for_test()),
    )
    .unwrap();

    assert_eq!(first.rgba_at(0, 0), [200, 0, 0, 255]);
    assert_eq!(first.pixels(), second.pixels());
    assert_eq!(first.diagnostics(), second.diagnostics());
    assert!(first.diagnostics().windows(2).all(|pair| pair[0] < pair[1]));
}
