use num_rational::Ratio;
use relief_core::{AuthoredModel, Bounds, CanonicalView, Chart, ResolvedCharts};
use relief_render::{CameraBasis, RenderRequest, TargetView, render_model};

fn resolved(bounds: Bounds, charts: Vec<Chart>) -> ResolvedCharts {
    AuthoredModel::new(bounds, charts).unwrap().resolve()
}

fn solid(bounds: Bounds, view: CanonicalView, pixel: [u8; 4]) -> Chart {
    let (width, height) = view.dimensions(bounds);
    Chart::from_rgba(view, width, height, vec![pixel; (width * height) as usize]).unwrap()
}

fn rgba_with_relief(rgb: [u8; 3], relief_eighths: u8) -> [u8; 4] {
    [rgb[0], rgb[1], rgb[2], 255 - relief_eighths]
}

#[test]
fn explicit_opposites_never_bleed_through_each_other() {
    let bounds = Bounds::new(2, 2, 2).unwrap();
    let front = solid(bounds, CanonicalView::Front, [255, 0, 0, 255]);
    let back = solid(bounds, CanonicalView::Back, [0, 0, 255, 255]);
    let resolved = AuthoredModel::new(bounds, vec![front, back])
        .unwrap()
        .resolve();
    let front_request = RenderRequest::new(8, 8, TargetView::front());
    let back_request = RenderRequest::new(8, 8, TargetView::back());
    assert!(
        render_model(&resolved, &front_request)
            .unwrap()
            .pixels()
            .iter()
            .all(|p| p[2] == 0)
    );
    assert!(
        render_model(&resolved, &back_request)
            .unwrap()
            .pixels()
            .iter()
            .all(|p| p[0] == 0)
    );
}

#[test]
fn one_authored_front_is_visible_as_a_derived_back_observation() {
    let bounds = Bounds::new(2, 2, 2).unwrap();
    let front = solid(bounds, CanonicalView::Front, [7, 11, 13, 255]);
    let resolved = AuthoredModel::new(bounds, vec![front]).unwrap().resolve();
    let request = RenderRequest::new(8, 8, TargetView::back());
    let rear = render_model(&resolved, &request).unwrap();
    let visible = rear
        .pixels()
        .iter()
        .enumerate()
        .find(|(_, pixel)| **pixel == [7, 11, 13, 255]);
    let (index, _) = visible.expect("derived Back observation must render");
    let x = index as u32 % rear.width();
    let y = index as u32 / rear.width();
    assert_eq!(rear.owner_at(x, y).unwrap().view, CanonicalView::Back);
}

#[test]
fn resolved_charts_are_invisible_when_edge_on() {
    let bounds = Bounds::new(2, 2, 2).unwrap();
    let front = solid(bounds, CanonicalView::Front, [7, 11, 13, 255]);
    let resolved = AuthoredModel::new(bounds, vec![front]).unwrap().resolve();
    let request = RenderRequest::new(8, 8, TargetView::left());
    let edge_on = render_model(&resolved, &request).unwrap();
    assert!(edge_on.pixels().iter().all(|pixel| pixel[3] == 0));
}

#[test]
fn a_flat_boundary_cell_covers_its_pixel() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let chart = Chart::from_rgba(CanonicalView::Front, 1, 1, vec![[23, 45, 67, 255]]).unwrap();

    let charts = resolved(bounds, vec![chart]);
    let frame = render_model(&charts, &RenderRequest::new(1, 1, TargetView::front())).unwrap();

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

    let charts = resolved(bounds, vec![chart]);
    let frame = render_model(&charts, &RenderRequest::new(1, 1, target)).unwrap();

    assert_eq!(frame.rgba_at(0, 0), [70, 80, 90, 255]);
}

#[test]
fn exact_shared_source_edge_uses_the_lowest_nearest_texel_tie() {
    let bounds = Bounds::new(2, 1, 1).unwrap();
    let chart = Chart::from_rgba(
        CanonicalView::Front,
        2,
        1,
        vec![[200, 10, 20, 255], [10, 20, 200, 255]],
    )
    .unwrap();

    let charts = resolved(bounds, vec![chart]);
    let frame = render_model(&charts, &RenderRequest::new(1, 1, TargetView::front())).unwrap();

    assert_eq!(frame.rgba_at(0, 0), [200, 10, 20, 255]);
    let owner = frame.owner_at(0, 0).unwrap();
    assert_eq!((owner.source_x, owner.source_y), (0, 0));
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

    let charts = resolved(bounds, vec![chart]);
    let flat = render_model(&charts, &RenderRequest::new(11, 1, flat_nearer)).unwrap();
    let displaced = render_model(&charts, &RenderRequest::new(11, 1, displaced_nearer)).unwrap();

    assert_eq!(flat.rgba_at(8, 0), [220, 20, 20, 255]);
    assert_eq!(displaced.rgba_at(8, 0), [20, 20, 220, 255]);
    assert_eq!(flat.owner_at(8, 0).unwrap().source_x, 0);
    assert_eq!(displaced.owner_at(8, 0).unwrap().source_x, 2);
}
