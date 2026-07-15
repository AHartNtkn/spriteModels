use std::{collections::BTreeSet, path::Path};

use depthsprite_format::load_path;
use fixture_gen::{bowl_model, generate_examples};
use relief_core::{AuthoredModel, CanonicalView, Chart, DecodedTexel};

const DIAMETER: i32 = 32;
const RADIUS_DOUBLED: u32 = 32;
const HEIGHT_INTERVALS: u32 = 11;
const MAX_FRONT_RELIEF: u8 = 128;
const BASIN_DEPTH_EIGHTHS: u32 = 48;
const EXTERIOR_HEIGHT_EIGHTHS: u32 = 88;

fn foreground(chart: &Chart, x: i32, y: i32) -> bool {
    let (width, height) = chart.dimensions();
    x >= 0
        && y >= 0
        && x < width as i32
        && y < height as i32
        && matches!(
            chart.texel_at(x as u32, y as u32),
            Some(DecodedTexel::Relief { .. })
        )
}

fn relief(chart: &Chart, x: u32, y: u32) -> u8 {
    match chart.texel_at(x, y) {
        Some(DecodedTexel::Relief { eighths, .. }) => eighths,
        _ => panic!("expected foreground at ({x}, {y})"),
    }
}

fn floor_sqrt(value: u32) -> u32 {
    let mut low = 0;
    let mut high = value.min(1 << 16) + 1;
    while low + 1 < high {
        let middle = (low + high) / 2;
        if middle.saturating_mul(middle) <= value {
            low = middle;
        } else {
            high = middle;
        }
    }
    low
}

fn centered_doubled(coordinate: i32) -> i32 {
    2 * coordinate + 1 - DIAMETER
}

fn expected_front_radius_doubled(y: u32) -> u32 {
    let vertical = RADIUS_DOUBLED * y / HEIGHT_INTERVALS;
    floor_sqrt(RADIUS_DOUBLED.pow(2) - vertical.pow(2)).max(1)
}

fn is_horizontal_silhouette(chart: &Chart, x: i32, y: i32) -> bool {
    !foreground(chart, x - 1, y) || !foreground(chart, x + 1, y)
}

fn bowl() -> AuthoredModel {
    bowl_model().expect("the authored bowl must be valid")
}

#[test]
fn bowl_assigns_one_png_to_front_and_back_and_no_png_to_bottom() {
    let model = bowl();
    let front = model.chart(CanonicalView::Front).unwrap();
    let top = model.chart(CanonicalView::Top).unwrap();
    let resolved = model.resolve();

    assert!(front.supplies_opposite());
    assert!(front.mirrors_opposite());
    assert!(!top.supplies_opposite());
    assert!(!top.mirrors_opposite());
    assert!(resolved.chart(CanonicalView::Front).is_some());
    assert!(resolved.chart(CanonicalView::Back).is_some());
    assert!(resolved.chart(CanonicalView::Top).is_some());
    assert!(resolved.chart(CanonicalView::Bottom).is_none());
}

#[test]
fn bowl_back_is_the_exact_mirrored_front_with_matching_world_lighting() {
    let model = bowl();
    let front = model.chart(CanonicalView::Front).unwrap();
    let resolved = model.resolve();
    let back = resolved.chart(CanonicalView::Back).unwrap();
    let (width, height) = front.dimensions();

    for y in 0..height {
        for x in 0..width {
            assert_eq!(
                back.rgba_at(width - 1 - x, y),
                front.rgba_at(x, y),
                "world-registered bowl color and relief differ at ({x}, {y})"
            );
        }
    }
}

