use num_rational::Ratio;
use relief_core::{Bounds, CanonicalView, Chart, ReliefField};

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
