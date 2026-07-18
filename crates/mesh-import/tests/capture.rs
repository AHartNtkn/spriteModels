use std::f64::consts::PI;

use mesh_import::{
    ALL_VIEWS, ImportError, ImportSettings, Material, SideMode, Triangle, TriangleScene, convert,
    derived_bounds,
};
use relief_core::CanonicalView;

const IDENTITY: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn plain_material() -> Material {
    Material {
        base_color_factor: [1.0, 1.0, 1.0, 1.0],
        base_color_texture: None,
        alpha_cutoff: None,
    }
}

fn tri(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> Triangle {
    Triangle {
        positions: [a, b, c],
        normals: [[0.0, 0.0, -1.0]; 3],
        uvs: [[0.0, 0.0]; 3],
        colors: [[1.0, 1.0, 1.0, 1.0]; 3],
        material: 0,
    }
}

/// Axis-aligned unit-cube surface (12 triangles) spanning (0,0,0)..(1,1,1).
fn cube() -> TriangleScene {
    let corner = |mask: u8| {
        [
            f32::from(mask & 1),
            f32::from((mask >> 1) & 1),
            f32::from((mask >> 2) & 1),
        ]
    };
    // Each face as two triangles; winding is irrelevant (two-sided raster).
    let faces: [[u8; 4]; 6] = [
        [0, 1, 3, 2], // z = 0
        [4, 5, 7, 6], // z = 1
        [0, 1, 5, 4], // y = 0
        [2, 3, 7, 6], // y = 1
        [0, 2, 6, 4], // x = 0
        [1, 3, 7, 5], // x = 1
    ];
    let mut triangles = Vec::new();
    for face in faces {
        let p = face.map(corner);
        triangles.push(tri(p[0], p[1], p[2]));
        triangles.push(tri(p[0], p[2], p[3]));
    }
    TriangleScene {
        triangles,
        materials: vec![plain_material()],
    }
}

fn settings(longest: u32) -> ImportSettings {
    ImportSettings {
        longest_axis_pixels: longest,
        ..ImportSettings::default()
    }
}

/// Spec ("Fitting"): the default import rotation is the half-turn about X
/// that maps glTF's +Y-up/+Z-toward-viewer convention onto the box frame's
/// y-down/+z-front convention, so identity (which would import every model
/// upside down and back-to-front) must not be the default.
#[test]
fn default_rotation_maps_gltf_up_to_box_up() {
    let rotation = ImportSettings::default().rotation;
    let gltf_up = [0.0, 1.0, 0.0];
    let gltf_front = [0.0, 0.0, 1.0];
    let rotate = |v: [f32; 3]| {
        [
            rotation[0][0] * v[0] + rotation[0][1] * v[1] + rotation[0][2] * v[2],
            rotation[1][0] * v[0] + rotation[1][1] * v[1] + rotation[1][2] * v[2],
            rotation[2][0] * v[0] + rotation[2][1] * v[1] + rotation[2][2] * v[2],
        ]
    };
    assert_eq!(
        rotate(gltf_up),
        [0.0, -1.0, 0.0],
        "glTF's +Y-up must map to the box frame's -y (down-positive box y)"
    );
    assert_eq!(
        rotate(gltf_front),
        [0.0, 0.0, -1.0],
        "glTF's +Z-toward-viewer must map to the box frame's -z (front)"
    );
    let det = rotation[0][0] * (rotation[1][1] * rotation[2][2] - rotation[1][2] * rotation[2][1])
        - rotation[0][1] * (rotation[1][0] * rotation[2][2] - rotation[1][2] * rotation[2][0])
        + rotation[0][2] * (rotation[1][0] * rotation[2][1] - rotation[1][1] * rotation[2][0]);
    assert_eq!(
        det, 1.0,
        "the default rotation must be a rotation, not a mirror"
    );
}

#[test]
fn cube_spanning_the_box_captures_relief_zero_on_all_six_faces() {
    // This test pins the cube's raw scene coordinates to the box frame, not
    // the default import rotation: identity is set explicitly.
    let config = ImportSettings {
        rotation: IDENTITY,
        ..settings(8)
    };
    let model = convert(&cube(), &config).expect("cube converts");
    assert_eq!(model.charts().len(), 6);
    for chart in model.charts() {
        let (width, height) = chart.dimensions();
        assert_eq!((width, height), (8, 8), "{:?}", chart.view());
        for texel in chart.rgba() {
            // Surface exactly on the box face: relief 0, alpha 255.
            assert_eq!(texel[3], 255, "{:?}", chart.view());
        }
    }
}

#[test]
fn geometry_past_the_midplane_is_dropped() {
    // A quad at z = 0.9 of a unit-depth box. Extents: x [0,1], y [0,1],
    // z [0,0.9] (from the sliver below) -> longest = 1, scale = 8,
    // depth = ceil(0.9 * 8) = ceil(7.2) = 8, so h_max = 4 * 8 = 32, and
    // centering offset_z = (8 - 7.2) / 2 = 0.4. The quad's box-space depth
    // from Front is 0.9 * 8 + 0.4 = 7.6px, quantizing to relief
    // round(8 * 7.6) = 61, past h_max = 32: the texel must be dropped
    // (empty), not clamped to h_max.
    // A single edge-on triangle spans z = 0..0.9 to give the bounding box
    // its full depth; it projects to (near-)zero area from the Front, so
    // it cannot win the probed interior texel.
    let scene = TriangleScene {
        triangles: vec![
            tri([0.0, 0.0, 0.9], [1.0, 0.0, 0.9], [1.0, 1.0, 0.9]),
            tri([0.0, 0.0, 0.9], [1.0, 1.0, 0.9], [0.0, 1.0, 0.9]),
            tri([0.0, 0.0, 0.0], [0.0, 0.0, 0.9], [0.0, 0.0001, 0.45]),
        ],
        materials: vec![plain_material()],
    };
    // This test pins the quad's raw scene z to an analytic box-space depth,
    // not the default import rotation: identity is set explicitly.
    let mut config = ImportSettings {
        rotation: IDENTITY,
        ..settings(8)
    };
    let mut modes = config.side_modes;
    for side in [
        CanonicalView::Back,
        CanonicalView::Left,
        CanonicalView::Right,
        CanonicalView::Top,
        CanonicalView::Bottom,
    ] {
        modes.set(side, SideMode::Off).expect("legal mode");
    }
    config.side_modes = modes;
    let model = convert(&scene, &config).expect("converts");
    let chart = model.chart(CanonicalView::Front).expect("front chart");
    let center = chart.rgba()[4 * 8 + 4];
    assert_eq!(
        center,
        [0, 0, 0, 0],
        "geometry past the midplane must be dropped (empty texel), not clamped"
    );
}

#[test]
fn deep_feature_belongs_to_the_side_that_reaches_it() {
    // A two-part scene: a "slab" near the Front face and a "knob" near the
    // Back face, on disjoint x ranges so each side's coverage is probed
    // independently. An edge-on sliver at x = 0 spans z = 0..0.9 to give
    // the bounding box its full depth without covering any probed texel.
    //
    // Extents: x [0,1] (slab [0,0.4] union knob [0.6,1.0]), y [0,1],
    // z [0,0.9] (sliver). longest = max(1, 1, 0.9) = 1, scale = 8 / 1 = 8.
    // width = ceil(1*8) = 8, height = ceil(1*8) = 8, depth = ceil(0.9*8) =
    // ceil(7.2) = 8. offset_x = offset_y = 0 (extent*scale already equals
    // the dim); offset_z = (8 - 7.2) / 2 = 0.4. h_max (Front/Back, driven
    // by depth) = 4 * 8 = 32.
    let slab_z = 0.05_f64;
    let knob_z = 0.9_f64;
    let scene = TriangleScene {
        triangles: vec![
            // Slab: x in [0, 0.4], full y, z = 0.05.
            tri([0.0, 0.0, 0.05], [0.4, 0.0, 0.05], [0.4, 1.0, 0.05]),
            tri([0.0, 0.0, 0.05], [0.4, 1.0, 0.05], [0.0, 1.0, 0.05]),
            // Knob: x in [0.6, 1.0], full y, z = 0.9.
            tri([0.6, 0.0, 0.9], [1.0, 0.0, 0.9], [1.0, 1.0, 0.9]),
            tri([0.6, 0.0, 0.9], [1.0, 1.0, 0.9], [0.6, 1.0, 0.9]),
            // Edge-on sliver at x = 0, giving the box its full z extent.
            tri([0.0, 0.0, 0.0], [0.0, 0.0, 0.9], [0.0, 0.0001, 0.45]),
        ],
        materials: vec![plain_material()],
    };
    // This test pins the slab/knob's raw scene z to analytic box-space
    // depths, not the default import rotation: identity is set explicitly.
    let mut config = ImportSettings {
        rotation: IDENTITY,
        ..settings(8)
    };
    let mut modes = config.side_modes;
    for side in [
        CanonicalView::Left,
        CanonicalView::Right,
        CanonicalView::Top,
        CanonicalView::Bottom,
    ] {
        modes.set(side, SideMode::Off).expect("legal mode");
    }
    config.side_modes = modes;
    let model = convert(&scene, &config).expect("converts");
    let bounds = model.bounds();
    assert_eq!(
        (bounds.width(), bounds.height(), bounds.depth()),
        (8, 8, 8),
        "derived bounds"
    );

    let scale = 8.0_f64;
    let offset_z = (f64::from(bounds.depth()) - 0.9 * scale) / 2.0;
    let slab_box_z = slab_z * scale + offset_z;
    let knob_box_z = knob_z * scale + offset_z;

    let h_max_front = i64::from(CanonicalView::Front.maximum_inward_depth(bounds));
    let h_max_back = i64::from(CanonicalView::Back.maximum_inward_depth(bounds));
    assert_eq!((h_max_front, h_max_back), (32, 32), "h_max derivation");

    // Front depth is measured from the z = 0 face.
    let front_slab_relief = (slab_box_z * 8.0).round() as i64;
    let front_knob_relief = (knob_box_z * 8.0).round() as i64;
    assert!(
        front_slab_relief <= h_max_front,
        "slab must be reachable from Front"
    );
    assert!(
        front_knob_relief > h_max_front,
        "knob must be past the midplane from Front"
    );

    // Back depth is measured from the z = depth face, inward is -z.
    let back_slab_relief = ((f64::from(bounds.depth()) - slab_box_z) * 8.0).round() as i64;
    let back_knob_relief = ((f64::from(bounds.depth()) - knob_box_z) * 8.0).round() as i64;
    assert!(
        back_knob_relief <= h_max_back,
        "knob must be reachable from Back"
    );
    assert!(
        back_slab_relief > h_max_back,
        "slab must be past the midplane from Back"
    );

    let front = model.chart(CanonicalView::Front).expect("front chart");
    let back = model.chart(CanonicalView::Back).expect("back chart");
    let row = 4usize;
    let width = bounds.width() as usize;

    // Front's frame is unmirrored (source_u = [1,0,0], origin_x = 0): pixel
    // column px has world x centered at px + 0.5. Slab occupies box x in
    // [0, 3.2) and knob occupies box x in [4.8, 8); take the floor of each
    // range's midpoint as a texel comfortably inside it.
    let slab_x_min = 0.0_f64;
    let slab_x_max = 0.4 * scale;
    let knob_x_min = 0.6 * scale;
    let knob_x_max = 1.0 * scale;
    let front_slab_col = (((slab_x_min + slab_x_max) / 2.0).floor()) as usize;
    let front_knob_col = (((knob_x_min + knob_x_max) / 2.0).floor()) as usize;

    // Back's frame mirrors u (source_u = [-1,0,0], origin_x = width): pixel
    // column px has world x centered at width - (px + 0.5), i.e. screen_x =
    // width - world_x. The slab/knob ranges swap columns accordingly.
    let back_slab_col =
        ((f64::from(bounds.width()) - (slab_x_min + slab_x_max) / 2.0).floor()) as usize;
    let back_knob_col =
        ((f64::from(bounds.width()) - (knob_x_min + knob_x_max) / 2.0).floor()) as usize;

    let front_slab_texel = front.rgba()[row * width + front_slab_col];
    let front_knob_texel = front.rgba()[row * width + front_knob_col];
    assert_eq!(
        i64::from(255 - front_slab_texel[3]),
        front_slab_relief,
        "Front slab-side relief must match the analytic depth"
    );
    assert_eq!(
        front_knob_texel,
        [0, 0, 0, 0],
        "Front knob-side texel must be empty (past the midplane)"
    );

    let back_slab_texel = back.rgba()[row * width + back_slab_col];
    let back_knob_texel = back.rgba()[row * width + back_knob_col];
    assert_eq!(
        back_slab_texel,
        [0, 0, 0, 0],
        "Back slab-side texel must be empty (past the midplane)"
    );
    assert_eq!(
        i64::from(255 - back_knob_texel[3]),
        back_knob_relief,
        "Back knob-side relief must match the analytic depth, not a clamped value"
    );
}

#[test]
fn exact_midplane_hit_keeps_relief_at_h_max() {
    // A quad at z = 0.5 spans the full x/y extent; a sliver at x = 0 spans
    // z = 0..1, giving the box its full depth. Extents: x [0,1], y [0,1],
    // z [0,1] -> longest = 1, scale = 8, depth = ceil(1*8) = 8,
    // offset_z = (8 - 8) / 2 = 0 (no centering slack: the sliver's extent
    // already matches the dim exactly). The quad's box-space depth from
    // Front is 0.5 * 8 + 0 = 4.0px, quantizing to relief round(8 * 4.0) =
    // 32, which equals h_max = 4 * 8 = 32 exactly: the exact-midplane hit
    // must be kept, not dropped.
    let scene = TriangleScene {
        triangles: vec![
            tri([0.0, 0.0, 0.5], [1.0, 0.0, 0.5], [1.0, 1.0, 0.5]),
            tri([0.0, 0.0, 0.5], [1.0, 1.0, 0.5], [0.0, 1.0, 0.5]),
            tri([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0001, 0.5]),
        ],
        materials: vec![plain_material()],
    };
    // This test pins the quad's raw scene z to an analytic box-space depth,
    // not the default import rotation: identity is set explicitly.
    let mut config = ImportSettings {
        rotation: IDENTITY,
        ..settings(8)
    };
    let mut modes = config.side_modes;
    for side in [
        CanonicalView::Back,
        CanonicalView::Left,
        CanonicalView::Right,
        CanonicalView::Top,
        CanonicalView::Bottom,
    ] {
        modes.set(side, SideMode::Off).expect("legal mode");
    }
    config.side_modes = modes;
    let model = convert(&scene, &config).expect("converts");
    let bounds = model.bounds();
    assert_eq!(
        (bounds.width(), bounds.height(), bounds.depth()),
        (8, 8, 8),
        "derived bounds"
    );
    let h_max = i64::from(CanonicalView::Front.maximum_inward_depth(bounds));
    assert_eq!(h_max, 32, "h_max derivation");
    let chart = model.chart(CanonicalView::Front).expect("front chart");
    let center = chart.rgba()[4 * 8 + 4];
    assert_ne!(center[3], 0, "the exact-midplane hit must not be dropped");
    assert_eq!(
        i64::from(255 - center[3]),
        h_max,
        "the exact-midplane hit keeps relief == h_max"
    );
}

#[test]
fn flat_quad_gives_depth_one_covered_relief_and_empty_edge_on_sides() {
    // One flat quad in the z = 0 plane: extents 1 x 1 x 0.
    let scene = TriangleScene {
        triangles: vec![
            tri([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0]),
            tri([0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]),
        ],
        materials: vec![plain_material()],
    };
    // This test pins the quad's face-normal orientation (Front vs Back) to
    // the raw scene axes, not the default import rotation: identity is set
    // explicitly.
    let config = ImportSettings {
        rotation: IDENTITY,
        ..settings(8)
    };
    let model = convert(&scene, &config).expect("quad converts");
    // The flat z axis floors at depth 1.
    assert_eq!(model.bounds().depth(), 1);

    // Front: the zero-extent axis centers the quad at z = 0.5px inside the
    // depth-1 box, so every covered texel has relief exactly 8 * 0.5 = 4
    // units, which is also h_max (4 * depth) — the quad sits on the
    // midplane, as centering demands.
    let front = model.chart(CanonicalView::Front).expect("front chart");
    let h_max = CanonicalView::Front.maximum_inward_depth(model.bounds());
    for &texel in front.rgba() {
        assert_eq!(
            255 - texel[3],
            h_max,
            "centered flat quad sits on the midplane"
        );
    }

    // Left: the quad is edge-on (zero projected area), so the captured
    // chart is present but entirely empty.
    let left = model.chart(CanonicalView::Left).expect("left chart");
    assert!(
        left.rgba().iter().all(|texel| texel[3] == 0),
        "edge-on capture must produce an all-empty chart, not an error"
    );

    // Back: the quad's geometric winding gives face normal (0,0,-1) (same
    // as its explicit vertex normal, since both triangles share the same
    // winding pattern as the cube fixture's z=0 face). Front observes it as
    // the front face (sigma=+1: -n.forward_Front = 1 >= 0); Back observes
    // the exact same point as the reverse face (sigma=-1: n.forward_Back =
    // 1 > 0). A side is never a same-oriented-face candidate for the
    // other's hit (score(Front) for Back's sigma, and vice versa, both
    // work out to -1 <= 0), so ownership never dedups across the two
    // orientations: both charts must stay fully covered.
    let back = model.chart(CanonicalView::Back).expect("back chart");
    assert!(
        back.rgba().iter().all(|texel| texel[3] != 0),
        "Back observes the reverse face of the same sheet and must keep full coverage, \
         not be deduplicated away by Front's ownership"
    );
}

/// A scene with zero triangles has no geometry to fit a box around; both
/// entry points that compute the fit must reject it loudly rather than
/// silently succeeding with a degenerate 1x1x1 box.
#[test]
fn empty_scene_is_rejected_by_convert_and_derived_bounds() {
    let empty = TriangleScene {
        triangles: vec![],
        materials: vec![],
    };
    assert!(
        matches!(
            derived_bounds(&empty, IDENTITY, 8),
            Err(ImportError::NoTriangles)
        ),
        "derived_bounds on an empty scene must report NoTriangles"
    );
    assert!(
        matches!(
            mesh_import::convert(&empty, &settings(8)),
            Err(ImportError::NoTriangles)
        ),
        "convert on an empty scene must report NoTriangles"
    );
}

#[test]
fn derived_bounds_ceil_and_floor_rules() {
    // Extents 1.0 x 0.5 x 0.26 at longest 8: width 8, height ceil(4)=4,
    // depth ceil(2.08)=3.
    let scene = TriangleScene {
        triangles: vec![tri([0.0, 0.0, 0.0], [1.0, 0.5, 0.26], [1.0, 0.0, 0.0])],
        materials: vec![plain_material()],
    };
    let bounds = derived_bounds(&scene, IDENTITY, 8).expect("bounds derive");
    assert_eq!((bounds.width(), bounds.height(), bounds.depth()), (8, 4, 3));

    // A degenerate flat axis floors at 1.
    let flat = TriangleScene {
        triangles: vec![tri([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0])],
        materials: vec![plain_material()],
    };
    let bounds = derived_bounds(&flat, IDENTITY, 63).expect("bounds derive");
    assert_eq!(bounds.depth(), 1);

    assert!(matches!(
        derived_bounds(&flat, IDENTITY, 64),
        Err(ImportError::LongestAxisRange(64))
    ));
    assert!(matches!(
        derived_bounds(&flat, IDENTITY, 0),
        Err(ImportError::LongestAxisRange(0))
    ));
}

#[test]
fn pair_modes_set_opposite_and_mirror_bits_and_capture_only_the_primary() {
    let mut config = settings(8);
    let mut modes = config.side_modes;
    modes
        .set(CanonicalView::Back, SideMode::FromOppositeMirrored)
        .expect("legal");
    modes
        .set(CanonicalView::Right, SideMode::FromOpposite)
        .expect("legal");
    modes.set(CanonicalView::Top, SideMode::Off).expect("legal");
    modes
        .set(CanonicalView::Bottom, SideMode::Off)
        .expect("legal");
    config.side_modes = modes;
    let model = convert(&cube(), &config).expect("cube converts");

    let views: Vec<_> = model.charts().iter().map(|c| c.view()).collect();
    assert_eq!(views, vec![CanonicalView::Front, CanonicalView::Left]);
    let front = model.chart(CanonicalView::Front).expect("front");
    assert!(front.supplies_opposite());
    assert!(front.mirrors_opposite());
    let left = model.chart(CanonicalView::Left).expect("left");
    assert!(left.supplies_opposite());
    assert!(!left.mirrors_opposite());
}

#[test]
fn side_mode_constraints_are_enforced() {
    let mut modes = mesh_import::SideModes::default();
    // FromOpposite requires the opposite to be Capture.
    modes
        .set(CanonicalView::Front, SideMode::Off)
        .expect("legal");
    assert!(
        modes
            .set(CanonicalView::Back, SideMode::FromOpposite)
            .is_err()
    );

    // Un-capturing a supplier resets its dependent to Off.
    let mut modes = mesh_import::SideModes::default();
    modes
        .set(CanonicalView::Back, SideMode::FromOpposite)
        .expect("legal");
    modes
        .set(CanonicalView::Front, SideMode::Off)
        .expect("legal");
    assert_eq!(modes.get(CanonicalView::Back), SideMode::Off);

    // All-off conversion is rejected.
    let mut config = ImportSettings::default();
    let mut modes = mesh_import::SideModes::default();
    for side in [
        CanonicalView::Front,
        CanonicalView::Back,
        CanonicalView::Left,
        CanonicalView::Right,
        CanonicalView::Top,
        CanonicalView::Bottom,
    ] {
        modes.set(side, SideMode::Off).expect("legal");
    }
    config.side_modes = modes;
    assert!(matches!(
        convert(&cube(), &config),
        Err(ImportError::NoCaptureSides)
    ));
}

#[test]
fn legal_modes_matches_what_set_accepts() {
    // A few representative states: all-Capture default, one side turned
    // Off, and one side already wired to FromOpposite.
    let states = [
        mesh_import::SideModes::default(),
        {
            let mut modes = mesh_import::SideModes::default();
            modes
                .set(CanonicalView::Front, SideMode::Off)
                .expect("legal");
            modes
        },
        {
            let mut modes = mesh_import::SideModes::default();
            modes
                .set(CanonicalView::Back, SideMode::FromOpposite)
                .expect("legal");
            modes
        },
    ];
    let all_modes = [
        SideMode::Capture,
        SideMode::FromOpposite,
        SideMode::FromOppositeMirrored,
        SideMode::Off,
    ];

    for state in states {
        for view in ALL_VIEWS {
            let legal: Vec<SideMode> = state.legal_modes(view).collect();
            for mode in all_modes {
                let mut candidate = state;
                let accepted = candidate.set(view, mode).is_ok();
                assert_eq!(
                    legal.contains(&mode),
                    accepted,
                    "legal_modes({view:?}) disagreed with set({view:?}, {mode:?}) \
                     on state {state:?}"
                );
            }
        }
    }
}

/// UV-sphere inscribed in the unit box: radius 0.5, centered at
/// (0.5, 0.5, 0.5), touching every box face at a single point. 96 rings x
/// 192 segments (both divisible by 4, so the equator touches x/z = 0 and 1
/// exactly, matching the poles' exact touch of y = 0 and 1); fitting at
/// `longest = 63` therefore scales it to fill the box exactly (bounds
/// 63x63x63, no centering offset), matching the box-space sphere
/// (radius 31.5, center (31.5,31.5,31.5)) the derivations below assume.
///
/// Winding is `(p00, p10, p01)` / `(p01, p10, p11)` — the mirror of the
/// naive `(p00, p01, p10)` / `(p01, p11, p10)` diagonal split — chosen so
/// this crate's own `triangle_face_normal` convention (`cross(p2-p0,
/// p1-p0)`) evaluates to the true *outward* sphere normal, not the inward
/// one the naive order gives under that same cross-product convention
/// (verified analytically: at the equator sample theta=pi/2, phi=0, the
/// tangent cross `d/dtheta x d/dphi` points inward, so outward needs the
/// swapped `d/dphi x d/dtheta` order, which is what this winding produces).
fn sphere_scene() -> TriangleScene {
    const RINGS: usize = 96;
    const SEGMENTS: usize = 192;
    const RADIUS: f64 = 0.5;
    const CENTER: [f64; 3] = [0.5, 0.5, 0.5];
    let position = |i: usize, j: usize| -> [f64; 3] {
        let theta = i as f64 * PI / RINGS as f64;
        let phi = j as f64 * 2.0 * PI / SEGMENTS as f64;
        let (sin_t, cos_t) = theta.sin_cos();
        let (sin_p, cos_p) = phi.sin_cos();
        [
            CENTER[0] + RADIUS * sin_t * cos_p,
            CENTER[1] + RADIUS * cos_t,
            CENTER[2] + RADIUS * sin_t * sin_p,
        ]
    };
    let as_f32 = |p: [f64; 3]| [p[0] as f32, p[1] as f32, p[2] as f32];
    let normal_at = |p: [f64; 3]| -> [f32; 3] {
        as_f32([
            (p[0] - CENTER[0]) / RADIUS,
            (p[1] - CENTER[1]) / RADIUS,
            (p[2] - CENTER[2]) / RADIUS,
        ])
    };
    let to_tri = |a: [f64; 3], b: [f64; 3], c: [f64; 3]| -> Triangle {
        Triangle {
            positions: [as_f32(a), as_f32(b), as_f32(c)],
            normals: [normal_at(a), normal_at(b), normal_at(c)],
            uvs: [[0.0, 0.0]; 3],
            colors: [[1.0, 1.0, 1.0, 1.0]; 3],
            material: 0,
        }
    };
    let mut triangles = Vec::with_capacity(RINGS * SEGMENTS * 2);
    for i in 0..RINGS {
        for j in 0..SEGMENTS {
            let p00 = position(i, j);
            let p01 = position(i, j + 1);
            let p10 = position(i + 1, j);
            let p11 = position(i + 1, j + 1);
            // At the poles, two of a quad's four corners coincide, making
            // one of its two triangles zero-area; `rasterize`'s
            // non-finite-area-reciprocal guard skips those safely.
            triangles.push(to_tri(p00, p10, p01));
            triangles.push(to_tri(p01, p10, p11));
        }
    }
    TriangleScene {
        triangles,
        materials: vec![plain_material()],
    }
}

#[test]
fn sphere_ownership_deduplicates_to_the_best_facing_side() {
    // Analytic sphere point at polar angle theta from the Front axis (the
    // axis Front looks down) projects onto Front's screen at radius
    // R*sin(theta), with outward normal n = (sin(theta)cos(phi), n_y,
    // cos(theta)) in Front's (right, down, forward) basis at azimuth phi
    // (phi measured around the Front axis).
    //
    // - contains: Front is the strict-or-tied best owner of a hit iff
    //   cos(theta) >= max(|n_x|, |n_y|) = R*|sin(theta)| * max(|cos(phi)|,
    //   ...)-normalized, i.e. cos(theta) >= sin(theta)*max(|cos(phi)|,
    //   |sin(phi)|); the worst azimuth has max(|cos(phi)|,|sin(phi)|) = 1
    //   (phi a multiple of 90 degrees), giving cos(theta) >= sin(theta),
    //   i.e. theta <= 45 degrees for every azimuth. That admits every point
    //   inside screen radius R*sin(45deg) = R/sqrt(2); ties at exactly 45
    //   degrees resolve to Front by rank. Shrink by 1 texel for
    //   center-vs-region discretization.
    // - contained: ownership only requires cos(theta) >= |n_x| AND
    //   cos(theta) >= |n_y| (Front need not be uniquely best, just not
    //   beaten), so the admitted region extends to the weakest azimuth
    //   (45 degrees, |n_x| = |n_y|): tan(theta) <= sqrt(2), i.e.
    //   sin(theta) <= sqrt(2/3). Add 1 texel for the closure ring plus
    //   0.5 texel for the texel-center-vs-continuous-boundary offset.
    //
    // By symmetry (the sphere and box are both invariant under permuting
    // axes) the identical bounds apply to Top's chart around its own axis.
    let scene = sphere_scene();
    let model = convert(&scene, &settings(63)).expect("sphere converts");
    let bounds = model.bounds();
    assert_eq!(
        (bounds.width(), bounds.height(), bounds.depth()),
        (63, 63, 63),
        "sphere inscribed in the unit box must fit the model box exactly"
    );

    let radius = 31.5_f64;
    let center = 31.5_f64;
    let sin_45 = std::f64::consts::FRAC_1_SQRT_2;
    let sin_weakest_azimuth = (2.0_f64 / 3.0).sqrt();
    let contains_bound = sin_45 * radius - 1.0;
    let contained_bound = sin_weakest_azimuth * radius + 1.5;

    for view in [CanonicalView::Front, CanonicalView::Top] {
        let chart = model.chart(view).expect("chart present");
        let (width, height) = chart.dimensions();
        assert_eq!((width, height), (63, 63), "{view:?} dimensions");
        for y in 0..height {
            for x in 0..width {
                let dx = f64::from(x) + 0.5 - center;
                let dy = f64::from(y) + 0.5 - center;
                let dist = (dx * dx + dy * dy).sqrt();
                let covered = chart.rgba()[(y * width + x) as usize][3] != 0;
                if dist <= contains_bound {
                    assert!(
                        covered,
                        "{view:?} texel ({x},{y}) at distance {dist:.2} from center is \
                         within the contains bound {contains_bound:.2} and must be covered"
                    );
                }
                if covered {
                    assert!(
                        dist <= contained_bound,
                        "{view:?} texel ({x},{y}) at distance {dist:.2} from center exceeds \
                         the contained bound {contained_bound:.2}; ownership must not leave \
                         this far outside the owned cap"
                    );
                }
            }
        }
    }
}

/// Three quads in a unit box (identity rotation): a "slant" tilted 30
/// degrees from horizontal, a horizontal "occluder" strictly above it, and
/// an edge-on sliver pinning the box's y/z extent.
///
/// The slant's geometric normal is proportional to `(0, -cos30, -sin30)`
/// (facing up-and-front): winding `(A,B,D),(B,C,D)` around corners
/// `A = (x0,y0,z0)`, `B = (x1,y0,z0)`, `C = (x1,y0+L*sin30,z0-L*cos30)`,
/// `D = (x0,y0+L*sin30,z0-L*cos30)` gives this, because this crate's own
/// `triangle_face_normal` convention is `cross(p2-p0, p1-p0)`: with
/// `e1 = B-A` along the flat (x) tangent and `e2 = D-A` along the tilt
/// tangent `(0, sin30, -cos30)`, `cross(e2, e1)` is `(0, -cos30, -sin30)`
/// up to a positive scale (verified by direct component expansion).
///
/// The occluder's geometric normal is `(0, -1, 0)` (facing Top): winding
/// `(A,C,B),(A,D,C)` around corners `A = (x0,y,z0)`, `B = (x1,y,z0)`,
/// `C = (x1,y,z1)`, `D = (x0,y,z1)` gives this by the same convention (a
/// horizontal quad is the z0=0 face of `cube()`'s pattern rotated into the
/// x/z plane; the winding is chosen, not copied, to hit this specific
/// normal sign).
fn slant_and_occluder_scene() -> (TriangleScene, f64, f64, f64, f64, f64) {
    let (sin30, cos30) = (30.0_f64).to_radians().sin_cos();

    let (x0, x1) = (0.0_f64, 1.0_f64);
    let (y0, l) = (0.3_f64, 0.4_f64);
    // z0 is chosen so the slant's whole z range (box-space, scale 16)
    // stays under h_max_front = 4*16 = 64, i.e. z <= 8.0 scene-scaled:
    // z0 = 0.45 keeps the slant's top (z0 itself, box z = 7.2) comfortably
    // under that reachability ceiling, so Front's own reachability filter
    // (Task 12, unrelated to ownership) never drops a slant texel and this
    // test isolates the ownership visibility gate alone.
    let z0 = 0.45_f64;
    let y_top = y0 + l * sin30;
    let z_min = z0 - l * cos30;

    let slant_a = [x0, y0, z0];
    let slant_b = [x1, y0, z0];
    let slant_c = [x1, y_top, z_min];
    let slant_d = [x0, y_top, z_min];

    // Strictly below (smaller y than) every slant point: y0 is the
    // slant's minimum y.
    let y_occ = y0 - 0.2;
    // Covers the slant's z range [z_min, z0] with margin.
    let z_occ_min = z_min - 0.05;
    let z_occ_max = z0 + 0.05;
    let occ_a = [x0, y_occ, z_occ_min];
    let occ_b = [x1, y_occ, z_occ_min];
    let occ_c = [x1, y_occ, z_occ_max];
    let occ_d = [x0, y_occ, z_occ_max];

    let as_f32 = |p: [f64; 3]| [p[0] as f32, p[1] as f32, p[2] as f32];
    let scene = TriangleScene {
        triangles: vec![
            tri(as_f32(slant_a), as_f32(slant_b), as_f32(slant_d)),
            tri(as_f32(slant_b), as_f32(slant_c), as_f32(slant_d)),
            tri(as_f32(occ_a), as_f32(occ_c), as_f32(occ_b)),
            tri(as_f32(occ_a), as_f32(occ_d), as_f32(occ_c)),
            // Edge-on sliver at x = 0 pinning the box's full y and z
            // extent. Constant x makes every triple of its vertices
            // collinear under both Front's (x,y) and Top's (x,z)
            // projections (screen coordinate 0 is shared by all three),
            // so it contributes zero rasterized area to either enabled
            // side while still setting the bounding box's y/z extent to
            // [0,1] via its raw vertex positions.
            tri([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        ],
        materials: vec![plain_material()],
    };
    (scene, y0, y_top, z0, z_min, y_occ)
}

#[test]
fn fallback_owner_used_when_the_best_side_is_occluded() {
    // The occluder covers the slant's full x/z footprint at a strictly
    // smaller y, so Top's raw nearest-hit rasterization (ordinary
    // occlusion, established before ownership runs) never sees the slant
    // at all there. Ownership's own-score/candidate scoring then favors
    // Top for the slant's hit (score cos(30deg) ~= 0.866 vs Front's own
    // score sin(30deg) = 0.5), but Top fails the visibility gate: its
    // filtered depth buffer at the slant's projected texel holds the
    // occluder's much shallower depth, not the slant's, so the "d_T(p) <=
    // z + tol" check rejects it — Front, the only remaining candidate,
    // becomes the fallback owner. This isolates the visibility condition
    // specifically: separately verified in comments below that Top's
    // reach and in-bounds checks both pass for the slant's reconstructed
    // point, so visibility is the only gate excluding it.
    let (sin30, cos30) = (30.0_f64).to_radians().sin_cos();
    let (scene, y0, y_top, z0, z_min, y_occ) = slant_and_occluder_scene();

    // The slant/occluder scene (see its doc comment) is built assuming
    // identity rotation; set it explicitly rather than relying on the
    // default import rotation.
    let mut config = ImportSettings {
        rotation: IDENTITY,
        ..settings(16)
    };
    let mut modes = config.side_modes;
    for side in [
        CanonicalView::Back,
        CanonicalView::Left,
        CanonicalView::Right,
        CanonicalView::Bottom,
    ] {
        modes.set(side, SideMode::Off).expect("legal mode");
    }
    config.side_modes = modes;
    let model = convert(&scene, &config).expect("converts");
    let bounds = model.bounds();
    // Unit-extent scene at longest_axis_pixels = 16: the slant/occluder
    // touch x = 0 and x = 1, and the sliver touches y = 0/1 and z = 0/1,
    // so every axis's extent is exactly 1 with no centering offset: scale
    // is exactly 16.
    assert_eq!(
        (bounds.width(), bounds.height(), bounds.depth()),
        (16, 16, 16),
        "derived bounds"
    );
    let scale = f64::from(bounds.width());

    let top = model.chart(CanonicalView::Top).expect("top chart");
    let front = model.chart(CanonicalView::Front).expect("front chart");

    // Top: within the occluder's covered rows, every texel must show the
    // occluder's own relief (Top trivially owns it: normal (0,-1,0) gives
    // Top's own score 1, and the only other enabled side, Front, scores
    // exactly 0 for that normal — dot((0,-1,0),(0,0,1)) = 0 — so Front is
    // not even a same-oriented-face candidate). Matching this exact
    // relief (not just "covered") also proves the slant never leaks
    // through: the slant's relief range is far higher (see below).
    let occ_relief = ((y_occ * scale) * 8.0).round() as i64;
    let z_occ_min_box = (z_min - 0.05) * scale;
    let z_occ_max_box = (z0 + 0.05) * scale;
    let mut occluder_rows_checked = 0;
    for pz in 0..bounds.depth() {
        let center = f64::from(pz) + 0.5;
        if center < z_occ_min_box || center >= z_occ_max_box {
            continue;
        }
        occluder_rows_checked += 1;
        for px in 0..bounds.width() {
            let idx = (pz * bounds.width() + px) as usize;
            let texel = top.rgba()[idx];
            assert_ne!(
                texel[3], 0,
                "Top ({px},{pz}) must be covered by the occluder"
            );
            assert_eq!(
                i64::from(255 - texel[3]),
                occ_relief,
                "Top ({px},{pz}) must show the occluder's relief {occ_relief}, not the slant's \
                 (occlusion + ownership must hide the slant from Top entirely)"
            );
        }
    }
    assert!(
        occluder_rows_checked > 0,
        "test setup: no occluder rows probed"
    );

    // Front: every row whose pixel-center y falls within the slant's
    // range must be covered at the slant's analytic per-row relief
    // (fallback ownership, since Top is occluded there).
    let mut slant_rows_checked = 0;
    for py in 0..bounds.height() {
        let world_y_scene = (f64::from(py) + 0.5) / scale;
        if world_y_scene < y0 || world_y_scene > y_top {
            continue;
        }
        slant_rows_checked += 1;
        let t = (world_y_scene - y0) / sin30;
        let z_scene = z0 - t * cos30;
        let expected_relief = ((z_scene * scale) * 8.0).round() as i64;
        for px in 0..bounds.width() {
            let idx = (py * bounds.width() + px) as usize;
            let texel = front.rgba()[idx];
            assert_ne!(
                texel[3], 0,
                "Front ({px},{py}) must be covered by the slant (fallback owner)"
            );
            assert_eq!(
                i64::from(255 - texel[3]),
                expected_relief,
                "Front ({px},{py}) relief must match the slant's analytic depth"
            );
        }
    }
    assert!(slant_rows_checked > 0, "test setup: no slant rows probed");
}

/// "Floor" quad at z = 0.45 spans the full x/y footprint; "plate" quad at
/// z = 0.1 covers the central region x,y in [0.25,0.75]. Both face Front
/// (geometric normal (0,0,-1) — the `cube()` z=0 face's winding pattern,
/// confirmed Front-facing by `flat_quad_gives_depth_one_covered_relief_...`
/// above), so Front owns both with no competing candidate. A sliver at
/// x = 0 spans z = 0..1 purely to widen the scene's bounding box so both
/// surfaces land inside Front's reachable front half (h_max); it is
/// edge-on to Front (near-zero y extent) and contributes no rasterized
/// coverage (same technique as `geometry_past_the_midplane_is_dropped`).
///
/// Plate and floor are 4-adjacent across the plate's silhouette with real
/// empty space between them, so the pair is cut by the continuity
/// verdict; with only Front enabled no side can rescue the ring, which
/// stays empty.
#[test]
fn occlusion_cut_drops_the_far_strip() {
    let scene = TriangleScene {
        triangles: vec![
            // Floor: x,y in [0,1], z = 0.45.
            tri([0.0, 0.0, 0.45], [1.0, 0.0, 0.45], [1.0, 1.0, 0.45]),
            tri([0.0, 0.0, 0.45], [1.0, 1.0, 0.45], [0.0, 1.0, 0.45]),
            // Plate: x,y in [0.25,0.75], z = 0.1.
            tri([0.25, 0.25, 0.1], [0.75, 0.25, 0.1], [0.75, 0.75, 0.1]),
            tri([0.25, 0.25, 0.1], [0.75, 0.75, 0.1], [0.25, 0.75, 0.1]),
            // Edge-on sliver widening the z extent to [0,1].
            tri([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0001, 0.5]),
        ],
        materials: vec![plain_material()],
    };
    let mut config = ImportSettings {
        rotation: IDENTITY,
        ..settings(16)
    };
    let mut modes = config.side_modes;
    for side in [
        CanonicalView::Back,
        CanonicalView::Left,
        CanonicalView::Right,
        CanonicalView::Top,
        CanonicalView::Bottom,
    ] {
        modes.set(side, SideMode::Off).expect("legal mode");
    }
    config.side_modes = modes;

    let model = convert(&scene, &config).expect("converts");
    let bounds = model.bounds();
    // Every extent (x and y from floor, z from the sliver) is exactly 1.0,
    // so scale is exactly longest_axis_pixels with no centering offset.
    assert_eq!(
        (bounds.width(), bounds.height(), bounds.depth()),
        (16, 16, 16),
        "derived bounds"
    );
    let scale = f64::from(bounds.width());

    let h_max = i64::from(CanonicalView::Front.maximum_inward_depth(bounds));
    let plate_relief = ((0.1_f64 * scale) * 8.0).round() as i64;
    let floor_relief = ((0.45_f64 * scale) * 8.0).round() as i64;
    assert!(
        plate_relief <= h_max && floor_relief <= h_max,
        "both surfaces must be reachable from Front (plate {plate_relief}, floor \
         {floor_relief}, h_max {h_max})"
    );

    // Plate occupies texel columns/rows whose center (idx + 0.5) falls
    // inside [0.25*scale, 0.75*scale) = [4, 12).
    let plate_lo = (0.25 * scale) as u32;
    let plate_hi = (0.75 * scale) as u32 - 1;

    let front = model.chart(CanonicalView::Front).expect("front chart");
    let (width, height) = front.dimensions();
    assert_eq!((width, height), (16, 16));

    for y in 0..height {
        for x in 0..width {
            let texel = front.rgba()[(y * width + x) as usize];
            let in_plate_x = (plate_lo..=plate_hi).contains(&x);
            let in_plate_y = (plate_lo..=plate_hi).contains(&y);
            let in_plate = in_plate_x && in_plate_y;
            let adjacent_left = x + 1 == plate_lo;
            let adjacent_right = x == plate_hi + 1;
            let adjacent_top = y + 1 == plate_lo;
            let adjacent_bottom = y == plate_hi + 1;
            let ring = !in_plate
                && ((in_plate_x && (adjacent_top || adjacent_bottom))
                    || (in_plate_y && (adjacent_left || adjacent_right)));
            if in_plate {
                assert_eq!(
                    i64::from(255 - texel[3]),
                    plate_relief,
                    "({x},{y}) is inside the plate's silhouette and must keep it intact"
                );
            } else if ring {
                assert_eq!(
                    texel,
                    [0, 0, 0, 0],
                    "({x},{y}) is 4-adjacent to the plate across the fabricated cliff and \
                     must be cut"
                );
            } else {
                assert_eq!(
                    i64::from(255 - texel[3]),
                    floor_relief,
                    "({x},{y}) is floor, two or more texels from the plate, and must stay \
                     covered"
                );
            }
        }
    }
}

/// Same framing as `occlusion_cut_drops_the_far_strip`, but the two relief
/// levels are physically connected: an upper shelf at z = 0.1 for x in
/// [0,0.5], a lower shelf at z = 0.45 for x in [0.5,1.0], and a vertical
/// quad joining them at x = 0.5 spanning z in [0.1,0.45]. The connecting
/// wall's cross-section joins the two shelves, so the shelf-to-shelf pair
/// straddling x = 0.5 is continuous by the continuity verdict, and both
/// shelves stay covered.
#[test]
fn real_step_wall_is_kept() {
    let scene = TriangleScene {
        triangles: vec![
            // Upper shelf: x in [0,0.5], z = 0.1.
            tri([0.0, 0.0, 0.1], [0.5, 0.0, 0.1], [0.5, 1.0, 0.1]),
            tri([0.0, 0.0, 0.1], [0.5, 1.0, 0.1], [0.0, 1.0, 0.1]),
            // Lower shelf: x in [0.5,1.0], z = 0.45.
            tri([0.5, 0.0, 0.45], [1.0, 0.0, 0.45], [1.0, 1.0, 0.45]),
            tri([0.5, 0.0, 0.45], [1.0, 1.0, 0.45], [0.5, 1.0, 0.45]),
            // Connecting wall at x = 0.5, spanning z in [0.1,0.45]: edge-on
            // (zero area) to Front, so it contributes no coverage there,
            // only cross-section surface for the continuity verdict to
            // join the two shelves through.
            tri([0.5, 0.0, 0.1], [0.5, 1.0, 0.1], [0.5, 1.0, 0.45]),
            tri([0.5, 0.0, 0.1], [0.5, 1.0, 0.45], [0.5, 0.0, 0.45]),
            // Edge-on sliver widening the z extent to [0,1].
            tri([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0001, 0.5]),
        ],
        materials: vec![plain_material()],
    };
    let mut config = ImportSettings {
        rotation: IDENTITY,
        ..settings(16)
    };
    let mut modes = config.side_modes;
    for side in [
        CanonicalView::Back,
        CanonicalView::Left,
        CanonicalView::Right,
        CanonicalView::Top,
        CanonicalView::Bottom,
    ] {
        modes.set(side, SideMode::Off).expect("legal mode");
    }
    config.side_modes = modes;

    let model = convert(&scene, &config).expect("converts");
    let bounds = model.bounds();
    assert_eq!(
        (bounds.width(), bounds.height(), bounds.depth()),
        (16, 16, 16),
        "derived bounds"
    );
    let scale = f64::from(bounds.width());
    let upper_relief = ((0.1_f64 * scale) * 8.0).round() as i64;
    let lower_relief = ((0.45_f64 * scale) * 8.0).round() as i64;

    let boundary = (0.5 * scale) as u32; // upper covers [0,boundary), lower [boundary,16)

    let front = model.chart(CanonicalView::Front).expect("front chart");
    let (width, height) = front.dimensions();
    assert_eq!((width, height), (16, 16));

    for y in 0..height {
        for x in 0..width {
            let texel = front.rgba()[(y * width + x) as usize];
            let expected = if x < boundary {
                upper_relief
            } else {
                lower_relief
            };
            assert_eq!(
                i64::from(255 - texel[3]),
                expected,
                "({x},{y}) must stay covered at its shelf's own relief; the real wall at \
                 x = {boundary} must not be cut, unlike the fabricated-cliff test"
            );
        }
    }
}

/// Mesh-space tab-over-slanted-floor (the bunny's ear-over-back in
/// miniature), captured from Top and Back only. The floor slants so its
/// upward normal has a front-facing component: Back genuinely observes
/// it. Top must keep the tab intact and relinquish the floor strip
/// 4-adjacent to the tab's silhouette; the strip BEHIND the tab is within
/// Back's reach, so Back's chart must render it — the seam bug is exactly
/// this strip being dropped by everyone. The strip IN FRONT of the tab is
/// beyond Back's reach and stays an honest hole.
#[test]
fn relinquished_silhouette_strip_is_rescued_by_back() {
    let quad =
        |p0: [f32; 3], p1: [f32; 3], p2: [f32; 3], p3: [f32; 3]| [tri(p0, p1, p2), tri(p0, p2, p3)];
    let mut triangles = Vec::new();
    // Floor: y = 0.125 + 0.25 z over x,z in [0,1] (box y = 1 + 0.25 Z).
    triangles.extend(quad(
        [0.0, 0.125, 0.0],
        [1.0, 0.125, 0.0],
        [1.0, 0.375, 1.0],
        [0.0, 0.375, 1.0],
    ));
    // Tab: y = 0.0625 over x,z in [0.25, 0.75] (box y = 0.5, x,z in [2,6]).
    triangles.extend(quad(
        [0.25, 0.0625, 0.25],
        [0.75, 0.0625, 0.25],
        [0.75, 0.0625, 0.75],
        [0.25, 0.0625, 0.75],
    ));
    // Edge-on slivers pinning the AABB to the unit cube (zero coverage).
    triangles.push(tri([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0001, 0.5, 0.0]));
    triangles.push(tri([1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.9999, 0.5, 1.0]));
    let scene = TriangleScene {
        triangles,
        materials: vec![plain_material()],
    };
    let mut config = ImportSettings {
        rotation: IDENTITY,
        ..settings(8)
    };
    let mut modes = config.side_modes;
    for side in [
        CanonicalView::Front,
        CanonicalView::Left,
        CanonicalView::Right,
        CanonicalView::Bottom,
    ] {
        modes.set(side, SideMode::Off).expect("legal mode");
    }
    config.side_modes = modes;
    let model = convert(&scene, &config).expect("converts");
    assert_eq!(
        (
            model.bounds().width(),
            model.bounds().height(),
            model.bounds().depth()
        ),
        (8, 8, 8)
    );

    let top = model.chart(CanonicalView::Top).expect("top chart");
    let top_rgba = |x: u32, z: u32| top.rgba()[(z * 8 + x) as usize];
    // Tab intact at relief 4 (y = 0.5 box units).
    for z in 2..=5u32 {
        for x in 2..=5u32 {
            assert_eq!(
                i64::from(255 - top_rgba(x, z)[3]),
                4,
                "tab texel ({x},{z}) keeps its silhouette and relief"
            );
        }
    }
    // Relinquished strips: empty in Top.
    for x in 2..=5u32 {
        assert_eq!(top_rgba(x, 6), [0, 0, 0, 0], "far strip ({x},6) yields");
        assert_eq!(top_rgba(x, 1), [0, 0, 0, 0], "near strip ({x},1) yields");
    }

    // The rescue: Back's chart renders the far strip. Back texel (u, v)
    // maps to box x = 8 - (u + 0.5); the strip columns x in 2..=5 land at
    // u in 2..=5 reversed. Back's row v = 2 (y center 2.5) samples the
    // floor at box z = 4 (y - 1) = 6, depth 8 - 6 = 2, relief 16. Kept
    // texels u in {2..5}; closure support extends one texel to u in
    // {1, 6}; u in {0, 7} stays empty (Top keeps those floor points).
    let back = model.chart(CanonicalView::Back).expect("back chart");
    let back_rgba = |u: u32, v: u32| back.rgba()[(v * 8 + u) as usize];
    for u in 1..=6u32 {
        assert_eq!(
            i64::from(255 - back_rgba(u, 2)[3]),
            16,
            "Back ({u},2) must render the rescued strip"
        );
    }
    for u in [0u32, 7] {
        assert_eq!(back_rgba(u, 2), [0, 0, 0, 0], "Back ({u},2) defers to Top");
    }
    // Everything else in Back is unreachable or missed: rows other than
    // v = 2 are empty.
    for v in (0..8u32).filter(|&v| v != 2) {
        for u in 0..8u32 {
            assert_eq!(back_rgba(u, v), [0, 0, 0, 0], "Back ({u},{v}) empty");
        }
    }
}
