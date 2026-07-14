use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use depthsprite_format::load_path;
use relief_core::{Bounds, CanonicalView, DecodedTexel};
use relief_render::{RenderRequest, TargetView, render_model};

const TOP_RGB: [u8; 3] = [216, 156, 85];
const FRONT_RGB: [u8; 3] = [144, 76, 52];

fn asset(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("assets/examples")
        .join(name)
}

fn is_foreground(chart: &relief_core::Chart, x: i32, y: i32) -> bool {
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

fn is_eight_neighbor_boundary(chart: &relief_core::Chart, x: i32, y: i32) -> bool {
    is_foreground(chart, x, y)
        && (-1..=1).any(|dy| {
            (-1..=1).any(|dx| (dx != 0 || dy != 0) && !is_foreground(chart, x + dx, y + dy))
        })
}

#[test]
fn generated_bowl_has_exact_two_view_schema_radial_extrema_and_symmetry() {
    let model = load_path(asset("bowl.depthsprite")).unwrap();
    assert_eq!(model.bounds(), Bounds::new(32, 16, 32).unwrap());
    assert_eq!(
        model
            .charts()
            .iter()
            .map(|chart| chart.view())
            .collect::<Vec<_>>(),
        vec![CanonicalView::Front, CanonicalView::Top]
    );
    let front = &model.charts()[0];
    let top = &model.charts()[1];

    let foreground_top: Vec<_> = top
        .texels()
        .filter_map(|texel| match texel {
            DecodedTexel::Background => None,
            DecodedTexel::Relief { rgb, eighths } => Some((rgb, eighths)),
        })
        .collect();
    assert!(foreground_top.iter().all(|(rgb, _)| *rgb == TOP_RGB));
    assert_eq!(
        foreground_top.iter().map(|(_, depth)| *depth).max(),
        Some(64)
    );
    assert!(foreground_top.iter().any(|(_, depth)| *depth == 0));
    assert_eq!(top.texel_at(0, 0), Some(DecodedTexel::Background));
    assert_eq!(
        top.texel_at(15, 15),
        Some(DecodedTexel::Relief {
            rgb: TOP_RGB,
            eighths: 64,
        })
    );
    let mut radial_depths = BTreeMap::new();
    let mut central_plateau = Vec::new();
    for y in 0..32 {
        for x in 0..32 {
            let dx = 2 * x as i32 + 1 - 32;
            let dy = 2 * y as i32 + 1 - 32;
            let distance_squared = dx * dx + dy * dy;
            assert_eq!(
                matches!(top.texel_at(x, y), Some(DecodedTexel::Relief { .. })),
                distance_squared <= 28_i32.pow(2),
                "radius-14 mask mismatch at ({x}, {y})"
            );
            if let Some(DecodedTexel::Relief { eighths, .. }) = top.texel_at(x, y) {
                if is_eight_neighbor_boundary(top, x as i32, y as i32) {
                    assert_eq!(eighths, 0, "8-neighbor boundary must be zero at ({x}, {y})");
                }
                if eighths == 64 {
                    central_plateau.push((x, y));
                }
                if let Some(existing) = radial_depths.get(&distance_squared) {
                    assert_eq!(*existing, eighths, "equal radii must have equal relief");
                } else {
                    radial_depths.insert(distance_squared, eighths);
                }
            }
            assert_eq!(top.texel_at(x, y), top.texel_at(31 - x, y));
            assert_eq!(top.texel_at(x, y), top.texel_at(x, 31 - y));
            assert_eq!(top.texel_at(x, y), top.texel_at(y, x));
        }
    }
    assert!(
        radial_depths
            .values()
            .copied()
            .zip(radial_depths.values().copied().skip(1))
            .all(|(inner, outer)| inner >= outer)
    );
    assert_eq!(
        central_plateau,
        vec![(15, 15), (16, 15), (15, 16), (16, 16)]
    );

    assert!(front.texels().all(|texel| matches!(
        texel,
        DecodedTexel::Background | DecodedTexel::Relief { rgb: FRONT_RGB, .. }
    )));
    let mut column_heights = Vec::new();
    let mut column_relief = Vec::new();
    for y in 0..16 {
        for x in 0..32 {
            assert_eq!(front.texel_at(x, y), front.texel_at(31 - x, y));
        }
    }
    for x in 0..32 {
        let occupied: Vec<_> = (0..16)
            .filter(|y| matches!(front.texel_at(x, *y), Some(DecodedTexel::Relief { .. })))
            .collect();
        let doubled_offset = (2 * x as i32 + 1 - 32).unsigned_abs();
        let expected_height =
            ((31_u32.pow(2) - doubled_offset.pow(2)) * 10 / 31_u32.pow(2)).saturating_sub(1);
        assert_eq!(occupied, (2..=4 + expected_height).collect::<Vec<_>>());
        column_heights.push(occupied.len());
        let reliefs = occupied
            .iter()
            .map(|y| match front.texel_at(x, *y) {
                Some(DecodedTexel::Relief { eighths, .. }) => eighths,
                _ => unreachable!("occupied texel is foreground"),
            })
            .collect::<Vec<_>>();
        assert!(reliefs.windows(2).all(|pair| pair[0] == pair[1]));
        let rounded_root = (0_u32..=32)
            .min_by_key(|candidate| {
                (*candidate)
                    .pow(2)
                    .abs_diff(32_u32.pow(2) - doubled_offset.pow(2))
            })
            .unwrap();
        assert_eq!(reliefs[0], ((32 - rounded_root) * 4) as u8);
        column_relief.push(reliefs[0]);
    }
    assert_eq!(
        column_heights,
        column_heights.iter().rev().copied().collect::<Vec<_>>()
    );
    assert!(column_heights[15] > column_heights[0]);
    assert!(
        column_heights
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            >= 4
    );
    assert_eq!(
        column_relief,
        column_relief.iter().rev().copied().collect::<Vec<_>>()
    );
    assert!(
        column_relief[..16]
            .windows(2)
            .all(|pair| pair[0] >= pair[1])
    );
    assert_eq!(column_relief.iter().copied().min(), Some(0));
    assert_eq!(column_relief.iter().copied().max(), Some(96));
}

#[test]
fn generated_block_is_three_flat_canonical_charts() {
    let model = load_path(asset("block.depthsprite")).unwrap();
    assert_eq!(
        model
            .charts()
            .iter()
            .map(|chart| chart.view())
            .collect::<Vec<_>>(),
        vec![
            CanonicalView::Front,
            CanonicalView::Right,
            CanonicalView::Top,
        ]
    );
    for chart in model.charts() {
        assert!(
            chart
                .texels()
                .all(|texel| matches!(texel, DecodedTexel::Relief { eighths: 0, .. }))
        );
    }
}

#[test]
fn two_chart_bowl_has_front_near_rim_and_top_recessed_visible_basin() {
    let model = load_path(asset("bowl.depthsprite")).unwrap();
    let frame = render_model(
        model.bounds(),
        model.charts(),
        &RenderRequest::new(96, 96, TargetView::bowl_acceptance()),
    )
    .unwrap();

    let rim = frame.owner_at(48, 67).expect("near rim");
    let basin = frame.owner_at(48, 48).expect("recessed basin");
    assert_eq!(rim.view, CanonicalView::Front);
    assert_eq!(basin.view, CanonicalView::Top);
    assert_eq!((rim.source_x, rim.source_y), (27, 2));
    assert_eq!((basin.source_x, basin.source_y), (16, 16));
    let front = model
        .charts()
        .iter()
        .find(|chart| chart.view() == CanonicalView::Front)
        .unwrap();
    let top = model
        .charts()
        .iter()
        .find(|chart| chart.view() == CanonicalView::Top)
        .unwrap();
    assert_eq!(
        front.texel_at(rim.source_x, rim.source_y),
        Some(DecodedTexel::Relief {
            rgb: FRONT_RGB,
            eighths: 40,
        })
    );
    assert_eq!(
        top.texel_at(basin.source_x, basin.source_y),
        Some(DecodedTexel::Relief {
            rgb: TOP_RGB,
            eighths: 64,
        })
    );
    assert_eq!(
        frame.rgba_at(48, 67),
        [FRONT_RGB[0], FRONT_RGB[1], FRONT_RGB[2], 255]
    );
    assert_eq!(
        frame.rgba_at(48, 48),
        [TOP_RGB[0], TOP_RGB[1], TOP_RGB[2], 255]
    );
    assert_eq!(frame.rgba_at(0, 0), [0, 0, 0, 0]);
}
