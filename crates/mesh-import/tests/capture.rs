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

#[test]
fn cube_spanning_the_box_captures_relief_zero_on_all_six_faces() {
    let model = convert(&cube(), &settings(8)).expect("cube converts");
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
    let mut config = settings(8);
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
    let mut config = settings(8);
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
    let mut config = settings(8);
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
    let model = convert(&scene, &settings(8)).expect("quad converts");
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
