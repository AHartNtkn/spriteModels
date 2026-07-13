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
        .iter()
        .filter_map(|texel| match texel {
            DecodedTexel::Background => None,
            DecodedTexel::Relief { rgb, eighths } => Some((*rgb, *eighths)),
        })
        .collect();
    assert!(foreground_top.iter().all(|(rgb, _)| *rgb == TOP_RGB));
    assert_eq!(
        foreground_top.iter().map(|(_, depth)| *depth).max(),
        Some(64)
    );
    assert!(foreground_top.iter().any(|(_, depth)| *depth == 0));
    assert_eq!(top.texel(0, 0), Some(DecodedTexel::Background));
    assert_eq!(
        top.texel(15, 15),
        Some(DecodedTexel::Relief {
            rgb: TOP_RGB,
            eighths: 64,
        })
    );
    let mut radial_depths = BTreeMap::new();
    for y in 0..32 {
        for x in 0..32 {
            let dx = 2 * x as i32 + 1 - 32;
            let dy = 2 * y as i32 + 1 - 32;
            let distance_squared = dx * dx + dy * dy;
            assert_eq!(
                matches!(top.texel(x, y), Some(DecodedTexel::Relief { .. })),
                distance_squared <= 28_i32.pow(2),
                "radius-14 mask mismatch at ({x}, {y})"
            );
            if let Some(DecodedTexel::Relief { eighths, .. }) = top.texel(x, y) {
                if let Some(existing) = radial_depths.get(&distance_squared) {
                    assert_eq!(*existing, eighths, "equal radii must have equal relief");
                } else {
                    radial_depths.insert(distance_squared, eighths);
                }
            }
            assert_eq!(top.texel(x, y), top.texel(31 - x, y));
            assert_eq!(top.texel(x, y), top.texel(x, 31 - y));
            assert_eq!(top.texel(x, y), top.texel(y, x));
        }
    }
    assert!(
        radial_depths
            .values()
            .copied()
            .zip(radial_depths.values().copied().skip(1))
            .all(|(inner, outer)| inner >= outer)
    );

    assert!(front.texels().iter().all(|texel| matches!(
        texel,
        DecodedTexel::Background | DecodedTexel::Relief { rgb: FRONT_RGB, .. }
    )));
    let mut column_heights = Vec::new();
    for y in 0..16 {
        for x in 0..32 {
            assert_eq!(front.texel(x, y), front.texel(31 - x, y));
        }
    }
    for x in 0..32 {
        let occupied: Vec<_> = (0..16)
            .filter(|y| matches!(front.texel(x, *y), Some(DecodedTexel::Relief { .. })))
            .collect();
        assert_eq!(occupied.first(), Some(&2));
        assert!(occupied.windows(2).all(|pair| pair[1] == pair[0] + 1));
        column_heights.push(occupied.len());
    }
    assert_eq!(
        column_heights,
        column_heights.iter().rev().copied().collect::<Vec<_>>()
    );
    assert!(column_heights[15] > column_heights[0]);
    assert!(front.texels().iter().any(|texel| matches!(
        texel,
        DecodedTexel::Relief { eighths, .. } if *eighths > 0
    )));
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
                .iter()
                .all(|texel| matches!(texel, DecodedTexel::Relief { eighths: 0, .. }))
        );
    }
}

#[test]
fn two_chart_bowl_has_front_near_rim_and_top_recessed_visible_basin() {
    let model = load_path(asset("bowl.depthsprite")).unwrap();
    let frame = render_model(
        model.charts(),
        &RenderRequest::new(96, 96, TargetView::bowl_acceptance()),
    )
    .unwrap();

    let rim = frame.owner_at(48, 38).expect("near rim");
    let basin = frame.owner_at(48, 48).expect("recessed basin");
    assert_eq!(rim.view, CanonicalView::Front);
    assert_eq!(basin.view, CanonicalView::Top);
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
    assert!(matches!(
        front.texel(rim.source_x, rim.source_y),
        Some(DecodedTexel::Relief { rgb: FRONT_RGB, eighths }) if eighths > 0
    ));
    assert!(matches!(
        top.texel(basin.source_x, basin.source_y),
        Some(DecodedTexel::Relief { rgb: TOP_RGB, eighths }) if eighths > 0
    ));
    assert_eq!(
        frame.rgba_at(48, 38),
        [FRONT_RGB[0], FRONT_RGB[1], FRONT_RGB[2], 255]
    );
    assert_eq!(
        frame.rgba_at(48, 48),
        [TOP_RGB[0], TOP_RGB[1], TOP_RGB[2], 255]
    );
    assert_eq!(frame.rgba_at(0, 0), [0, 0, 0, 0]);
}
