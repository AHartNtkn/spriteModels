//! Whole-pipeline properties on the real GLB fixtures, checked with the
//! continuity labels as an independent oracle against the emitted charts.
//!
//! The whole module is gated `#[cfg(test)]` at its `mod` declaration in
//! `lib.rs`; an inner `#![cfg(test)]` here would duplicate that gate
//! (`clippy::duplicated_attributes`).

use std::path::PathBuf;

use crate::capture::run_capture;
use crate::continuity::side_continuity;
use crate::{ImportSettings, TriangleScene, box_space_scene, convert_box_space, load_scene};

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
fn assert_no_fabricated_adjacency(name: &str, longest: u32) {
    let scene = fixture(name);
    let settings = ImportSettings {
        longest_axis_pixels: longest,
        ..Default::default()
    };
    let (box_scene, bounds) =
        box_space_scene(&scene, settings.rotation, settings.longest_axis_pixels)
            .expect("box space");
    let model = convert_box_space(&box_scene, bounds, &settings).expect("converts");
    let pipeline = run_capture(&box_scene, bounds, &settings);
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
                            "{name}@{longest}: fabricated adjacency in {:?} at \
                             ({x},{y})-({nx},{ny})",
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
    assert_no_fabricated_adjacency("stanford-bunny.glb", 63);
    assert_no_fabricated_adjacency("stanford-bunny.glb", 32);
}

#[test]
fn teapot_charts_contain_no_fabricated_adjacency() {
    assert_no_fabricated_adjacency("teapot.glb", 63);
}

#[test]
fn earth_charts_contain_no_fabricated_adjacency() {
    assert_no_fabricated_adjacency("earth.glb", 63);
}

#[test]
fn dragon_charts_contain_no_fabricated_adjacency() {
    assert_no_fabricated_adjacency("xyzrgb_dragon.glb", 63);
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
