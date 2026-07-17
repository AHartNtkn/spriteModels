use mesh_import::{
    Lighting, Material, Texture, Triangle, TriangleScene, View, light_direction, rasterize,
};

fn quad(z: f32, material: usize) -> [Triangle; 2] {
    let v = |x: f32, y: f32| [x, y, z];
    let tri = |a: [f32; 3], b: [f32; 3], c: [f32; 3], uvs: [[f32; 2]; 3]| Triangle {
        positions: [a, b, c],
        normals: [[0.0, 0.0, -1.0]; 3],
        uvs,
        colors: [[1.0, 1.0, 1.0, 1.0]; 3],
        material,
    };
    [
        tri(
            v(0.0, 0.0),
            v(4.0, 0.0),
            v(4.0, 4.0),
            [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]],
        ),
        tri(
            v(0.0, 0.0),
            v(4.0, 4.0),
            v(0.0, 4.0),
            [[0.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        ),
    ]
}

fn plain_material() -> Material {
    Material {
        base_color_factor: [1.0, 1.0, 1.0, 1.0],
        base_color_texture: None,
        alpha_cutoff: None,
    }
}

fn front_view() -> View {
    View {
        origin: [0.0, 0.0, 0.0],
        right: [1.0, 0.0, 0.0],
        down: [0.0, 1.0, 0.0],
        forward: [0.0, 0.0, 1.0],
        scale: 1.0,
        width: 4,
        height: 4,
    }
}

fn unlit() -> Lighting {
    // Ambient 1.0 makes shade exactly 1: color assertions are then pure
    // base-color assertions.
    Lighting {
        direction: [0.0, 0.0, -1.0],
        ambient: 1.0,
    }
}

#[test]
fn axis_aligned_quad_covers_projected_texels_at_its_depth() {
    let scene = TriangleScene {
        triangles: quad(1.5, 0).to_vec(),
        materials: vec![plain_material()],
    };
    let raster = rasterize(&scene, &front_view(), &unlit());
    for y in 0..4 {
        for x in 0..4 {
            let i = (y * 4 + x) as usize;
            assert_eq!(raster.depth[i], 1.5, "texel ({x},{y}) depth");
            assert_eq!(
                raster.color[i],
                [255, 255, 255, 255],
                "texel ({x},{y}) color"
            );
        }
    }
}

#[test]
fn nearest_of_two_overlapping_quads_wins_everywhere() {
    let mut triangles = quad(3.0, 0).to_vec();
    for mut tri in quad(1.0, 1) {
        // Recolor the near quad through its vertex colors so ownership is
        // observable: near quad is red.
        tri.colors = [[1.0, 0.0, 0.0, 1.0]; 3];
        triangles.push(tri);
    }
    let scene = TriangleScene {
        triangles,
        materials: vec![plain_material(), plain_material()],
    };
    let raster = rasterize(&scene, &front_view(), &unlit());
    for i in 0..16 {
        assert_eq!(raster.depth[i], 1.0, "texel {i} must hold the nearer depth");
        assert_eq!(
            raster.color[i],
            [255, 0, 0, 255],
            "texel {i} must hold the nearer color"
        );
    }
}

#[test]
fn lambert_shading_matches_the_formula() {
    let scene = TriangleScene {
        triangles: quad(1.0, 0).to_vec(),
        materials: vec![plain_material()],
    };
    // Light straight at the quad's normal (0,0,-1): n.l = 1.
    let full = rasterize(
        &scene,
        &front_view(),
        &Lighting {
            direction: [0.0, 0.0, -1.0],
            ambient: 0.25,
        },
    );
    assert_eq!(full.color[0], [255, 255, 255, 255]);

    // Light from behind: max(0, n.l) = 0, so shade = ambient = 0.25.
    let back = rasterize(
        &scene,
        &front_view(),
        &Lighting {
            direction: [0.0, 0.0, 1.0],
            ambient: 0.25,
        },
    );
    let expected = (255.0f32 * 0.25).round() as u8;
    assert_eq!(back.color[0], [expected, expected, expected, 255]);
}

#[test]
fn away_facing_normals_are_flipped_for_two_sided_shading() {
    let mut triangles = quad(1.0, 0).to_vec();
    for tri in &mut triangles {
        // Normal points away from the viewer (along +forward).
        tri.normals = [[0.0, 0.0, 1.0]; 3];
    }
    let scene = TriangleScene {
        triangles,
        materials: vec![plain_material()],
    };
    let raster = rasterize(
        &scene,
        &front_view(),
        &Lighting {
            direction: [0.0, 0.0, -1.0],
            ambient: 0.0,
        },
    );
    // Flipped back toward the viewer, n.l = 1: full brightness, not black.
    assert_eq!(raster.color[0], [255, 255, 255, 255]);
}

#[test]
fn mask_cutoff_discards_transparent_texels() {
    // 2x1 texture: left texel opaque green, right texel alpha 0.
    let texture = Texture {
        width: 2,
        height: 1,
        rgba: vec![[0, 255, 0, 255], [0, 255, 0, 0]],
    };
    let material = Material {
        base_color_factor: [1.0, 1.0, 1.0, 1.0],
        base_color_texture: Some(texture),
        alpha_cutoff: Some(0.5),
    };
    let scene = TriangleScene {
        triangles: quad(1.0, 0).to_vec(),
        materials: vec![material],
    };
    let raster = rasterize(&scene, &front_view(), &unlit());
    // u < 0.5 samples the opaque texel, u > 0.5 the transparent one.
    // Texel centers x=0,1 have u=0.125,0.375 (opaque); x=2,3 have
    // u=0.625,0.875 (discarded).
    assert_ne!(raster.depth[0], f32::INFINITY);
    assert_ne!(raster.depth[1], f32::INFINITY);
    assert_eq!(raster.depth[2], f32::INFINITY);
    assert_eq!(raster.depth[3], f32::INFINITY);
    assert_eq!(raster.color[2][3], 0, "discarded texel stays uncovered");
}

#[test]
fn clockwise_wound_triangles_are_rasterized_not_culled() {
    // Reverse the vertex order of a standard quad to make it clockwise-wound
    // (negative screen-space area); verify it still rasterizes correctly.
    let v = |x: f32, y: f32| [x, y, 1.5];
    let tri = |a: [f32; 3], b: [f32; 3], c: [f32; 3], uvs: [[f32; 2]; 3]| Triangle {
        positions: [a, b, c],
        normals: [[0.0, 0.0, -1.0]; 3],
        uvs,
        colors: [[1.0, 1.0, 1.0, 1.0]; 3],
        material: 0,
    };
    let triangles = vec![
        // First triangle: reversed from (0,0)-(4,0)-(4,4) to (0,0)-(4,4)-(4,0)
        tri(
            v(0.0, 0.0),
            v(4.0, 4.0),
            v(4.0, 0.0),
            [[0.0, 0.0], [1.0, 1.0], [1.0, 0.0]],
        ),
        // Second triangle: reversed from (0,0)-(4,4)-(0,4) to (0,0)-(0,4)-(4,4)
        tri(
            v(0.0, 0.0),
            v(0.0, 4.0),
            v(4.0, 4.0),
            [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
        ),
    ];
    let scene = TriangleScene {
        triangles,
        materials: vec![plain_material()],
    };
    let raster = rasterize(&scene, &front_view(), &unlit());
    for y in 0..4 {
        for x in 0..4 {
            let i = (y * 4 + x) as usize;
            assert_eq!(raster.depth[i], 1.5, "clockwise texel ({x},{y}) depth");
            assert_eq!(
                raster.color[i],
                [255, 255, 255, 255],
                "clockwise texel ({x},{y}) color"
            );
        }
    }
}

#[test]
fn light_direction_places_the_light_by_azimuth_and_elevation() {
    let front = light_direction(0.0, 0.0);
    assert!((front[0]).abs() < 1e-6 && (front[1]).abs() < 1e-6 && (front[2] + 1.0).abs() < 1e-6);
    let overhead = light_direction(0.0, 90.0);
    // -y is up in box space.
    assert!((overhead[1] + 1.0).abs() < 1e-6);
}
