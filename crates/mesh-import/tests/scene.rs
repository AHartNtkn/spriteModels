mod glb;

use std::io::Write;

use mesh_import::{ImportError, load_scene};

/// One triangle with explicit normals and UVs, translated by a node
/// transform of (10, 0, 0). Positions: (0,0,0) (1,0,0) (0,1,0);
/// normals all (0,0,1); uvs (0,0) (1,0) (0,1).
fn single_triangle_glb() -> Vec<u8> {
    let mut bin = Vec::new();
    let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let normals: [[f32; 3]; 3] = [[0.0, 0.0, 1.0]; 3];
    let uvs: [[f32; 2]; 3] = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
    for p in positions {
        for c in p {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    for n in normals {
        for c in n {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    for uv in uvs {
        for c in uv {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let json = serde_json::json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "translation": [10.0, 0.0, 0.0]}],
        "meshes": [{"primitives": [{
            "attributes": {"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2},
            "material": 0
        }]}],
        "materials": [{"pbrMetallicRoughness": {"baseColorFactor": [0.5, 1.0, 1.0, 1.0]}}],
        "buffers": [{"byteLength": bin.len()}],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": 36},
            {"buffer": 0, "byteOffset": 36, "byteLength": 36},
            {"buffer": 0, "byteOffset": 72, "byteLength": 24}
        ],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
             "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]},
            {"bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3"},
            {"bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC2"}
        ]
    });
    glb::write_glb(&json.to_string(), &bin)
}

fn write_temp_glb(bytes: &[u8]) -> tempfile::NamedTempFile {
    let mut file = tempfile::Builder::new()
        .suffix(".glb")
        .tempfile()
        .expect("temp file");
    file.write_all(bytes).expect("write glb");
    file
}

#[test]
fn node_transform_applies_to_positions_and_material_factor_is_loaded() {
    let file = write_temp_glb(&single_triangle_glb());
    let scene = load_scene(file.path()).expect("scene loads");

    assert_eq!(scene.triangles.len(), 1);
    let tri = &scene.triangles[0];
    assert_eq!(tri.positions[0], [10.0, 0.0, 0.0]);
    assert_eq!(tri.positions[1], [11.0, 0.0, 0.0]);
    assert_eq!(tri.positions[2], [10.0, 1.0, 0.0]);
    assert_eq!(tri.normals[0], [0.0, 0.0, 1.0]);
    assert_eq!(tri.uvs[1], [1.0, 0.0]);
    // COLOR_0 is absent, so vertex colors default to opaque white.
    assert_eq!(tri.colors[0], [1.0, 1.0, 1.0, 1.0]);
    let material = &scene.materials[tri.material];
    assert_eq!(material.base_color_factor, [0.5, 1.0, 1.0, 1.0]);
    assert!(material.base_color_texture.is_none());
    assert!(material.alpha_cutoff.is_none());
}

/// Same geometry with NORMAL omitted: loading must synthesize the face
/// normal (0, 0, 1) — unit cross product of the edge vectors.
#[test]
fn missing_normals_get_face_normals() {
    let mut bin = Vec::new();
    let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    for p in positions {
        for c in p {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let json = serde_json::json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0}}]}],
        "buffers": [{"byteLength": bin.len()}],
        "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 36}],
        "accessors": [{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                       "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]}]
    });
    let file = write_temp_glb(&glb::write_glb(&json.to_string(), &bin));
    let scene = load_scene(file.path()).expect("scene loads");
    assert_eq!(scene.triangles.len(), 1);
    for vertex in 0..3 {
        assert_eq!(scene.triangles[0].normals[vertex], [0.0, 0.0, 1.0]);
    }
}

/// A points-only primitive has no triangles; loading must reject the
/// scene rather than produce an empty conversion input.
#[test]
fn scene_without_triangles_is_rejected() {
    let mut bin = Vec::new();
    for c in [0.0f32, 0.0, 0.0] {
        bin.extend_from_slice(&c.to_le_bytes());
    }
    let json = serde_json::json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "mode": 0}]}],
        "buffers": [{"byteLength": bin.len()}],
        "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 12}],
        "accessors": [{"bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3",
                       "min": [0.0, 0.0, 0.0], "max": [0.0, 0.0, 0.0]}]
    });
    let file = write_temp_glb(&glb::write_glb(&json.to_string(), &bin));
    let error = load_scene(file.path()).expect_err("points-only scene must be rejected");
    assert!(matches!(error, ImportError::NoTriangles));
}

#[test]
fn malformed_file_is_rejected_with_gltf_error() {
    let file = write_temp_glb(b"not a gltf file");
    let error = load_scene(file.path()).expect_err("garbage must be rejected");
    assert!(matches!(error, ImportError::Gltf(_)));
}