#[test]
fn front_is_the_sampled_lower_ellipsoid_not_a_cone() {
    let model = bowl();
    let front = model.chart(CanonicalView::Front).unwrap();

    let actual_widths = (0..12)
        .map(|y| (0..32).filter(|&x| foreground(front, x, y)).count())
        .collect::<Vec<_>>();
    let expected_widths = (0..12)
        .map(|y| {
            let radius = expected_front_radius_doubled(y);
            (0..32)
                .filter(|&x| centered_doubled(x).unsigned_abs() <= radius)
                .count()
        })
        .collect::<Vec<_>>();

    assert_eq!(actual_widths, expected_widths);
    assert_eq!(actual_widths.first(), Some(&32));
    assert_eq!(actual_widths.last(), Some(&2));
    let width_steps = actual_widths
        .windows(2)
        .map(|pair| pair[0] as i32 - pair[1] as i32)
        .collect::<BTreeSet<_>>();
    assert!(
        width_steps.len() >= 4,
        "a rounded profile must not have the cone's nearly constant row-width step: {actual_widths:?}"
    );

    for y in 0..12_u32 {
        let row_radius = expected_front_radius_doubled(y);
        for x in 0..32_u32 {
            if !foreground(front, x as i32, y as i32) {
                continue;
            }
            let offset = centered_doubled(x as i32).unsigned_abs();
            let expected = if is_horizontal_silhouette(front, x as i32, y as i32) {
                MAX_FRONT_RELIEF
            } else {
                (4 * (RADIUS_DOUBLED - floor_sqrt(row_radius.pow(2) - offset.pow(2)))) as u8
            };
            assert_eq!(
                relief(front, x, y),
                expected,
                "Front relief must come directly from row {y}'s cross-section at x={x}"
            );
        }
    }

    assert_eq!(relief(front, 15, 11), MAX_FRONT_RELIEF);
    assert_eq!(relief(front, 16, 11), MAX_FRONT_RELIEF);
}

#[test]
fn top_cavity_shares_the_rim_and_stays_strictly_inside_the_exterior() {
    let model = bowl();
    let top = model.chart(CanonicalView::Top).unwrap();

    for y in 0..32_u32 {
        for x in 0..32_u32 {
            if !foreground(top, x as i32, y as i32) {
                continue;
            }
            let dx = centered_doubled(x as i32);
            let dz = centered_doubled(y as i32);
            let radial_squared = (dx * dx + dz * dz) as u32;
            let span = floor_sqrt(RADIUS_DOUBLED.pow(2) - radial_squared);
            let on_rim = (-1..=1).any(|dy| {
                (-1..=1).any(|dx| {
                    (dx != 0 || dy != 0) && !foreground(top, x as i32 + dx, y as i32 + dy)
                })
            });
            let cavity = u32::from(relief(top, x, y));
            if on_rim {
                assert_eq!(cavity, 0, "the cavity and exterior must share the rim");
            } else {
                assert_eq!(cavity, BASIN_DEPTH_EIGHTHS * span / RADIUS_DOUBLED);
                let exterior = EXTERIOR_HEIGHT_EIGHTHS * span / RADIUS_DOUBLED;
                assert!(
                    cavity < exterior,
                    "the cavity must remain above the exterior at ({x}, {y}): {cavity} !< {exterior}"
                );
            }
        }
    }
}

#[test]
fn both_charts_have_baked_directional_lighting() {
    let model = bowl();
    for view in [CanonicalView::Front, CanonicalView::Top] {
        let colors = model
            .chart(view)
            .unwrap()
            .texels()
            .filter_map(|texel| match texel {
                DecodedTexel::Background => None,
                DecodedTexel::Relief { rgb, .. } => Some(rgb),
            })
            .collect::<BTreeSet<_>>();
        assert!(
            colors.len() >= 24,
            "{view:?} must retain a graduated value ramp that visibly encodes curvature"
        );
    }
}

#[test]
fn committed_bowl_is_exactly_the_authoritative_generated_model() {
    let committed = load_path(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("assets/examples/bowl.depthsprite"),
    )
    .unwrap();
    let generated = bowl();

    assert_eq!(committed.bounds(), generated.bounds());
    for view in [CanonicalView::Front, CanonicalView::Top] {
        assert_eq!(
            committed.chart(view).unwrap().rgba(),
            generated.chart(view).unwrap().rgba(),
            "the committed {view:?} chart must be regenerated from bowl_model"
        );
    }
}

#[test]
fn generated_bowl_package_is_byte_deterministic() {
    let temporary = tempfile::tempdir().unwrap();
    let first = temporary.path().join("first");
    let second = temporary.path().join("second");

    generate_examples(&first).unwrap();
    generate_examples(&second).unwrap();

    assert_eq!(
        std::fs::read(first.join("bowl.depthsprite")).unwrap(),
        std::fs::read(second.join("bowl.depthsprite")).unwrap(),
    );
}
