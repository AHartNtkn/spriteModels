use num_rational::Ratio;
use relief_core::{Bounds, CanonicalView, Chart, ComponentMap, ReliefField};

fn alpha(depth_eighths: u8) -> u8 {
    255 - depth_eighths
}

#[test]
fn tent_field_is_exact_at_texel_centers_and_interpolates_between_them() {
    let chart = Chart::from_rgba(
        Bounds::new(2, 1, 1).unwrap(),
        CanonicalView::Front,
        2,
        1,
        vec![[10, 0, 0, alpha(0)], [20, 0, 0, alpha(8)]],
    )
    .unwrap();
    let field = ReliefField::new(&chart);
    assert_eq!(
        field.sample(Ratio::new(1, 2), Ratio::new(1, 2)),
        Some(Ratio::from_integer(0))
    );
    assert_eq!(
        field.sample(Ratio::new(3, 2), Ratio::new(1, 2)),
        Some(Ratio::from_integer(8))
    );
    assert_eq!(
        field.sample(Ratio::from_integer(1), Ratio::new(1, 2)),
        Some(Ratio::from_integer(4))
    );
}

#[test]
fn alpha_zero_terminates_the_domain_and_components_do_not_mix() {
    let chart = Chart::from_rgba(
        Bounds::new(3, 1, 1).unwrap(),
        CanonicalView::Front,
        3,
        1,
        vec![[1, 0, 0, alpha(0)], [0, 0, 0, 0], [2, 0, 0, alpha(24)]],
    )
    .unwrap();
    let field = ReliefField::new(&chart);
    assert_eq!(
        field.sample(Ratio::new(1, 2), Ratio::new(1, 2)),
        Some(Ratio::from_integer(0))
    );
    assert_eq!(field.sample(Ratio::new(3, 2), Ratio::new(1, 2)), None);
    assert_eq!(
        field.sample(Ratio::new(5, 2), Ratio::new(1, 2)),
        Some(Ratio::from_integer(24))
    );
}

#[test]
fn diagonal_foreground_texels_remain_independent_near_their_shared_corner() {
    let chart = Chart::from_rgba(
        Bounds::new(2, 2, 1).unwrap(),
        CanonicalView::Front,
        2,
        2,
        vec![
            [1, 0, 0, alpha(0)],
            [0, 0, 0, 0],
            [0, 0, 0, 0],
            [2, 0, 0, alpha(16)],
        ],
    )
    .unwrap();
    let components = ComponentMap::label(&chart);
    let field = ReliefField::new(&chart);

    assert_ne!(components.at(0, 0), components.at(1, 1));
    assert_eq!(
        field.sample(Ratio::new(3, 4), Ratio::new(3, 4)),
        Some(Ratio::from_integer(0))
    );
    assert_eq!(
        field.sample(Ratio::new(5, 4), Ratio::new(5, 4)),
        Some(Ratio::from_integer(16))
    );
}

#[test]
fn mask_edges_and_one_pixel_wide_support_normalize_partial_tent_weight() {
    let chart = Chart::from_rgba(
        Bounds::new(2, 1, 1).unwrap(),
        CanonicalView::Front,
        2,
        1,
        vec![[1, 0, 0, alpha(4)], [2, 0, 0, alpha(12)]],
    )
    .unwrap();
    let field = ReliefField::new(&chart);

    assert_eq!(
        field.sample(Ratio::new(1, 4), Ratio::new(1, 4)),
        Some(Ratio::from_integer(4))
    );
    assert_eq!(
        field.sample(Ratio::new(5, 4), Ratio::new(1, 4)),
        Some(Ratio::from_integer(10))
    );
}

#[test]
fn sampling_rejects_chart_boundaries_background_and_extrapolation() {
    let chart = Chart::from_rgba(
        Bounds::new(3, 1, 1).unwrap(),
        CanonicalView::Front,
        3,
        1,
        vec![[0, 0, 0, 0], [1, 0, 0, alpha(8)], [0, 0, 0, 0]],
    )
    .unwrap();
    let field = ReliefField::new(&chart);

    assert_eq!(field.sample(Ratio::new(-1, 4), Ratio::new(1, 2)), None);
    assert_eq!(field.sample(Ratio::new(3, 2), Ratio::new(-1, 4)), None);
    assert_eq!(field.sample(Ratio::from_integer(3), Ratio::new(1, 2)), None);
    assert_eq!(field.sample(Ratio::new(3, 2), Ratio::from_integer(1)), None);
    assert_eq!(field.sample(Ratio::new(3, 4), Ratio::new(1, 2)), None);
    assert_eq!(field.sample(Ratio::new(9, 4), Ratio::new(1, 2)), None);
}
