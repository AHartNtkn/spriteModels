//! Whole-pipeline properties on the real GLB fixtures, checked with the
//! continuity labels as an independent oracle against the emitted charts.
//!
//! The whole module is gated `#[cfg(test)]` at its `mod` declaration in
//! `lib.rs`; an inner `#![cfg(test)]` here would duplicate that gate
//! (`clippy::duplicated_attributes`).

use std::path::PathBuf;

use relief_core::CanonicalView;

use crate::capture::run_capture;
use crate::continuity::side_continuity;
use crate::{
    ImportSettings, TriangleScene, box_space_scene, convert, convert_box_space, load_scene,
};

fn fixture(name: &str) -> TriangleScene {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    assert!(
        path.exists(),
        "Missing fixture {}. Provision it per tests/fixtures/README.md.",
        path.display()
    );
    load_scene(&path).unwrap_or_else(|error| panic!("{name} must load: {error}"))
}

/// The chart invariant, checked from the outside: no emitted chart may
/// contain a 4-adjacent covered pair whose edge the continuity oracle
/// labels cut — that adjacency renders as a fabricated wall (the bunny
/// ear spikes). The oracle recomputes capture-side state independently of
/// the pipeline under test.
fn assert_no_fabricated_adjacency(name: &str, settings: &ImportSettings) {
    let scene = fixture(name);
    let (box_scene, bounds) =
        box_space_scene(&scene, settings.rotation, settings.longest_axis_pixels)
            .expect("box space");
    let model = convert_box_space(&box_scene, bounds, settings).expect("converts");
    let pipeline = run_capture(&box_scene, bounds, settings);
    for side in &pipeline.sides {
        let labels = side_continuity(&box_scene.triangles, &side.continuity_view());
        let chart = model.chart(side.view).expect("chart present");
        let covered = |x: u32, y: u32| chart.rgba()[(y * side.width + x) as usize][3] != 0;
        for y in 0..side.height {
            for x in 0..side.width {
                for (nx, ny) in [(x + 1, y), (x, y + 1)] {
                    if nx >= side.width || ny >= side.height {
                        continue;
                    }
                    if covered(x, y) && covered(nx, ny) {
                        assert!(
                            labels.connected(x, y, nx, ny),
                            "{name}@{}: fabricated adjacency in {:?} at \
                             ({x},{y})-({nx},{ny})",
                            settings.longest_axis_pixels,
                            side.view
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn bunny_charts_contain_no_fabricated_adjacency() {
    assert_no_fabricated_adjacency(
        "stanford-bunny.glb",
        &ImportSettings {
            longest_axis_pixels: 63,
            ..Default::default()
        },
    );
    assert_no_fabricated_adjacency(
        "stanford-bunny.glb",
        &ImportSettings {
            longest_axis_pixels: 32,
            ..Default::default()
        },
    );
}

#[test]
fn teapot_charts_contain_no_fabricated_adjacency() {
    assert_no_fabricated_adjacency(
        "teapot.glb",
        &ImportSettings {
            longest_axis_pixels: 63,
            ..Default::default()
        },
    );
}

#[test]
fn earth_charts_contain_no_fabricated_adjacency() {
    assert_no_fabricated_adjacency(
        "earth.glb",
        &ImportSettings {
            longest_axis_pixels: 63,
            ..Default::default()
        },
    );
}

#[test]
fn dragon_charts_contain_no_fabricated_adjacency() {
    assert_no_fabricated_adjacency(
        "xyzrgb_dragon.glb",
        &ImportSettings {
            longest_axis_pixels: 63,
            ..Default::default()
        },
    );
}

/// The chart invariant must hold at non-default orientations too (spec:
/// real-model runs at several bounds and rotations): quarter turn about
/// y composed with the default glTF mapping.
#[test]
fn bunny_rotated_charts_contain_no_fabricated_adjacency() {
    let default = ImportSettings::default();
    let quarter = [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]];
    let mut rotation = [[0.0f32; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            for (k, d_row) in default.rotation.iter().enumerate() {
                rotation[i][j] += quarter[i][k] * d_row[j];
            }
        }
    }
    let settings = ImportSettings {
        rotation,
        ..Default::default()
    };
    assert_no_fabricated_adjacency("stanford-bunny.glb", &settings);
}

/// The fixpoint's end-state theorem: every covered sample is kept,
/// banned, or defers to a strictly better candidate that is kept. The
/// old pipeline violated this (cut texels were dropped while worse sides
/// still deferred to them); the new one satisfies it by construction, and
/// this pins it against regressions.
#[test]
fn every_covered_sample_is_kept_banned_or_deferred_to_a_keeper() {
    for (name, longest) in [("stanford-bunny.glb", 63u32), ("teapot.glb", 63)] {
        let scene = fixture(name);
        let settings = ImportSettings {
            longest_axis_pixels: longest,
            ..Default::default()
        };
        let (box_scene, bounds) =
            box_space_scene(&scene, settings.rotation, settings.longest_axis_pixels)
                .expect("box space");
        let pipeline = run_capture(&box_scene, bounds, &settings);
        for (s_idx, side) in pipeline.sides.iter().enumerate() {
            for y in 0..side.height {
                for x in 0..side.width {
                    let idx = side.index(x, y);
                    if !side.depth[idx].is_finite() {
                        continue;
                    }
                    if pipeline.ownership.kept[s_idx][idx] || pipeline.ownership.banned[s_idx][idx]
                    {
                        continue;
                    }
                    let (_, better) =
                        crate::capture::better_candidates(&pipeline.sides, s_idx, x, y);
                    assert!(
                        better
                            .iter()
                            .any(|c| pipeline.ownership.kept[c.side][c.index]),
                        "{name}: orphaned sample {:?} ({x},{y})",
                        side.view
                    );
                }
            }
        }
    }
}

/// No hidden order-dependence: rotating the model a quarter turn about
/// the vertical axis must produce the correspondingly rotated Top chart.
/// With R = quarter turn about y, (x,y,z) -> (z, y, -x), and cube bounds
/// n = 8, rotated box coords are X = Z0, Y = Y0, Z = n - X0, so
/// rotatedTop(u', v') == originalTop(u = n-1-v', v = u'). Only alpha
/// (relief) is compared: RGB carries box-frame lighting, which
/// legitimately differs between the two orientations.
/// The tab is deliberately off-center in x (range [0.125, 0.625] not [0.25, 0.75])
/// to ensure the mapping's reflection component (n-1-v') is discriminated — a
/// centered tab produces row-palindromic alpha coverage that would hide a
/// reflection error.
#[test]
fn quarter_turn_rotation_maps_the_top_chart() {
    let tri3 = |a: [f32; 3], b: [f32; 3], c: [f32; 3]| crate::Triangle {
        positions: [a, b, c],
        normals: [[0.0, -1.0, 0.0]; 3],
        uvs: [[0.0, 0.0]; 3],
        colors: [[1.0, 1.0, 1.0, 1.0]; 3],
        material: 0,
    };
    let quad4 = |p0: [f32; 3], p1: [f32; 3], p2: [f32; 3], p3: [f32; 3]| {
        [tri3(p0, p1, p2), tri3(p0, p2, p3)]
    };
    let mut triangles = Vec::new();
    // Mesh-space tab-over-slanted-floor inside the unit cube, the same
    // scene as the rescue regression in tests/capture.rs except the tab is
    // deliberately off-center in x (see the reflection-discrimination
    // rationale in this test's doc comment above).
    triangles.extend(quad4(
        [0.0, 0.125, 0.0],
        [1.0, 0.125, 0.0],
        [1.0, 0.375, 1.0],
        [0.0, 0.375, 1.0],
    ));
    triangles.extend(quad4(
        [0.125, 0.0625, 0.25],
        [0.625, 0.0625, 0.25],
        [0.625, 0.0625, 0.75],
        [0.125, 0.0625, 0.75],
    ));
    triangles.push(tri3([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0001, 0.5, 0.0]));
    triangles.push(tri3([1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.9999, 0.5, 1.0]));
    let scene = TriangleScene {
        triangles,
        materials: vec![crate::Material {
            base_color_factor: [1.0, 1.0, 1.0, 1.0],
            base_color_texture: None,
            alpha_cutoff: None,
        }],
    };
    const IDENTITY: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    const QUARTER_TURN_Y: [[f32; 3]; 3] = [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]];
    let settings = |rotation| ImportSettings {
        rotation,
        longest_axis_pixels: 8,
        ..Default::default()
    };
    let original = convert(&scene, &settings(IDENTITY)).expect("converts");
    let rotated = convert(&scene, &settings(QUARTER_TURN_Y)).expect("converts");
    let n = 8u32;
    for model in [&original, &rotated] {
        let b = model.bounds();
        assert_eq!((b.width(), b.height(), b.depth()), (n, n, n), "cube bounds");
    }
    let top_a = original.chart(CanonicalView::Top).expect("top");
    let top_b = rotated.chart(CanonicalView::Top).expect("top");
    assert!(
        top_a.rgba().iter().any(|texel| texel[3] != 0),
        "scene must produce a nonempty Top chart"
    );
    for v in 0..n {
        for u in 0..n {
            let b = top_b.rgba()[(v * n + u) as usize][3];
            let a = top_a.rgba()[(u * n + (n - 1 - v)) as usize][3];
            assert_eq!(a, b, "Top alpha mismatch at rotated ({u},{v})");
        }
    }
}
