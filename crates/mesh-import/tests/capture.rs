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
fn geometry_past_the_midplane_clamps_to_h_max() {
    // A quad at z = 0.9 of a unit-depth box: from the Front (depth 8px at
    // longest 8), its depth is 7.2px = 57.6 relief units, past h_max = 32.
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
    let h_max = CanonicalView::Front.maximum_inward_depth(model.bounds());
    let center = chart.rgba()[4 * 8 + 4];
    assert_eq!(center[3], 255 - h_max, "relief must clamp to h_max");
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