fn assert_close(actual: [f32; 3], expected: [f32; 3], epsilon: f32) {
    for i in 0..3 {
        assert!(
            (actual[i] - expected[i]).abs() <= epsilon,
            "component {i}: actual {} expected {} (epsilon {epsilon})",
            actual[i],
            expected[i]
        );
    }
}

/// A determinant near zero from a small *uniform* scale (0.001^3 = 1e-9) is
/// not singular: the inverse-transpose exists and, for a uniform scale,
/// leaves normal direction unchanged after renormalization. An absolute
/// determinant threshold misclassifies this as singular and substitutes the
/// face normal instead of the correctly transformed source normal.
#[test]
fn small_uniform_scale_preserves_source_normal_direction() {
    let mut bin = Vec::new();
    // Face normal of this triangle is (0,0,1) - deliberately different from
    // the source normal, so a wrong fallback to the face normal is caught.
    let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let normals: [[f32; 3]; 3] = [[0.6, 0.8, 0.0]; 3];
    for p in positions {
        for c in p {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    for n in normals {
        for c in n {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let json = serde_json::json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "scale": [0.001, 0.001, 0.001]}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1}}]}],
        "buffers": [{"byteLength": bin.len()}],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": 36},
            {"buffer": 0, "byteOffset": 36, "byteLength": 36}
        ],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
             "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]},
            {"bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3"}
        ]
    });
    let file = write_temp_glb(&glb::write_glb(&json.to_string(), &bin));
    let scene = load_scene(file.path()).expect("scene loads");
    assert_eq!(scene.triangles.len(), 1);
    for vertex in 0..3 {
        assert_close(scene.triangles[0].normals[vertex], [0.6, 0.8, 0.0], 1e-5);
    }
}

/// Non-uniform scale is the case the inverse-transpose exists specifically
/// to handle correctly (a plain normal-matrix multiply would skew the
/// normal off the surface). scale (2,1,1) applied to source normal
/// (1/sqrt(2), 1/sqrt(2), 0) via the inverse-transpose diag(0.5,1,1) gives
/// (0.35355335, 0.7071068, 0), which renormalizes to
/// (0.4472136, 0.8944272, 0).
#[test]
fn non_uniform_scale_transforms_normal_by_inverse_transpose() {
    let mut bin = Vec::new();
    let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let frac_1_sqrt_2 = std::f32::consts::FRAC_1_SQRT_2;
    let normals: [[f32; 3]; 3] = [[frac_1_sqrt_2, frac_1_sqrt_2, 0.0]; 3];
    for p in positions {
        for c in p {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    for n in normals {
        for c in n {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let json = serde_json::json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "scale": [2.0, 1.0, 1.0]}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1}}]}],
        "buffers": [{"byteLength": bin.len()}],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": 36},
            {"buffer": 0, "byteOffset": 36, "byteLength": 36}
        ],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
             "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]},
            {"bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3"}
        ]
    });
    let file = write_temp_glb(&glb::write_glb(&json.to_string(), &bin));
    let scene = load_scene(file.path()).expect("scene loads");
    assert_eq!(scene.triangles.len(), 1);
    for vertex in 0..3 {
        assert_close(
            scene.triangles[0].normals[vertex],
            [0.4472136, 0.8944272, 0.0],
            1e-4,
        );
    }
}

/// A mesh primitive whose POSITION accessor cannot actually be read must be
/// a loud error, not silently dropped geometry (the project bans silent
/// failures).
///
/// A primitive that textually omits the POSITION key is rejected by
/// `gltf::import` itself before this crate's code ever runs (gltf-json's
/// schema validation requires every primitive to declare POSITION,
/// confirmed empirically: such a document fails with
/// `Gltf(Validation([.. "POSITION" .. Missing]))`). The reachable silent-drop
/// case is a POSITION *accessor* whose backing bufferView is smaller than
/// the accessor declares (schema-valid — gltf-json never cross-checks
/// accessor size against bufferView length) — the `gltf` crate's own
/// `read_positions()` returns `None` at read time in that case, which is
/// exactly the `None => return` silent-skip this test targets.
#[test]
fn primitive_with_unreadable_position_accessor_is_a_loud_error() {
    let mut bin = Vec::new();
    let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    for p in positions {
        for c in p {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let json = serde_json::json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0}],
        "meshes": [{"primitives": [
            {"attributes": {"POSITION": 0}},
            {"attributes": {"POSITION": 1}}
        ]}],
        "buffers": [{"byteLength": bin.len()}],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": 36},
            // Declares only 12 bytes backing an accessor that needs 36
            // (3 x VEC3 f32) — schema-valid, but too small to read.
            {"buffer": 0, "byteOffset": 0, "byteLength": 12}
        ],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
             "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]},
            {"bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3",
             "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]}
        ]
    });
    let file = write_temp_glb(&glb::write_glb(&json.to_string(), &bin));
    let error = load_scene(file.path())
        .expect_err("primitive with an unreadable POSITION accessor must be rejected");
    assert!(matches!(error, ImportError::MissingPositions));
}
