use std::path::{Path, PathBuf};

use depthsprite_format::load_path;
use relief_core::{CanonicalView, DecodedTexel};
use relief_render::{FrameBuffer, RenderRequest, TargetView, render_model};

fn asset(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("assets/examples")
        .join(name)
}

fn relief_at(model: &relief_core::AuthoredModel, view: CanonicalView, x: u32, y: u32) -> u8 {
    match model.chart(view).unwrap().texel_at(x, y) {
        Some(DecodedTexel::Relief { eighths, .. }) => eighths,
        _ => panic!("render owner must identify authored foreground"),
    }
}

fn owner_positions(
    frame: &FrameBuffer,
    predicate: impl Fn(CanonicalView, u32, u32) -> bool,
) -> Vec<(u32, u32)> {
    let mut positions = Vec::new();
    for y in 0..frame.height() {
        for x in 0..frame.width() {
            if let Some(owner) = frame.owner_at(x, y)
                && predicate(owner.view, owner.source_x, owner.source_y)
            {
                positions.push((x, y));
            }
        }
    }
    positions
}

fn regions_touch(first: &[(u32, u32)], second: &[(u32, u32)]) -> bool {
    first.iter().any(|&(first_x, first_y)| {
        second.iter().any(|&(second_x, second_y)| {
            first_x.abs_diff(second_x) <= 1
                && first_y.abs_diff(second_y) <= 1
                && (first_x != second_x || first_y != second_y)
        })
    })
}

#[test]
fn foundational_bowl_render_has_basin_rim_exterior_and_touching_ownership() {
    let model = load_path(asset("bowl.depthsprite")).unwrap();
    let frame = render_model(
        &model.resolve(),
        &RenderRequest::new(128, 96, TargetView::bowl_acceptance()),
    )
    .unwrap();

    let basin = owner_positions(&frame, |view, x, y| {
        view == CanonicalView::Top && relief_at(&model, view, x, y) > 0
    });
    let rim = owner_positions(&frame, |view, x, y| {
        view == CanonicalView::Top && relief_at(&model, view, x, y) == 0
    });
    let exterior = owner_positions(&frame, |view, _, _| view == CanonicalView::Front);

    assert!(!basin.is_empty(), "the recessed Top basin must render");
    assert!(!rim.is_empty(), "the zero-relief Top rim must render");
    assert!(!exterior.is_empty(), "the Front exterior must render");
    assert!(
        regions_touch(&basin, &rim),
        "the rendered basin must meet the rendered rim"
    );
    assert!(
        regions_touch(&rim, &exterior),
        "Top and Front ownership must touch in an eight-neighbor output neighborhood"
    );
}
