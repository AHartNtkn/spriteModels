# 3D Model Import Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Convert glTF/GLB models into `.depthsprite` documents through a graphical import dialog with orientation, side-mode, bounds, and lighting controls.

**Architecture:** A new UI-independent `mesh-import` crate loads glTF into a triangle soup, rasterizes it orthographically on the CPU (one rasterizer serves both height-field capture and the dialog's mesh preview), and packs captures into an `AuthoredModel`. The desktop app adds a File menu action and a modal dialog with two camera-synced viewports.

**Tech Stack:** Rust (edition 2024), `gltf = "=1.4.1"` (glTF loading, decodes embedded images; verified at docs.rs/gltf/1.4.1: `let (document, buffers, images) = gltf::import(path)?`), existing `relief-core`/`editor-core`/`relief-render`/eframe.

**Spec:** `docs/superpowers/specs/2026-07-17-model-import-design.md` — read it before starting any task.

## Global Constraints

- Bounds are `1..=63` per axis; relief is 8 units per model pixel; `h_max = 4·L` (opposing dimension `L`); alpha `255 − h`, alpha 0 = empty.
- All dependency versions exactly pinned (`=x.y.z`) in `[workspace.dependencies]`, matching existing style.
- Tests assert analytically-derived properties. Never byte-exact images, hashes, or stored reference outputs.
- Tests never touch the network. Fixtures are committed files; a missing fixture must fail loudly, never skip.
- No heuristics: every constant carries a principled justification in a comment or the spec.
- Commit message style: lowercase `feat:`/`test:`/`docs:` prefixes, ending with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
- `cargo test --workspace` must be green at the end of every task.

---

### Task 1: `mesh-import` crate scaffold and glTF scene loading

**Files:**
- Modify: `Cargo.toml` (workspace members + dependencies)
- Create: `crates/mesh-import/Cargo.toml`
- Create: `crates/mesh-import/src/lib.rs`
- Create: `crates/mesh-import/src/scene.rs`
- Create: `crates/mesh-import/src/error.rs`
- Test: `crates/mesh-import/tests/scene.rs`
- Test helper: `crates/mesh-import/tests/glb.rs` (GLB writer used by later test files too)

**Interfaces:**
- Consumes: `relief_core::{CanonicalView, ModelError}` (error variants referenced by `ImportError`, used fully in Task 3).
- Produces:
  - `mesh_import::TriangleScene { triangles: Vec<Triangle>, materials: Vec<Material> }`
  - `mesh_import::Triangle { positions: [[f32; 3]; 3], normals: [[f32; 3]; 3], uvs: [[f32; 2]; 3], colors: [[f32; 4]; 3], material: usize }`
  - `mesh_import::Material { base_color_factor: [f32; 4], base_color_texture: Option<Texture>, alpha_cutoff: Option<f32> }`
  - `mesh_import::Texture { width: u32, height: u32, rgba: Vec<[u8; 4]> }`
  - `mesh_import::load_scene(path: impl AsRef<Path>) -> Result<TriangleScene, ImportError>`
  - `mesh_import::ImportError` (thiserror enum)

- [ ] **Step 1: Wire the crate into the workspace**

In the root `Cargo.toml`:
- add `"crates/mesh-import"` to `[workspace] members` (alphabetical position: after `fixture-gen`... the list is alphabetical — place between `fixture-gen` and `relief-core`).
- add to `[workspace.dependencies]`: `gltf = "=1.4.1"`.
- add to the optimized-test-profile block at the bottom (rasterizing ~100k-triangle fixtures in an unoptimized profile is needlessly slow; identical reasoning to the existing entries):

```toml
[profile.test.package.mesh-import]
opt-level = 2
```

Create `crates/mesh-import/Cargo.toml`:

```toml
[package]
name = "mesh-import"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish = false

[dependencies]
gltf.workspace = true
relief-core = { path = "../relief-core" }
thiserror.workspace = true

[dev-dependencies]
serde_json.workspace = true
tempfile.workspace = true
```

Create `crates/mesh-import/src/error.rs`:

```rust
use relief_core::{CanonicalView, ModelError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ImportError {
    #[error("could not load glTF: {0}")]
    Gltf(#[from] gltf::Error),
    #[error("the scene contains no triangle geometry")]
    NoTriangles,
    #[error("no side is set to Capture")]
    NoCaptureSides,
    #[error("{side:?} is supplied by its opposite, but {opposite:?} is not captured")]
    UnsatisfiedOpposite {
        side: CanonicalView,
        opposite: CanonicalView,
    },
    #[error("longest axis {0} is outside 1..=63")]
    LongestAxisRange(u32),
    #[error(transparent)]
    Chart(#[from] relief_core::ChartError),
    #[error(transparent)]
    Model(#[from] ModelError),
}
```

Create `crates/mesh-import/src/lib.rs` (module list grows in later tasks):

```rust
mod error;
mod scene;

pub use error::ImportError;
pub use scene::{Material, Texture, Triangle, TriangleScene, load_scene};
```

- [ ] **Step 2: Write the failing scene-loading tests**

Create `crates/mesh-import/tests/glb.rs` — a minimal GLB writer so tests construct exact inputs without base64 or fixture files. GLB layout per the glTF 2.0 spec: 12-byte header (`glTF` magic, version 2, total length), then chunks (length, type, payload) with the JSON chunk space-padded and the BIN chunk zero-padded to 4-byte alignment.

```rust
//! Test-only GLB container writer. Kept as a separate integration-test
//! module (`mod glb;`) so every mesh-import test file can build exact
//! glTF inputs inline.

pub fn write_glb(json: &str, bin: &[u8]) -> Vec<u8> {
    let mut json_bytes = json.as_bytes().to_vec();
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }
    let mut bin_bytes = bin.to_vec();
    while bin_bytes.len() % 4 != 0 {
        bin_bytes.push(0);
    }
    let total = 12 + 8 + json_bytes.len() + 8 + bin_bytes.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&(bin_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin_bytes);
    out
}
```

Create `crates/mesh-import/tests/scene.rs` with these tests (complete file):

```rust
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
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p mesh-import`
Expected: compile error — `scene.rs` does not exist yet / `load_scene` unresolved.

- [ ] **Step 4: Implement `scene.rs`**

```rust
use std::path::Path;

use crate::ImportError;

#[derive(Clone)]
pub struct TriangleScene {
    pub triangles: Vec<Triangle>,
    pub materials: Vec<Material>,
}

#[derive(Clone, Copy)]
pub struct Triangle {
    pub positions: [[f32; 3]; 3],
    pub normals: [[f32; 3]; 3],
    pub uvs: [[f32; 2]; 3],
    pub colors: [[f32; 4]; 3],
    pub material: usize,
}

#[derive(Clone)]
pub struct Material {
    pub base_color_factor: [f32; 4],
    pub base_color_texture: Option<Texture>,
    /// `Some(cutoff)` for glTF `alphaMode: MASK`; `None` renders opaque
    /// (OPAQUE and BLEND — the depthsprite format has no translucency).
    pub alpha_cutoff: Option<f32>,
}

#[derive(Clone)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<[u8; 4]>,
}

pub fn load_scene(path: impl AsRef<Path>) -> Result<TriangleScene, ImportError> {
    let (document, buffers, images) = gltf::import(path)?;

    // Materials in document order; one extra default slot at the end for
    // primitives without a material (glTF's default material).
    let mut materials: Vec<Material> = document
        .materials()
        .map(|material| convert_material(&material, &images))
        .collect();
    let default_material = materials.len();
    materials.push(Material {
        base_color_factor: [1.0, 1.0, 1.0, 1.0],
        base_color_texture: None,
        alpha_cutoff: None,
    });

    let mut triangles = Vec::new();
    let scene = document
        .default_scene()
        .or_else(|| document.scenes().next())
        .ok_or(ImportError::NoTriangles)?;
    for node in scene.nodes() {
        collect_node(
            &node,
            [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]],
            &buffers,
            default_material,
            &mut triangles,
        );
    }
    if triangles.is_empty() {
        return Err(ImportError::NoTriangles);
    }
    Ok(TriangleScene {
        triangles,
        materials,
    })
}

fn convert_material(material: &gltf::Material<'_>, images: &[gltf::image::Data]) -> Material {
    let pbr = material.pbr_metallic_roughness();
    let base_color_texture = pbr.base_color_texture().map(|info| {
        let image = &images[info.texture().source().index()];
        decode_image(image)
    });
    let alpha_cutoff = match material.alpha_mode() {
        gltf::material::AlphaMode::Mask => Some(material.alpha_cutoff().unwrap_or(0.5)),
        gltf::material::AlphaMode::Opaque | gltf::material::AlphaMode::Blend => None,
    };
    Material {
        base_color_factor: pbr.base_color_factor(),
        base_color_texture,
        alpha_cutoff,
    }
}

/// Expand every gltf image format to RGBA8. 16-bit channels divide by 257
/// (the exact 65535 -> 255 ratio); float channels clamp to [0, 1].
fn decode_image(image: &gltf::image::Data) -> Texture {
    use gltf::image::Format;
    let texel_count = (image.width as usize) * (image.height as usize);
    let mut rgba = Vec::with_capacity(texel_count);
    let push = |rgba: &mut Vec<[u8; 4]>, r: u8, g: u8, b: u8, a: u8| rgba.push([r, g, b, a]);
    match image.format {
        Format::R8 => {
            for chunk in image.pixels.chunks_exact(1) {
                push(&mut rgba, chunk[0], chunk[0], chunk[0], 255);
            }
        }
        Format::R8G8 => {
            for chunk in image.pixels.chunks_exact(2) {
                push(&mut rgba, chunk[0], chunk[1], 0, 255);
            }
        }
        Format::R8G8B8 => {
            for chunk in image.pixels.chunks_exact(3) {
                push(&mut rgba, chunk[0], chunk[1], chunk[2], 255);
            }
        }
        Format::R8G8B8A8 => {
            for chunk in image.pixels.chunks_exact(4) {
                push(&mut rgba, chunk[0], chunk[1], chunk[2], chunk[3]);
            }
        }
        Format::R16 | Format::R16G16 | Format::R16G16B16 | Format::R16G16B16A16 => {
            let channels = match image.format {
                Format::R16 => 1,
                Format::R16G16 => 2,
                Format::R16G16B16 => 3,
                _ => 4,
            };
            for chunk in image.pixels.chunks_exact(2 * channels) {
                let mut texel = [0u8, 0, 0, 255];
                for (i, pair) in chunk.chunks_exact(2).enumerate() {
                    texel[i] = (u16::from_le_bytes([pair[0], pair[1]]) / 257) as u8;
                }
                if channels == 1 {
                    texel[1] = texel[0];
                    texel[2] = texel[0];
                }
                rgba.push(texel);
            }
        }
        Format::R32G32B32FLOAT | Format::R32G32B32A32FLOAT => {
            let channels = if image.format == Format::R32G32B32FLOAT { 3 } else { 4 };
            for chunk in image.pixels.chunks_exact(4 * channels) {
                let mut texel = [0u8, 0, 0, 255];
                for (i, quad) in chunk.chunks_exact(4).enumerate() {
                    let value = f32::from_le_bytes([quad[0], quad[1], quad[2], quad[3]]);
                    texel[i] = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
                }
                rgba.push(texel);
            }
        }
    }
    Texture {
        width: image.width,
        height: image.height,
        rgba,
    }
}

fn collect_node(
    node: &gltf::Node<'_>,
    parent: [[f32; 4]; 4],
    buffers: &[gltf::buffer::Data],
    default_material: usize,
    triangles: &mut Vec<Triangle>,
) {
    let local = node.transform().matrix();
    let world = matrix_multiply(parent, local);
    if let Some(mesh) = node.mesh() {
        for primitive in mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            collect_primitive(&primitive, world, buffers, default_material, triangles);
        }
    }
    for child in node.children() {
        collect_node(&child, world, buffers, default_material, triangles);
    }
}

fn collect_primitive(
    primitive: &gltf::mesh::Primitive<'_>,
    world: [[f32; 4]; 4],
    buffers: &[gltf::buffer::Data],
    default_material: usize,
    triangles: &mut Vec<Triangle>,
) {
    let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()].0[..]));
    let positions: Vec<[f32; 3]> = match reader.read_positions() {
        Some(positions) => positions.collect(),
        None => return,
    };
    let normals: Option<Vec<[f32; 3]>> = reader.read_normals().map(Iterator::collect);
    let uvs: Option<Vec<[f32; 2]>> = reader
        .read_tex_coords(0)
        .map(|coords| coords.into_f32().collect());
    let colors: Option<Vec<[f32; 4]>> = reader
        .read_colors(0)
        .map(|colors| colors.into_rgba_f32().collect());
    let indices: Vec<u32> = match reader.read_indices() {
        Some(indices) => indices.into_u32().collect(),
        None => (0..positions.len() as u32).collect(),
    };
    let material = primitive.material().index().unwrap_or(default_material);
    let normal_matrix = normal_matrix(world);

    for face in indices.chunks_exact(3) {
        let idx = [face[0] as usize, face[1] as usize, face[2] as usize];
        let world_positions = idx.map(|i| transform_point(world, positions[i]));
        let world_normals = match (&normals, normal_matrix) {
            (Some(normals), Some(matrix)) => {
                idx.map(|i| normalize(transform_vector(matrix, normals[i])))
            }
            // No source normals, or a singular node transform: the face
            // normal of the transformed triangle is the only defined normal.
            _ => [face_normal(world_positions); 3],
        };
        triangles.push(Triangle {
            positions: world_positions,
            normals: world_normals,
            uvs: idx.map(|i| uvs.as_ref().map_or([0.0, 0.0], |uvs| uvs[i])),
            colors: idx.map(|i| colors.as_ref().map_or([1.0, 1.0, 1.0, 1.0], |c| c[i])),
            material,
        });
    }
}

fn matrix_multiply(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    // glTF matrices are column-major: m[column][row].
    let mut out = [[0.0f32; 4]; 4];
    for (column, out_column) in out.iter_mut().enumerate() {
        for row in 0..4 {
            out_column[row] = (0..4).map(|k| a[k][row] * b[column][k]).sum();
        }
    }
    out
}

fn transform_point(m: [[f32; 4]; 4], p: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * p[0] + m[1][0] * p[1] + m[2][0] * p[2] + m[3][0],
        m[0][1] * p[0] + m[1][1] * p[1] + m[2][1] * p[2] + m[3][1],
        m[0][2] * p[0] + m[1][2] * p[1] + m[2][2] * p[2] + m[3][2],
    ]
}

/// Inverse-transpose of the upper-left 3x3, for normals. `None` when the
/// transform is singular.
fn normal_matrix(m: [[f32; 4]; 4]) -> Option<[[f32; 3]; 3]> {
    let a = [
        [m[0][0], m[1][0], m[2][0]],
        [m[0][1], m[1][1], m[2][1]],
        [m[0][2], m[1][2], m[2][2]],
    ];
    let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
        - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
    if det.abs() < f32::EPSILON {
        return None;
    }
    let inv_det = 1.0 / det;
    // Inverse via adjugate, then transpose: inverse-transpose[r][c] = cofactor[r][c] / det.
    let cofactor = |r: usize, c: usize| -> f32 {
        let sub: Vec<f32> = (0..3)
            .filter(|&i| i != r)
            .flat_map(|i| (0..3).filter(|&j| j != c).map(move |j| (i, j)))
            .map(|(i, j)| a[i][j])
            .collect();
        let minor = sub[0] * sub[3] - sub[1] * sub[2];
        if (r + c) % 2 == 0 { minor } else { -minor }
    };
    let mut out = [[0.0f32; 3]; 3];
    for (r, row) in out.iter_mut().enumerate() {
        for (c, value) in row.iter_mut().enumerate() {
            *value = cofactor(r, c) * inv_det;
        }
    }
    Some(out)
}

fn transform_vector(m: [[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len <= f32::EPSILON {
        return [0.0, 0.0, 0.0];
    }
    [v[0] / len, v[1] / len, v[2] / len]
}

fn face_normal(p: [[f32; 3]; 3]) -> [f32; 3] {
    let e1 = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
    let e2 = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
    normalize([
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ])
}
```

Note for the implementer: the exact `gltf` reader method names above (`read_positions`, `read_tex_coords(0).into_f32()`, `read_colors(0).into_rgba_f32()`, `read_indices().into_u32()`, `buffer::Data.0`) are from the gltf 1.4 API — if the compiler disagrees, check `docs.rs/gltf/1.4.1` rather than guessing; do not downgrade the logic.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p mesh-import`
Expected: all 4 tests PASS.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/mesh-import
git commit -m "feat: mesh-import crate with gltf scene loading"
```

---

### Task 2: Orthographic rasterizer

**Files:**
- Create: `crates/mesh-import/src/raster.rs`
- Modify: `crates/mesh-import/src/lib.rs` (add `mod raster; pub use raster::{Lighting, Raster, View, light_direction, rasterize};`)
- Test: `crates/mesh-import/tests/raster.rs`

**Interfaces:**
- Consumes: `TriangleScene`, `Triangle`, `Material`, `Texture` from Task 1.
- Produces:
  - `View { origin: [f32; 3], right: [f32; 3], down: [f32; 3], forward: [f32; 3], scale: f32, width: u32, height: u32 }` — screen x of point `p` is `dot(p − origin, right) · scale`, y likewise with `down`; depth is `dot(p − origin, forward)` in world units (unscaled).
  - `Lighting { direction: [f32; 3], ambient: f32 }` — `direction` is the unit vector *toward* the light, in the same space as the triangles.
  - `Raster { width: u32, height: u32, depth: Vec<f32>, color: Vec<[u8; 4]> }` — `depth[i] == f32::INFINITY` means uncovered; covered color alpha is 255, uncovered 0.
  - `rasterize(scene: &TriangleScene, view: &View, lighting: &Lighting) -> Raster`
  - `light_direction(azimuth_degrees: f32, elevation_degrees: f32) -> [f32; 3]` — azimuth 0/elevation 0 is `[0, 0, −1]` (light from the front); positive elevation raises the light (−y is up in box space); positive azimuth swings it toward +x.

- [ ] **Step 1: Write the failing rasterizer tests**

Create `crates/mesh-import/tests/raster.rs`:

```rust
use mesh_import::{Lighting, Material, Texture, Triangle, TriangleScene, View, light_direction,
                  rasterize};

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
        tri(v(0.0, 0.0), v(4.0, 0.0), v(4.0, 4.0), [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]),
        tri(v(0.0, 0.0), v(4.0, 4.0), v(0.0, 4.0), [[0.0, 0.0], [1.0, 1.0], [0.0, 1.0]]),
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
    Lighting { direction: [0.0, 0.0, -1.0], ambient: 1.0 }
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
            assert_eq!(raster.color[i], [255, 255, 255, 255], "texel ({x},{y}) color");
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
        assert_eq!(raster.color[i], [255, 0, 0, 255], "texel {i} must hold the nearer color");
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
        &Lighting { direction: [0.0, 0.0, -1.0], ambient: 0.25 },
    );
    assert_eq!(full.color[0], [255, 255, 255, 255]);

    // Light from behind: max(0, n.l) = 0, so shade = ambient = 0.25.
    let back = rasterize(
        &scene,
        &front_view(),
        &Lighting { direction: [0.0, 0.0, 1.0], ambient: 0.25 },
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
        &Lighting { direction: [0.0, 0.0, -1.0], ambient: 0.0 },
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
fn light_direction_places_the_light_by_azimuth_and_elevation() {
    let front = light_direction(0.0, 0.0);
    assert!((front[0]).abs() < 1e-6 && (front[1]).abs() < 1e-6 && (front[2] + 1.0).abs() < 1e-6);
    let overhead = light_direction(0.0, 90.0);
    // -y is up in box space.
    assert!((overhead[1] + 1.0).abs() < 1e-6);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p mesh-import --test raster`
Expected: compile error — `rasterize` unresolved.

- [ ] **Step 3: Implement `raster.rs`**

```rust
use crate::{Material, Texture, TriangleScene};

pub struct View {
    pub origin: [f32; 3],
    pub right: [f32; 3],
    pub down: [f32; 3],
    pub forward: [f32; 3],
    pub scale: f32,
    pub width: u32,
    pub height: u32,
}

pub struct Lighting {
    /// Unit vector toward the light, in the triangles' space.
    pub direction: [f32; 3],
    pub ambient: f32,
}

pub struct Raster {
    pub width: u32,
    pub height: u32,
    /// `f32::INFINITY` marks an uncovered texel.
    pub depth: Vec<f32>,
    /// Covered texels have alpha 255; uncovered texels are [0, 0, 0, 0].
    pub color: Vec<[u8; 4]>,
}

pub fn light_direction(azimuth_degrees: f32, elevation_degrees: f32) -> [f32; 3] {
    let azimuth = azimuth_degrees.to_radians();
    let elevation = elevation_degrees.to_radians();
    [
        azimuth.sin() * elevation.cos(),
        -elevation.sin(),
        -azimuth.cos() * elevation.cos(),
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn rasterize(scene: &TriangleScene, view: &View, lighting: &Lighting) -> Raster {
    let width = view.width as usize;
    let height = view.height as usize;
    let mut depth = vec![f32::INFINITY; width * height];
    let mut color = vec![[0u8; 4]; width * height];

    for tri in &scene.triangles {
        // Project vertices to screen space.
        let mut screen = [[0.0f32; 3]; 3];
        for (vertex, out) in tri.positions.iter().zip(screen.iter_mut()) {
            let rel = [
                vertex[0] - view.origin[0],
                vertex[1] - view.origin[1],
                vertex[2] - view.origin[2],
            ];
            *out = [
                dot(rel, view.right) * view.scale,
                dot(rel, view.down) * view.scale,
                dot(rel, view.forward),
            ];
        }
        let [s0, s1, s2] = screen;
        let mut area =
            (s1[0] - s0[0]) * (s2[1] - s0[1]) - (s1[1] - s0[1]) * (s2[0] - s0[0]);
        // Two-sided: a negative area is a back-facing winding, sampled by
        // negating the barycentric weights rather than culling.
        let flip = if area < 0.0 { -1.0 } else { 1.0 };
        area *= flip;
        if area <= f32::EPSILON {
            continue;
        }
        let inv_area = 1.0 / area;

        let min_x = s0[0].min(s1[0]).min(s2[0]).floor().max(0.0) as usize;
        let max_x = (s0[0].max(s1[0]).max(s2[0]).ceil().max(0.0) as usize).min(width);
        let min_y = s0[1].min(s1[1]).min(s2[1]).floor().max(0.0) as usize;
        let max_y = (s0[1].max(s1[1]).max(s2[1]).ceil().max(0.0) as usize).min(height);
        let material = &scene.materials[tri.material];

        for py in min_y..max_y {
            let y = py as f32 + 0.5;
            for px in min_x..max_x {
                let x = px as f32 + 0.5;
                let w0 = flip
                    * ((s1[0] - x) * (s2[1] - y) - (s1[1] - y) * (s2[0] - x))
                    * inv_area;
                let w1 = flip
                    * ((s2[0] - x) * (s0[1] - y) - (s2[1] - y) * (s0[0] - x))
                    * inv_area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * s0[2] + w1 * s1[2] + w2 * s2[2];
                let index = py * width + px;
                if z >= depth[index] {
                    continue;
                }
                let interpolate3 = |values: [[f32; 3]; 3]| {
                    [
                        w0 * values[0][0] + w1 * values[1][0] + w2 * values[2][0],
                        w0 * values[0][1] + w1 * values[1][1] + w2 * values[2][1],
                        w0 * values[0][2] + w1 * values[1][2] + w2 * values[2][2],
                    ]
                };
                let uv = [
                    w0 * tri.uvs[0][0] + w1 * tri.uvs[1][0] + w2 * tri.uvs[2][0],
                    w0 * tri.uvs[0][1] + w1 * tri.uvs[1][1] + w2 * tri.uvs[2][1],
                ];
                let vertex_color = [
                    w0 * tri.colors[0][0] + w1 * tri.colors[1][0] + w2 * tri.colors[2][0],
                    w0 * tri.colors[0][1] + w1 * tri.colors[1][1] + w2 * tri.colors[2][1],
                    w0 * tri.colors[0][2] + w1 * tri.colors[1][2] + w2 * tri.colors[2][2],
                    w0 * tri.colors[0][3] + w1 * tri.colors[1][3] + w2 * tri.colors[2][3],
                ];
                let texel = material
                    .base_color_texture
                    .as_ref()
                    .map_or([1.0, 1.0, 1.0, 1.0], |texture| sample_bilinear(texture, uv));
                let alpha = material.base_color_factor[3] * texel[3] * vertex_color[3];
                if let Some(cutoff) = material.alpha_cutoff
                    && alpha < cutoff
                {
                    continue;
                }
                depth[index] = z;

                let mut normal = interpolate3(tri.normals);
                let len = dot(normal, normal).sqrt();
                if len > f32::EPSILON {
                    normal = [normal[0] / len, normal[1] / len, normal[2] / len];
                }
                // Two-sided shading: flip a normal that faces away from
                // the viewer so open meshes do not shade black inside.
                if dot(normal, view.forward) > 0.0 {
                    normal = [-normal[0], -normal[1], -normal[2]];
                }
                let lambert = dot(normal, lighting.direction).max(0.0);
                let shade = lighting.ambient + (1.0 - lighting.ambient) * lambert;
                let mut out = [0u8; 4];
                for channel in 0..3 {
                    let base = material.base_color_factor[channel]
                        * texel[channel]
                        * vertex_color[channel];
                    out[channel] = (base * shade * 255.0).round().clamp(0.0, 255.0) as u8;
                }
                out[3] = 255;
                color[index] = out;
            }
        }
    }
    Raster {
        width: view.width,
        height: view.height,
        depth,
        color,
    }
}

/// Bilinear sample with REPEAT wrapping (the glTF sampler default).
fn sample_bilinear(texture: &Texture, uv: [f32; 2]) -> [f32; 4] {
    let wrap = |v: f32| v - v.floor();
    let x = wrap(uv[0]) * texture.width as f32 - 0.5;
    let y = wrap(uv[1]) * texture.height as f32 - 0.5;
    let x0 = x.floor() as i64;
    let y0 = y.floor() as i64;
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let texel = |tx: i64, ty: i64| -> [f32; 4] {
        let tx = tx.rem_euclid(texture.width as i64) as usize;
        let ty = ty.rem_euclid(texture.height as i64) as usize;
        let raw = texture.rgba[ty * texture.width as usize + tx];
        raw.map(|channel| channel as f32 / 255.0)
    };
    let (t00, t10, t01, t11) = (texel(x0, y0), texel(x0 + 1, y0), texel(x0, y0 + 1),
                                texel(x0 + 1, y0 + 1));
    let mut out = [0.0f32; 4];
    for channel in 0..4 {
        let top = t00[channel] * (1.0 - fx) + t10[channel] * fx;
        let bottom = t01[channel] * (1.0 - fx) + t11[channel] * fx;
        out[channel] = top * (1.0 - fy) + bottom * fy;
    }
    out
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p mesh-import`
Expected: all scene + raster tests PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/mesh-import
git commit -m "feat: orthographic rasterizer with baked lambert shading"
```

---

### Task 3: Capture, side modes, and `convert`

**Files:**
- Create: `crates/mesh-import/src/capture.rs`
- Modify: `crates/mesh-import/src/lib.rs` (add `mod capture; pub use capture::{ALL_VIEWS, ImportSettings, SideMode, SideModes, box_space_scene, convert, derived_bounds};`)
- Test: `crates/mesh-import/tests/capture.rs`

**Interfaces:**
- Consumes: Task 1 scene types; Task 2 `rasterize`/`View`/`Lighting`/`light_direction`; `relief_core::{AuthoredModel, Bounds, CanonicalView, Chart}` (notably `CanonicalView::frame(bounds) -> CanonicalFrame { origin, source_u, source_v, inward }`, `CanonicalView::dimensions(bounds) -> (u32, u32)`, `CanonicalView::maximum_inward_depth(bounds) -> u8`, `Chart::from_rgba(view, w, h, rgba)`, `Chart::with_opposite_assignment()`, `Chart::with_mirrored_opposite()`, `AuthoredModel::new(bounds, charts)`).
- Produces:
  - `SideMode { Capture, FromOpposite, FromOppositeMirrored, Off }` (`Copy, Eq`)
  - `SideModes` with `get(view) -> SideMode`, `set(view, mode)` (resets a dependent opposite to `Off` when its supplier stops capturing; rejects `FromOpposite*` when the opposite is not `Capture`), `Default` = all `Capture`, `validate() -> Result<(), ImportError>`
  - `ImportSettings { rotation: [[f32; 3]; 3], side_modes: SideModes, longest_axis_pixels: u32, light_azimuth_degrees: f32, light_elevation_degrees: f32, ambient: f32 }` (`Clone, PartialEq`; `Default` = identity rotation, all Capture, 63, −35.0, 35.0, 0.25 — the spec's defaults)
  - `derived_bounds(scene: &TriangleScene, rotation: [[f32; 3]; 3], longest_axis_pixels: u32) -> Result<Bounds, ImportError>`
  - `convert(scene: &TriangleScene, settings: &ImportSettings) -> Result<AuthoredModel, ImportError>`

- [ ] **Step 1: Write the failing capture tests**

Create `crates/mesh-import/tests/capture.rs`:

```rust
use mesh_import::{ImportError, ImportSettings, Material, SideMode, Triangle, TriangleScene,
                  convert, derived_bounds};
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
    TriangleScene { triangles, materials: vec![plain_material()] }
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
    for side in [CanonicalView::Back, CanonicalView::Left, CanonicalView::Right,
                 CanonicalView::Top, CanonicalView::Bottom] {
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
        assert_eq!(255 - texel[3], h_max, "centered flat quad sits on the midplane");
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
    modes.set(CanonicalView::Back, SideMode::FromOppositeMirrored).expect("legal");
    modes.set(CanonicalView::Right, SideMode::FromOpposite).expect("legal");
    modes.set(CanonicalView::Top, SideMode::Off).expect("legal");
    modes.set(CanonicalView::Bottom, SideMode::Off).expect("legal");
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
    modes.set(CanonicalView::Front, SideMode::Off).expect("legal");
    assert!(modes.set(CanonicalView::Back, SideMode::FromOpposite).is_err());

    // Un-capturing a supplier resets its dependent to Off.
    let mut modes = mesh_import::SideModes::default();
    modes.set(CanonicalView::Back, SideMode::FromOpposite).expect("legal");
    modes.set(CanonicalView::Front, SideMode::Off).expect("legal");
    assert_eq!(modes.get(CanonicalView::Back), SideMode::Off);

    // All-off conversion is rejected.
    let mut config = ImportSettings::default();
    let mut modes = mesh_import::SideModes::default();
    for side in [CanonicalView::Front, CanonicalView::Back, CanonicalView::Left,
                 CanonicalView::Right, CanonicalView::Top, CanonicalView::Bottom] {
        modes.set(side, SideMode::Off).expect("legal");
    }
    config.side_modes = modes;
    assert!(matches!(convert(&cube(), &config), Err(ImportError::NoCaptureSides)));
}
```


- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p mesh-import --test capture`
Expected: compile error — `convert` unresolved.

- [ ] **Step 3: Implement `capture.rs`**

```rust
use relief_core::{AuthoredModel, Bounds, CanonicalView, Chart};

use crate::{ImportError, Lighting, Triangle, TriangleScene, View, light_direction, rasterize};

pub const ALL_VIEWS: [CanonicalView; 6] = [
    CanonicalView::Front,
    CanonicalView::Back,
    CanonicalView::Left,
    CanonicalView::Right,
    CanonicalView::Top,
    CanonicalView::Bottom,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SideMode {
    Capture,
    FromOpposite,
    FromOppositeMirrored,
    Off,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SideModes {
    modes: [SideMode; 6],
}

impl Default for SideModes {
    fn default() -> Self {
        Self { modes: [SideMode::Capture; 6] }
    }
}

impl SideModes {
    pub fn get(&self, view: CanonicalView) -> SideMode {
        self.modes[view.rank() as usize]
    }

    /// Sets one side's mode. `FromOpposite*` requires the opposite side to
    /// be `Capture`. Moving a side out of `Capture` resets an opposite that
    /// depended on it to `Off`.
    pub fn set(&mut self, view: CanonicalView, mode: SideMode) -> Result<(), ImportError> {
        if matches!(mode, SideMode::FromOpposite | SideMode::FromOppositeMirrored)
            && self.get(view.opposite()) != SideMode::Capture
        {
            return Err(ImportError::UnsatisfiedOpposite {
                side: view,
                opposite: view.opposite(),
            });
        }
        self.modes[view.rank() as usize] = mode;
        if mode != SideMode::Capture
            && matches!(
                self.get(view.opposite()),
                SideMode::FromOpposite | SideMode::FromOppositeMirrored
            )
        {
            self.modes[view.opposite().rank() as usize] = SideMode::Off;
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), ImportError> {
        for view in ALL_VIEWS {
            if matches!(
                self.get(view),
                SideMode::FromOpposite | SideMode::FromOppositeMirrored
            ) && self.get(view.opposite()) != SideMode::Capture
            {
                return Err(ImportError::UnsatisfiedOpposite {
                    side: view,
                    opposite: view.opposite(),
                });
            }
        }
        if ALL_VIEWS.iter().all(|&view| self.get(view) != SideMode::Capture) {
            return Err(ImportError::NoCaptureSides);
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq)]
pub struct ImportSettings {
    /// Rotation applied to the mesh before fitting (mesh -> box frame).
    pub rotation: [[f32; 3]; 3],
    pub side_modes: SideModes,
    pub longest_axis_pixels: u32,
    pub light_azimuth_degrees: f32,
    pub light_elevation_degrees: f32,
    pub ambient: f32,
}

impl Default for ImportSettings {
    fn default() -> Self {
        Self {
            rotation: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            side_modes: SideModes::default(),
            longest_axis_pixels: 63,
            // Spec defaults: light from upper-front-left, ambient 0.25.
            light_azimuth_degrees: -35.0,
            light_elevation_degrees: 35.0,
            ambient: 0.25,
        }
    }
}

fn rotate(rotation: [[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        rotation[0][0] * v[0] + rotation[0][1] * v[1] + rotation[0][2] * v[2],
        rotation[1][0] * v[0] + rotation[1][1] * v[1] + rotation[1][2] * v[2],
        rotation[2][0] * v[0] + rotation[2][1] * v[1] + rotation[2][2] * v[2],
    ]
}

struct Fit {
    bounds: Bounds,
    scale: f32,
    rotated_min: [f32; 3],
    offset: [f32; 3],
}

fn fit(
    scene: &TriangleScene,
    rotation: [[f32; 3]; 3],
    longest_axis_pixels: u32,
) -> Result<Fit, ImportError> {
    if !(1..=63).contains(&longest_axis_pixels) {
        return Err(ImportError::LongestAxisRange(longest_axis_pixels));
    }
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for tri in &scene.triangles {
        for &p in &tri.positions {
            let r = rotate(rotation, p);
            for axis in 0..3 {
                min[axis] = min[axis].min(r[axis]);
                max[axis] = max[axis].max(r[axis]);
            }
        }
    }
    let extents = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
    let longest = extents[0].max(extents[1]).max(extents[2]);
    // A scene of coincident points has no extent to scale; scale 1 keeps
    // the (single-texel) geometry finite.
    let scale = if longest > 0.0 {
        longest_axis_pixels as f32 / longest
    } else {
        1.0
    };
    // ceil so the mesh always fits inside the box (fitting itself never
    // clamps); min() removes float overshoot past the mathematical bound
    // extents * scale <= longest_axis_pixels; floor of 1 for flat axes.
    let dim = |extent: f32| -> u32 {
        ((extent * scale).ceil() as u32)
            .min(longest_axis_pixels)
            .max(1)
    };
    let bounds = Bounds::new(dim(extents[0]), dim(extents[1]), dim(extents[2]))?;
    let dims = [bounds.width() as f32, bounds.height() as f32, bounds.depth() as f32];
    // Center the mesh inside the box on each axis.
    let offset = [
        (dims[0] - extents[0] * scale) / 2.0,
        (dims[1] - extents[1] * scale) / 2.0,
        (dims[2] - extents[2] * scale) / 2.0,
    ];
    Ok(Fit { bounds, scale, rotated_min: min, offset })
}

fn to_box_space(scene: &TriangleScene, rotation: [[f32; 3]; 3], fit: &Fit) -> Vec<Triangle> {
    scene
        .triangles
        .iter()
        .map(|tri| {
            let mut out = *tri;
            for vertex in 0..3 {
                let r = rotate(rotation, tri.positions[vertex]);
                out.positions[vertex] = [
                    (r[0] - fit.rotated_min[0]) * fit.scale + fit.offset[0],
                    (r[1] - fit.rotated_min[1]) * fit.scale + fit.offset[1],
                    (r[2] - fit.rotated_min[2]) * fit.scale + fit.offset[2],
                ];
                // Uniform scale + rotation: normals rotate, no re-scaling.
                out.normals[vertex] = rotate(rotation, tri.normals[vertex]);
            }
            out
        })
        .collect()
}

pub fn derived_bounds(
    scene: &TriangleScene,
    rotation: [[f32; 3]; 3],
    longest_axis_pixels: u32,
) -> Result<Bounds, ImportError> {
    Ok(fit(scene, rotation, longest_axis_pixels)?.bounds)
}

/// The box-space triangle scene plus bounds, shared by capture and the
/// dialog's mesh preview so both always show identical geometry.
pub fn box_space_scene(
    scene: &TriangleScene,
    rotation: [[f32; 3]; 3],
    longest_axis_pixels: u32,
) -> Result<(TriangleScene, Bounds), ImportError> {
    let fit = fit(scene, rotation, longest_axis_pixels)?;
    let triangles = to_box_space(scene, rotation, &fit);
    Ok((
        TriangleScene {
            triangles,
            materials: scene.materials.clone(),
        },
        fit.bounds,
    ))
}

pub fn convert(
    scene: &TriangleScene,
    settings: &ImportSettings,
) -> Result<AuthoredModel, ImportError> {
    settings.side_modes.validate()?;
    let (box_scene, bounds) = box_space_scene(scene, settings.rotation, settings.longest_axis_pixels)?;
    let lighting = Lighting {
        direction: light_direction(
            settings.light_azimuth_degrees,
            settings.light_elevation_degrees,
        ),
        ambient: settings.ambient,
    };

    let mut charts = Vec::new();
    for view in ALL_VIEWS {
        if settings.side_modes.get(view) != SideMode::Capture {
            continue;
        }
        let frame = view.frame(bounds);
        let (width, height) = view.dimensions(bounds);
        let raster = rasterize(
            &box_scene,
            &View {
                origin: frame.origin.map(|c| c as f32),
                right: frame.source_u.map(|c| c as f32),
                down: frame.source_v.map(|c| c as f32),
                forward: frame.inward.map(|c| c as f32),
                scale: 1.0,
                width,
                height,
            },
            &lighting,
        );
        let h_max = i64::from(view.maximum_inward_depth(bounds));
        let rgba: Vec<[u8; 4]> = raster
            .depth
            .iter()
            .zip(raster.color.iter())
            .map(|(&depth, &color)| {
                if depth == f32::INFINITY {
                    [0, 0, 0, 0]
                } else {
                    // depth is in model pixels from the face plane; float
                    // error can dip epsilon-negative, clamp handles it.
                    let relief = ((f64::from(depth) * 8.0).round() as i64).clamp(0, h_max);
                    [color[0], color[1], color[2], (255 - relief) as u8]
                }
            })
            .collect();
        let mut chart = Chart::from_rgba(view, width, height, rgba)?;
        let opposite_mode = settings.side_modes.get(view.opposite());
        if opposite_mode == SideMode::FromOpposite {
            chart = chart.with_opposite_assignment();
        }
        if opposite_mode == SideMode::FromOppositeMirrored {
            chart = chart.with_opposite_assignment().with_mirrored_opposite();
        }
        charts.push(chart);
    }
    Ok(AuthoredModel::new(bounds, charts)?)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p mesh-import`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/mesh-import
git commit -m "feat: height-field capture and side-mode conversion to AuthoredModel"
```

---

### Task 4: Real-model fixtures and invariant tests

**Files:**
- Create: `crates/mesh-import/tests/fixtures/` (committed GLBs + README)
- Test: `crates/mesh-import/tests/real_models.rs`

**Interfaces:**
- Consumes: `load_scene`, `convert`, `ImportSettings`, `SideMode` from Tasks 1–3.
- Produces: committed fixture files `teapot.glb`, `stanford-bunny.glb`, `xyzrgb_dragon.glb`, `earth.glb`.

**Fixture provenance (verified 2026-07-17):**

| Fixture | Source | License/provenance |
| --- | --- | --- |
| teapot.obj | `https://raw.githubusercontent.com/alecjacobson/common-3d-test-models/master/data/teapot.obj` | Martin Newell's Utah teapot; repo gives source attribution, no formal license |
| stanford-bunny.obj | `https://raw.githubusercontent.com/alecjacobson/common-3d-test-models/master/data/stanford-bunny.obj` | Stanford 3D Scanning Repository scan |
| xyzrgb_dragon.obj | `https://raw.githubusercontent.com/alecjacobson/common-3d-test-models/master/data/xyzrgb_dragon.obj` | Stanford 3D Scanning Repository scan (repo-decimated ~10 MB OBJ) |
| earth.glb | `https://assets.science.nasa.gov/content/dam/science/psd/solar/2023/09/e/Earth_1_12756.glb` (12.32 MB) | NASA VTAD; NASA media usage guidelines |

OBJ→GLB conversion via `obj2gltf` (CesiumGS, Apache-2.0; CLI verified: `obj2gltf -i model.obj -o model.glb`).

- [ ] **Step 1: Provision fixtures (one-time, network use is here and only here)**

```bash
mkdir -p crates/mesh-import/tests/fixtures
cd /tmp/claude-1001 2>/dev/null || cd /tmp
for name in teapot stanford-bunny xyzrgb_dragon; do
  curl -fL -o "$name.obj" "https://raw.githubusercontent.com/alecjacobson/common-3d-test-models/master/data/$name.obj"
done
cd - >/dev/null
for name in teapot stanford-bunny xyzrgb_dragon; do
  npx --yes obj2gltf -i "/tmp/$name.obj" -o "crates/mesh-import/tests/fixtures/$name.glb"
done
curl -fL -o crates/mesh-import/tests/fixtures/earth.glb \
  "https://assets.science.nasa.gov/content/dam/science/psd/solar/2023/09/e/Earth_1_12756.glb"
ls -la crates/mesh-import/tests/fixtures/
```

(If the `/tmp/claude-1001` path does not exist, download to any scratch directory; the committed artifact is only the four GLBs.) Expected: four `.glb` files, each nonzero size. If any download or conversion fails, STOP and report — do not substitute or skip.

Write `crates/mesh-import/tests/fixtures/README.md` recording the table above verbatim (sources, dates, conversion command, obj2gltf version from `npx obj2gltf --version`).

- [ ] **Step 2: Write the failing real-model tests**

Create `crates/mesh-import/tests/real_models.rs`:

```rust
use std::path::PathBuf;

use mesh_import::{ImportSettings, TriangleScene, convert, load_scene};
use relief_core::{AuthoredModel, CanonicalView};

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

const ALL_VIEWS: [CanonicalView; 6] = [
    CanonicalView::Front,
    CanonicalView::Back,
    CanonicalView::Left,
    CanonicalView::Right,
    CanonicalView::Top,
    CanonicalView::Bottom,
];

/// Format invariants every conversion must satisfy: chart dims match the
/// bounds, and every texel is either empty (alpha 0) or has relief
/// h = 255 - alpha within 0..=h_max.
fn assert_format_invariants(model: &AuthoredModel) {
    for chart in model.charts() {
        let view = chart.view();
        assert_eq!(chart.dimensions(), view.dimensions(model.bounds()));
        let h_max = view.maximum_inward_depth(model.bounds());
        for &texel in chart.rgba() {
            if texel[3] == 0 {
                continue;
            }
            let relief = 255 - texel[3];
            assert!(
                relief <= h_max,
                "{view:?}: relief {relief} exceeds h_max {h_max}"
            );
        }
    }
}

fn assert_full_conversion(name: &str, minimum_triangles: usize) {
    let scene = fixture(name);
    assert!(
        scene.triangles.len() >= minimum_triangles,
        "{name}: {} triangles, expected at least {minimum_triangles}",
        scene.triangles.len()
    );
    for longest in [63, 32, 7] {
        let settings = ImportSettings { longest_axis_pixels: longest, ..Default::default() };
        let model = convert(&scene, &settings)
            .unwrap_or_else(|error| panic!("{name} at {longest}px must convert: {error}"));
        assert_eq!(model.charts().len(), 6);
        assert_format_invariants(&model);
        // Closed meshes must be visible from every axis.
        for view in ALL_VIEWS {
            let chart = model.chart(view).expect("all six captured");
            let covered = chart.rgba().iter().filter(|texel| texel[3] != 0).count();
            assert!(covered > 0, "{name} {view:?} at {longest}px has no coverage");
        }
    }
}

#[test]
fn teapot_converts_with_invariants() {
    assert_full_conversion("teapot.glb", 1_000);
}

#[test]
fn bunny_converts_with_invariants() {
    assert_full_conversion("stanford-bunny.glb", 10_000);
}

#[test]
fn dragon_converts_with_invariants() {
    assert_full_conversion("xyzrgb_dragon.glb", 10_000);
}

#[test]
fn earth_sphere_front_capture_is_a_textured_disc() {
    let scene = fixture("earth.glb");
    let model = convert(&scene, &ImportSettings::default()).expect("earth converts");
    assert_format_invariants(&model);
    let front = model.chart(CanonicalView::Front).expect("front chart");
    let (width, height) = front.dimensions();
    assert_eq!((width, height), (63, 63));

    // Center texel: the sphere touches the front face; with 0.5-texel
    // parallax on a radius-31.5px sphere the sag is r - sqrt(r^2 - 0.5^2)
    // < 0.004 px, so quantized relief at the center is at most one unit.
    let center = front.rgba()[(31 * 63 + 31) as usize];
    let center_relief = 255 - center[3];
    assert!(center_relief <= 1, "center relief {center_relief} must be ~0");

    // Silhouette circularity: covered area within one boundary-texel
    // annulus (2*pi*R ~ 198 texels) of the ideal disc pi*R^2, R = 31.5.
    let covered = front.rgba().iter().filter(|texel| texel[3] != 0).count() as f64;
    let ideal = std::f64::consts::PI * 31.5 * 31.5;
    let annulus = 2.0 * std::f64::consts::PI * 31.5;
    assert!(
        (covered - ideal).abs() <= annulus,
        "covered {covered} vs disc {ideal:.0} exceeds boundary annulus {annulus:.0}"
    );

    // Texture liveness: a constant-color capture means texture sampling is
    // dead. Earth's oceans and land must differ somewhere.
    let mut colors: Vec<[u8; 3]> = front
        .rgba()
        .iter()
        .filter(|texel| texel[3] != 0)
        .map(|texel| [texel[0], texel[1], texel[2]])
        .collect();
    colors.sort();
    colors.dedup();
    assert!(colors.len() > 1, "captured earth color must vary across the surface");
}
```

- [ ] **Step 3: Run tests to verify they fail correctly**

Run: `cargo test -p mesh-import --test real_models`
Expected: PASS if Tasks 1–3 are correct — this task's failure mode is fixture problems (missing file → loud panic with provisioning instructions). If a test fails, the conversion pipeline has a real bug: investigate, do not weaken the assertion. If the earth model's scene graph or scale makes an assertion fail (e.g., the GLB contains clouds as a second slightly-larger sphere), inspect with a small debug print of bounds/coverage, fix the *test's geometric premise* only if the premise is factually wrong about the fixture, and record the correction in the fixtures README.

- [ ] **Step 4: Commit (fixtures + tests together)**

```bash
git add crates/mesh-import/tests
git commit -m "test: real-model fixtures with conversion invariants"
```

---

### Task 5: editor-core additions — `from_unsaved_model` and `basis_f32`

**Files:**
- Modify: `crates/editor-core/src/document.rs`
- Modify: `crates/editor-core/src/camera.rs`
- Test: `crates/editor-core/tests/document.rs` (append)

**Interfaces:**
- Consumes: existing `EditorDocument`/`OrbitCamera` internals.
- Produces:
  - `EditorDocument::from_unsaved_model(model: AuthoredModel) -> Self` — pathless and **dirty**: its saved-state counterpart is the empty document, because nothing on disk holds this content; the unsaved-changes prompt must protect an import.
  - `OrbitCamera::basis_f32(self) -> [[f32; 3]; 3]` — rows `[right, down, forward]`, the same trigonometry as `target_view()` without the 1/1024 quantization (the mesh rasterizer needs plain floats; visually identical).

- [ ] **Step 1: Write the failing tests** (append to `crates/editor-core/tests/document.rs`; adapt imports to the file's existing ones)

```rust
#[test]
fn imported_model_document_is_untitled_and_dirty() {
    let bounds = Bounds::new(4, 4, 4).unwrap();
    let model = AuthoredModel::with_empty_chart(bounds, CanonicalView::Front).unwrap();
    let document = EditorDocument::from_unsaved_model(model);
    assert!(document.path().is_none());
    assert!(
        document.is_dirty(),
        "an imported model has no saved counterpart and must prompt before discard"
    );
}

#[test]
fn default_orbit_basis_is_orthonormal_and_matches_default_angles() {
    let basis = OrbitCamera::default().basis_f32();
    for row in basis {
        let len = (row[0] * row[0] + row[1] * row[1] + row[2] * row[2]).sqrt();
        assert!((len - 1.0).abs() < 1e-5);
    }
    // Default yaw 45deg: right = [cos45, 0, sin45].
    assert!((basis[0][0] - 45f32.to_radians().cos()).abs() < 1e-5);
    assert!((basis[0][2] - 45f32.to_radians().sin()).abs() < 1e-5);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p editor-core --test document`
Expected: compile error — the two methods don't exist.

- [ ] **Step 3: Implement**

In `document.rs`, next to `from_model`:

```rust
/// A document for a freshly imported model: untitled, and dirty because
/// no file holds this content yet. Its saved-state baseline is the empty
/// document so any nonempty import differs from "what is persisted".
pub fn from_unsaved_model(model: AuthoredModel) -> Self {
    let bounds = model.bounds();
    let selection = model.charts()[0].view();
    let saved_model = AuthoredModel::with_empty_chart(bounds, selection)
        .expect("validated bounds always produce a valid empty chart");
    let make_state = |model: AuthoredModel| DocumentState {
        model,
        selection,
        active_layer: ActiveLayer::Color,
        tool: Tool::Pencil,
        current_rgb: [0, 0, 0],
        current_depth: DepthValue::Relief(
            ReliefValue::new(0).expect("zero relief is always valid"),
        ),
    };
    Self {
        saved_state: make_state(saved_model),
        state: make_state(model),
        undo: Vec::new(),
        redo: Vec::new(),
        stroke_before: None,
        path: None,
        revision: 0,
        render_identity: NEXT_RENDER_IDENTITY.fetch_add(1, Ordering::Relaxed),
    }
}
```

In `camera.rs`, next to `target_view`:

```rust
/// The camera basis as plain floats: rows are screen-right, screen-down,
/// and view-forward in world coordinates. Same trigonometry as
/// `target_view` without ratio quantization; used by the import dialog's
/// mesh rasterizer.
pub fn basis_f32(self) -> [[f32; 3]; 3] {
    let yaw = millidegrees_to_radians(self.yaw_millidegrees);
    let pitch = millidegrees_to_radians(self.pitch_millidegrees);
    let (sin_yaw, cos_yaw) = yaw.sin_cos();
    let (sin_pitch, cos_pitch) = pitch.sin_cos();
    [
        [cos_yaw as f32, 0.0, sin_yaw as f32],
        [
            (sin_yaw * sin_pitch) as f32,
            cos_pitch as f32,
            (-cos_yaw * sin_pitch) as f32,
        ],
        [
            (-sin_yaw * cos_pitch) as f32,
            sin_pitch as f32,
            (cos_yaw * cos_pitch) as f32,
        ],
    ]
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p editor-core`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/editor-core
git commit -m "feat: unsaved-model documents and float camera basis for import"
```

---

### Task 6: Menu action, file picking, and import destructive flow

**Files:**
- Modify: `crates/desktop-app/Cargo.toml` (add `mesh-import = { path = "../mesh-import" }`)
- Modify: `crates/desktop-app/src/menu.rs`
- Modify: `crates/desktop-app/src/app.rs`
- Test: `crates/desktop-app/tests/menu.rs`, `crates/desktop-app/tests/application.rs` (append; follow each file's existing patterns)

**Interfaces:**
- Consumes: `mesh_import::load_scene`, `EditorDocument::from_unsaved_model` (Task 5).
- Produces:
  - `MenuAction::ImportModel`; File menu order: New, Open, **Import 3D Model…**, Save, Save As, Quit.
  - `PendingDestructiveAction::Import(AuthoredModel)` (derives stay `Clone, Debug, Eq, PartialEq` — `AuthoredModel` already derives all four).
  - `ShellState::complete_destructive` arm: `Import(model) => { self.document = EditorDocument::from_unsaved_model(model); self.file_error = None; }`
  - `DepthSpriteApp` field `import_dialog: Option<ImportDialogState>` (type arrives in Task 7; this task stores the loaded scene in a placeholder-free way by deferring the field to Task 7 — here, `handle_menu_action` routes `ImportModel` to a `start_import(path)` method that Task 7 completes; in this task `start_import` loads the scene and stores it in the new field `pending_import_scene: Option<(mesh_import::TriangleScene, String)>` which Task 7 replaces wholesale with the dialog state).
  - `fn pick_import_path() -> Option<PathBuf>` using `rfd::FileDialog::new().add_filter("glTF", &["gltf", "glb"]).pick_file()`.

- [ ] **Step 1: Failing tests**

Append to `crates/desktop-app/tests/menu.rs` (match its existing style for asserting menu contents):

```rust
#[test]
fn file_menu_offers_import_between_open_and_save() {
    let labels: Vec<&str> = menu_items(MenuGroup::File).iter().map(|item| item.label).collect();
    assert_eq!(labels, vec!["New", "Open", "Import 3D Model…", "Save", "Save As", "Quit"]);
}
```

Append to `crates/desktop-app/tests/application.rs`:

```rust
#[test]
fn completing_an_import_replaces_the_document_with_a_dirty_untitled_one() {
    let bounds = Bounds::new(4, 4, 4).unwrap();
    let model = AuthoredModel::with_empty_chart(bounds, CanonicalView::Front).unwrap();
    let mut shell = ShellState::new(EditorDocument::new(
        Bounds::new(8, 8, 8).unwrap(),
        CanonicalView::Front,
    ));
    shell.request_destructive(PendingDestructiveAction::Import(model));
    // The starting document is clean, so the action completes immediately.
    assert!(shell.pending_destructive_action().is_none());
    assert_eq!(shell.document().bounds(), bounds);
    assert!(shell.document().path().is_none());
    assert!(shell.document().is_dirty());
}

#[test]
fn import_over_a_dirty_document_waits_for_the_unsaved_prompt() {
    let bounds = Bounds::new(4, 4, 4).unwrap();
    let model = AuthoredModel::with_empty_chart(bounds, CanonicalView::Front).unwrap();
    let mut shell = {
        let mut document = EditorDocument::new(Bounds::new(8, 8, 8).unwrap(), CanonicalView::Front);
        document.set_current_rgb([10, 20, 30]);
        document.begin_stroke().unwrap();
        document.pencil_pixel(CanonicalView::Front, 0, 0).unwrap();
        document.finish_stroke().unwrap();
        assert!(document.is_dirty());
        ShellState::new(document)
    };
    shell.request_destructive(PendingDestructiveAction::Import(model));
    assert!(shell.pending_destructive_action().is_some(), "must prompt first");
    shell.resolve_unsaved(UnsavedChoice::Discard, None);
    assert_eq!(shell.document().bounds(), bounds);
}
```

(The dirtying idiom — set a color, stroke one pixel — mirrors this test file's existing dirty-document tests; if the file uses a different exact sequence, reuse that one. The inline `assert!(document.is_dirty())` guards the test's own premise.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p desktop-app --test menu --test application`
Expected: compile errors (`ImportModel`, `Import(..)` variants missing).

- [ ] **Step 3: Implement**

`menu.rs`: add `ImportModel` to `MenuAction`; add `Import(relief_core::AuthoredModel)` to `PendingDestructiveAction`; extend `FILE_ITEMS` to 6 entries with `MenuItem { label: "Import 3D Model…", action: MenuAction::ImportModel }` third.

`app.rs`:
- `complete_destructive`: add arm

```rust
PendingDestructiveAction::Import(model) => {
    self.document = EditorDocument::from_unsaved_model(model);
    self.file_error = None;
}
```

- add to `ShellState` a setter used by the app on load failure (it already has `file_error`; reuse the existing field via a new method):

```rust
pub fn report_file_error(&mut self, message: String) {
    self.file_error = Some(message);
}
```

- `DepthSpriteApp`: add field `pending_import_scene: Option<(mesh_import::TriangleScene, String)>` (initialize `None` in `from_startup_path`); in `handle_menu_action`:

```rust
MenuAction::ImportModel => {
    if let Some(path) = pick_import_path() {
        match mesh_import::load_scene(&path) {
            Ok(scene) => {
                let label = path
                    .file_name()
                    .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
                self.pending_import_scene = Some((scene, label));
            }
            Err(error) => self
                .shell
                .report_file_error(format!("Could not import {}: {error}", path.display())),
        }
    }
}
```

- add beside the other pickers:

```rust
fn pick_import_path() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("glTF", &["gltf", "glb"])
        .pick_file()
}
```

- [ ] **Step 4: Run to verify pass**

The loaded scene must be *used* in this task, not parked: have `ui()` show a minimal working modal when `pending_import_scene` is `Some` — heading "Import 3D Model", a label with the scene's triangle count, and a Cancel button that clears the field. Task 7 replaces this modal wholesale with the real dialog; until then it is real, working behavior (menu → picker → load → modal → cancel), not dead code.

Run: `cargo test -p desktop-app`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/desktop-app Cargo.lock
git commit -m "feat: import menu action, gltf file picking, and import destructive flow"
```

---

### Task 7: Import dialog state and recompute logic

**Files:**
- Create: `crates/desktop-app/src/import_dialog.rs` (state + logic only; `show` UI arrives in Task 8)
- Modify: `crates/desktop-app/src/lib.rs` (add `mod import_dialog;`)
- Modify: `crates/desktop-app/src/app.rs` (replace `pending_import_scene` with `import_dialog: Option<ImportDialogState>`)
- Test: unit tests inside `import_dialog.rs` (matching the crate's `#[cfg(test)] mod tests` style)

**Interfaces:**
- Consumes: `mesh_import::{ImportSettings, SideMode, TriangleScene, box_space_scene, convert, derived_bounds}`, `editor_core::{EditorDocument, OrbitCamera, PreviewCache}`.
- Produces (all `pub(crate)`):
  - `ImportDialogState::new(scene: TriangleScene, file_label: String) -> Self`
  - fields: `scene`, `file_label`, `settings: ImportSettings`, `camera: OrbitCamera`, `zoom_milli: u32` (init `1_000`, same default as `ModelView`), `converted: Result<ConvertedPreview, String>`, `last_settings: Option<ImportSettings>`
  - `ConvertedPreview { document: EditorDocument, preview: PreviewCache }`
  - `fn ensure_converted(&mut self)` — recomputes `converted` iff `settings != last_settings` (runs `convert`, wraps the model via `EditorDocument::from_model(model, None)` for previewing; on error stores the message)
  - `fn orbit_drag(&mut self, dx: f32, dy: f32)` — plain drag: `self.camera.drag(dx, dy)`
  - `fn model_drag(&mut self, dx: f32, dy: f32)` — Ctrl+drag: rotate `settings.rotation` about the camera's down axis by `dx` and right axis by `dy`, 0.25°/point (`DRAG_MILLIDEGREES_PER_POINT / 1000`, the same feel as camera orbit), then re-orthonormalize
  - `fn snap_rotation(&mut self)` — nearest signed-permutation matrix (per-row argmax with used-column tracking; if the determinant is −1, negate the row whose winning component had the smallest magnitude — the cheapest correction to a proper rotation)
  - `fn apply_preset(&mut self, preset: OrientationPreset)` where `OrientationPreset { ZUpToYUp, FlipX, FlipY, FlipZ }` — left-multiplies fixed rotations (−90° about X for Z-up→Y-up; 180° about the named axis for flips)
  - `fn conversion_count(&self) -> u64` (test observation counter incremented by every real `convert` run)

- [ ] **Step 1: Failing unit tests** (inside `import_dialog.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use mesh_import::{Material, Triangle, TriangleScene};

    fn quad_scene() -> TriangleScene {
        let tri = |a: [f32; 3], b: [f32; 3], c: [f32; 3]| Triangle {
            positions: [a, b, c],
            normals: [[0.0, 0.0, -1.0]; 3],
            uvs: [[0.0, 0.0]; 3],
            colors: [[1.0, 1.0, 1.0, 1.0]; 3],
            material: 0,
        };
        TriangleScene {
            triangles: vec![
                tri([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.5]),
                tri([0.0, 0.0, 0.0], [1.0, 1.0, 0.5], [0.0, 1.0, 0.5]),
            ],
            materials: vec![Material {
                base_color_factor: [1.0, 1.0, 1.0, 1.0],
                base_color_texture: None,
                alpha_cutoff: None,
            }],
        }
    }

    #[test]
    fn conversion_runs_once_per_settings_change() {
        let mut state = ImportDialogState::new(quad_scene(), "quad.glb".into());
        state.ensure_converted();
        state.ensure_converted();
        assert_eq!(state.conversion_count(), 1, "unchanged settings must not reconvert");

        state.settings.longest_axis_pixels = 32;
        state.ensure_converted();
        assert_eq!(state.conversion_count(), 2);

        state.orbit_drag(10.0, 5.0);
        state.ensure_converted();
        assert_eq!(state.conversion_count(), 2, "camera orbit never reconverts");

        state.model_drag(10.0, 0.0);
        state.ensure_converted();
        assert_eq!(state.conversion_count(), 3, "model rotation reconverts");
    }

    #[test]
    fn model_drag_keeps_rotation_orthonormal() {
        let mut state = ImportDialogState::new(quad_scene(), "quad.glb".into());
        for _ in 0..500 {
            state.model_drag(7.3, -3.1);
        }
        let r = state.settings.rotation;
        for i in 0..3 {
            let len = (0..3).map(|j| r[i][j] * r[i][j]).sum::<f32>().sqrt();
            assert!((len - 1.0).abs() < 1e-3, "row {i} length {len}");
            for k in (i + 1)..3 {
                let dot: f32 = (0..3).map(|j| r[i][j] * r[k][j]).sum();
                assert!(dot.abs() < 1e-3, "rows {i},{k} not orthogonal: {dot}");
            }
        }
    }

    #[test]
    fn snap_lands_on_a_signed_permutation_with_determinant_one() {
        let mut state = ImportDialogState::new(quad_scene(), "quad.glb".into());
        state.model_drag(40.0, 25.0); // ~10 and ~6 degrees: near identity
        state.snap_rotation();
        let r = state.settings.rotation;
        let mut ones = 0;
        for row in r {
            for value in row {
                assert!(
                    value == 0.0 || value == 1.0 || value == -1.0,
                    "snap must produce a signed permutation, got {value}"
                );
                if value != 0.0 {
                    ones += 1;
                }
            }
        }
        assert_eq!(ones, 3);
        let det = r[0][0] * (r[1][1] * r[2][2] - r[1][2] * r[2][1])
            - r[0][1] * (r[1][0] * r[2][2] - r[1][2] * r[2][0])
            + r[0][2] * (r[1][0] * r[2][1] - r[1][1] * r[2][0]);
        assert_eq!(det, 1.0);
        // Near identity snaps TO identity.
        assert_eq!(r, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    }

    #[test]
    fn flip_presets_are_involutions_and_z_up_preset_rotates_about_x() {
        let mut state = ImportDialogState::new(quad_scene(), "quad.glb".into());
        let before = state.settings.rotation;
        state.apply_preset(OrientationPreset::FlipY);
        state.apply_preset(OrientationPreset::FlipY);
        for i in 0..3 {
            for j in 0..3 {
                assert!((state.settings.rotation[i][j] - before[i][j]).abs() < 1e-6);
            }
        }
        state.apply_preset(OrientationPreset::ZUpToYUp);
        // -90 about X maps +z to -y (box up).
        let r = state.settings.rotation;
        let mapped_z = [r[0][2], r[1][2], r[2][2]];
        assert!((mapped_z[1] + 1.0).abs() < 1e-6, "+z must map to -y, got {mapped_z:?}");
    }

    #[test]
    fn conversion_error_is_stored_not_panicked() {
        let mut state = ImportDialogState::new(quad_scene(), "quad.glb".into());
        state.settings.longest_axis_pixels = 0;
        state.ensure_converted();
        assert!(state.converted.is_err());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p desktop-app import_dialog`
Expected: compile error — module doesn't exist.

- [ ] **Step 3: Implement the state half of `import_dialog.rs`**

```rust
use editor_core::{EditorDocument, OrbitCamera, PreviewCache};
use mesh_import::{ImportSettings, TriangleScene, convert};

const MODEL_DRAG_DEGREES_PER_POINT: f32 = 0.25; // same feel as camera orbit

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OrientationPreset {
    ZUpToYUp,
    FlipX,
    FlipY,
    FlipZ,
}

pub(crate) struct ConvertedPreview {
    pub document: EditorDocument,
    pub preview: PreviewCache,
}

pub(crate) struct ImportDialogState {
    pub scene: TriangleScene,
    pub file_label: String,
    pub settings: ImportSettings,
    pub camera: OrbitCamera,
    pub zoom_milli: u32,
    pub converted: Result<ConvertedPreview, String>,
    last_settings: Option<ImportSettings>,
    conversions: u64,
}

impl ImportDialogState {
    pub fn new(scene: TriangleScene, file_label: String) -> Self {
        Self {
            scene,
            file_label,
            settings: ImportSettings::default(),
            camera: OrbitCamera::default(),
            zoom_milli: 1_000,
            converted: Err(String::from("not yet converted")),
            last_settings: None,
            conversions: 0,
        }
    }

    pub fn ensure_converted(&mut self) {
        if self.last_settings.as_ref() == Some(&self.settings) {
            return;
        }
        self.last_settings = Some(self.settings.clone());
        self.conversions += 1;
        self.converted = match convert(&self.scene, &self.settings) {
            Ok(model) => Ok(ConvertedPreview {
                document: EditorDocument::from_model(model, None),
                preview: PreviewCache::default(),
            }),
            Err(error) => Err(error.to_string()),
        };
    }

    pub fn conversion_count(&self) -> u64 {
        self.conversions
    }

    pub fn orbit_drag(&mut self, dx: f32, dy: f32) {
        self.camera.drag(dx, dy);
    }

    pub fn model_drag(&mut self, dx: f32, dy: f32) {
        let basis = self.camera.basis_f32();
        let yaw = rotation_about(basis[1], dx * MODEL_DRAG_DEGREES_PER_POINT.to_radians());
        let pitch = rotation_about(basis[0], dy * MODEL_DRAG_DEGREES_PER_POINT.to_radians());
        self.settings.rotation =
            orthonormalized(multiply(pitch, multiply(yaw, self.settings.rotation)));
    }

    pub fn snap_rotation(&mut self) {
        let r = self.settings.rotation;
        let mut snapped = [[0.0f32; 3]; 3];
        let mut used = [false; 3];
        // Rows in order of their strongest component (most confident first)
        // so the strongest alignments win their axes.
        let mut order: Vec<usize> = (0..3).collect();
        let strength = |row: [f32; 3]| row.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        order.sort_by(|&a, &b| strength(r[b]).partial_cmp(&strength(r[a])).unwrap());
        let mut weakest = (order[2], 0usize, f32::INFINITY);
        for &i in &order {
            let (mut best, mut best_abs) = (usize::MAX, -1.0f32);
            for j in 0..3 {
                if !used[j] && r[i][j].abs() > best_abs {
                    best = j;
                    best_abs = r[i][j].abs();
                }
            }
            used[best] = true;
            snapped[i][best] = r[i][best].signum();
            if best_abs < weakest.2 {
                weakest = (i, best, best_abs);
            }
        }
        let det = snapped[0][0] * (snapped[1][1] * snapped[2][2] - snapped[1][2] * snapped[2][1])
            - snapped[0][1] * (snapped[1][0] * snapped[2][2] - snapped[1][2] * snapped[2][0])
            + snapped[0][2] * (snapped[1][0] * snapped[2][1] - snapped[1][1] * snapped[2][0]);
        if det < 0.0 {
            // A reflection is not an orientation; negating the least
            // confident row is the smallest change restoring det = +1.
            snapped[weakest.0][weakest.1] = -snapped[weakest.0][weakest.1];
        }
        self.settings.rotation = snapped;
    }

    pub fn apply_preset(&mut self, preset: OrientationPreset) {
        let rotation = match preset {
            // -90 about X: +y -> +z, +z -> -y (glTF Y-up from Z-up sources).
            OrientationPreset::ZUpToYUp => {
                [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]]
            }
            OrientationPreset::FlipX => [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]],
            OrientationPreset::FlipY => [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]],
            OrientationPreset::FlipZ => [[-1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]],
        };
        self.settings.rotation = multiply(rotation, self.settings.rotation);
    }
}

fn multiply(a: [[f32; 3]; 3], b: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut out = [[0.0f32; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}

/// Rodrigues rotation matrix about a unit axis.
fn rotation_about(axis: [f32; 3], angle: f32) -> [[f32; 3]; 3] {
    let (sin, cos) = angle.sin_cos();
    let one_minus = 1.0 - cos;
    let [x, y, z] = axis;
    [
        [
            cos + x * x * one_minus,
            x * y * one_minus - z * sin,
            x * z * one_minus + y * sin,
        ],
        [
            y * x * one_minus + z * sin,
            cos + y * y * one_minus,
            y * z * one_minus - x * sin,
        ],
        [
            z * x * one_minus - y * sin,
            z * y * one_minus + x * sin,
            cos + z * z * one_minus,
        ],
    ]
}

/// Gram-Schmidt on rows: keeps incremental drag rotations from drifting
/// away from orthonormality.
fn orthonormalized(m: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let normalize = |v: [f32; 3]| {
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        [v[0] / len, v[1] / len, v[2] / len]
    };
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let r0 = normalize(m[0]);
    let p = dot(m[1], r0);
    let r1 = normalize([m[1][0] - p * r0[0], m[1][1] - p * r0[1], m[1][2] - p * r0[2]]);
    let r2 = [
        r0[1] * r1[2] - r0[2] * r1[1],
        r0[2] * r1[0] - r0[0] * r1[2],
        r0[0] * r1[1] - r0[1] * r1[0],
    ];
    [r0, r1, r2]
}
```

Also in `app.rs`: replace `pending_import_scene` (and its stub modal from Task 6) with `import_dialog: Option<ImportDialogState>`; `MenuAction::ImportModel` success arm becomes `self.import_dialog = Some(ImportDialogState::new(scene, label));`. Add `use crate::import_dialog::ImportDialogState;`. The dialog is not rendered yet — add a temporary minimal modal in `ui()` that will be replaced in Task 8: it calls `state.ensure_converted()` and offers Cancel (sets `self.import_dialog = None`). It must compile and behave; Task 8 replaces its body.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p desktop-app`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/desktop-app
git commit -m "feat: import dialog state with recompute, arcball, snap, and presets"
```

---

### Task 8: Import dialog UI — synced viewports, settings, accept/cancel

**Files:**
- Modify: `crates/desktop-app/src/import_dialog.rs` (add the `show` UI half)
- Modify: `crates/desktop-app/src/model_view.rs` (make `presentation_scale` and `color_image` `pub(crate)`)
- Modify: `crates/desktop-app/src/app.rs` (render the dialog; route its outcome)
- Test: unit tests in `import_dialog.rs` + append to `crates/desktop-app/tests/application.rs`

**Interfaces:**
- Consumes: Task 7 state; `mesh_import::{box_space_scene, light_direction, rasterize, Lighting, View}`; `model_view::{presentation_scale, color_image}`; `PendingDestructiveAction::Import`.
- Produces:
  - `enum ImportDialogOutcome { KeepOpen, Cancel, Import(relief_core::AuthoredModel) }`
  - `ImportDialogState::show(&mut self, context: &egui::Context) -> ImportDialogOutcome`
  - app wiring: `DepthSpriteApp::ui` renders the dialog when `import_dialog.is_some()` (suppressed while a file-error or unsaved modal is up); `Cancel` drops the state; `Import(model)` drops the state then `self.shell.request_destructive(PendingDestructiveAction::Import(model))`.

- [ ] **Step 1: Failing tests**

In `import_dialog.rs` tests (extend the Task 7 module):

```rust
#[test]
fn import_outcome_carries_the_converted_model_and_cancel_carries_nothing() {
    let mut state = ImportDialogState::new(quad_scene(), "quad.glb".into());
    state.ensure_converted();
    let model = state.take_converted_model().expect("conversion succeeded");
    assert_eq!(model.bounds().width(), 63);

    let mut broken = ImportDialogState::new(quad_scene(), "quad.glb".into());
    broken.settings.longest_axis_pixels = 0;
    broken.ensure_converted();
    assert!(broken.take_converted_model().is_none(), "no model while conversion errors");
}
```

Append to `crates/desktop-app/tests/application.rs` a frame-driving test in the file's existing `run_frame` idiom:

```rust
#[test]
fn open_import_dialog_renders_and_cancel_closes_it_without_touching_the_document() {
    let mut app = DepthSpriteApp::from_startup_path(None);
    let revision_before = app.shell().document().revision();
    app.open_import_dialog_for_test(test_quad_scene(), "quad.glb".into());
    // One frame renders the modal.
    run_app_frame(&mut app);
    assert!(app.import_dialog_open_for_test());
    app.cancel_import_dialog_for_test();
    run_app_frame(&mut app);
    assert!(!app.import_dialog_open_for_test());
    assert_eq!(app.shell().document().revision(), revision_before);
}
```

Follow the exact frame-driving helper this test file already uses (`run_frame`-style with `egui::Context::run_ui` / the app's test constructor); add small `#[cfg(test)]` accessors on `DepthSpriteApp` (`open_import_dialog_for_test`, `import_dialog_open_for_test`, `cancel_import_dialog_for_test`) in the same style as its existing test observations — driving rfd file pickers headlessly is not possible, so tests inject the scene directly. `test_quad_scene` is this builder (same geometry as `quad_scene` in `import_dialog.rs`'s unit tests):

```rust
fn test_quad_scene() -> mesh_import::TriangleScene {
    let tri = |a: [f32; 3], b: [f32; 3], c: [f32; 3]| mesh_import::Triangle {
        positions: [a, b, c],
        normals: [[0.0, 0.0, -1.0]; 3],
        uvs: [[0.0, 0.0]; 3],
        colors: [[1.0, 1.0, 1.0, 1.0]; 3],
        material: 0,
    };
    mesh_import::TriangleScene {
        triangles: vec![
            tri([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.5]),
            tri([0.0, 0.0, 0.0], [1.0, 1.0, 0.5], [0.0, 1.0, 0.5]),
        ],
        materials: vec![mesh_import::Material {
            base_color_factor: [1.0, 1.0, 1.0, 1.0],
            base_color_texture: None,
            alpha_cutoff: None,
        }],
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p desktop-app`
Expected: compile errors (`take_converted_model`, test accessors missing).

- [ ] **Step 3: Implement the UI half**

Add to `ImportDialogState`:

```rust
pub fn take_converted_model(&mut self) -> Option<relief_core::AuthoredModel> {
    match &self.converted {
        Ok(converted) => Some(converted.document.to_model()),
        Err(_) => None,
    }
}
```

`show` (complete structure; visual constants are px sizes, chosen so two 63·4-px viewports and the settings rows fit a 1280×800 window):

```rust
pub(crate) enum ImportDialogOutcome {
    KeepOpen,
    Cancel,
    Import(relief_core::AuthoredModel),
}

const VIEWPORT_SIZE: f32 = 360.0;

impl ImportDialogState {
    pub fn show(&mut self, context: &eframe::egui::Context) -> ImportDialogOutcome {
        use eframe::egui;
        self.ensure_converted();
        let mut outcome = ImportDialogOutcome::KeepOpen;
        egui::Modal::new("import-3d-model-modal".into()).show(context, |ui| {
            ui.heading(format!("Import 3D Model — {}", self.file_label));
            ui.horizontal(|ui| {
                let mesh_rect = allocate_viewport(ui, "import-mesh-viewport");
                self.handle_viewport_input(ui, mesh_rect, true);
                self.draw_mesh_viewport(ui, mesh_rect);
                let converted_rect = allocate_viewport(ui, "import-converted-viewport");
                self.handle_viewport_input(ui, converted_rect, false);
                self.draw_converted_viewport(ui, converted_rect);
            });
            ui.separator();
            self.show_settings(ui);
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    outcome = ImportDialogOutcome::Cancel;
                }
                let importable = self.converted.is_ok();
                if ui.add_enabled(importable, egui::Button::new("Import")).clicked()
                    && let Some(model) = self.take_converted_model()
                {
                    outcome = ImportDialogOutcome::Import(model);
                }
            });
        });
        outcome
    }
}
```

with these private methods (write them fully; the key behaviors):

- `allocate_viewport`: `ui.allocate_exact_size(vec2(VIEWPORT_SIZE, VIEWPORT_SIZE), Sense::drag())`-based rect helper returning the rect (interaction handled separately so both viewports share code).
- `handle_viewport_input(ui, rect, is_mesh_viewport)`: `ui.interact(rect, id, Sense::drag())`; on primary drag: if `ui.input(|i| i.modifiers.ctrl)` **and** `is_mesh_viewport` → `self.model_drag(delta.x, delta.y)` else → `self.orbit_drag(delta.x, delta.y)`; when hovered, consume `smooth_scroll_delta.y` exactly like `ModelView::zoom` (copy the exponent/clamp constants by calling a shared `pub(crate) fn zoom_step(zoom_milli: u32, wheel_delta: f32) -> u32` extracted from `model_view.rs` — extract, don't duplicate).
- `draw_converted_viewport`: on `Ok(converted)` call `converted.preview.frame(&converted.document, self.camera)`; upload via the now-`pub(crate)` `color_image`, scale with `presentation_scale(native, rect.size(), self.zoom_milli)`, draw centered NEAREST like `ModelView::show` does. On `Err(message)` paint the message centered in `LIGHT_RED` monospace (same idiom as the main app's "Preview unavailable" branch).
- `draw_mesh_viewport`: compute the box-space scene once per settings change (cache `(TriangleScene, Bounds)` beside `last_settings` in `ensure_converted` via `mesh_import::box_space_scene`); rasterize with `View { origin: box_center - (rect_w/2/scale)·right - (rect_h/2/scale)·down, right, down, forward (rows of self.camera.basis_f32()), scale: pixels_per_model_px as f32, width: rect_w, height: rect_h }` where `pixels_per_model_px` = the same `presentation_scale` result used by the converted viewport when conversion succeeded, else the plain fit scale for the box diagonal (both viewports must show the model box at the same screen scale — that is the sync the spec demands); light = `Lighting { direction: light_direction(self.settings.light_azimuth_degrees, self.settings.light_elevation_degrees), ambient: self.settings.ambient }`; upload as a texture (LINEAR is fine here, the raster is already at screen resolution) and paint into `rect`. Cache the rasterized image keyed by `(camera, zoom_milli, settings-bits, rect size)` — f32 keys via `to_bits` — so an idle frame re-rasterizes nothing.
- `show_settings(ui)`: four `ui.horizontal`/`egui::Grid` groups:
  1. **Orientation**: buttons `Snap to 90°`, `Z-up → Y-up`, `Flip X`, `Flip Y`, `Flip Z` calling `snap_rotation`/`apply_preset`; a label "Ctrl+drag the mesh to rotate the model".
  2. **Sides**: three rows (Front/Back, Left/Right, Top/Bottom). Each side is an `egui::ComboBox` over the legal modes for that side right now: `Capture` and `Off` always; `From opposite` / `From opposite, mirrored` only when `self.settings.side_modes.get(view.opposite()) == SideMode::Capture`. Apply via `side_modes.set(view, mode)` — its `Err` cannot occur for options the UI offered; `expect` with that reasoning.
  3. **Bounds**: `egui::Slider::new(&mut self.settings.longest_axis_pixels, 1..=63).text("longest axis")`; readout label from `mesh_import::derived_bounds(&self.scene, self.settings.rotation, self.settings.longest_axis_pixels)` formatted `W {w} × H {h} × D {d}` (on error, the error string).
  4. **Lighting**: sliders azimuth `−180.0..=180.0`, elevation `−90.0..=90.0`, ambient `0.0..=1.0`.

In `app.rs` `ui()`, replace the Task 7 stub: render the dialog when no file-error/unsaved modal is showing:

```rust
if self.shell.file_error().is_none() && self.shell.pending_destructive_action().is_none()
    && let Some(dialog) = &mut self.import_dialog
{
    match dialog.show(&context) {
        ImportDialogOutcome::KeepOpen => {}
        ImportDialogOutcome::Cancel => self.import_dialog = None,
        ImportDialogOutcome::Import(model) => {
            self.import_dialog = None;
            self.shell
                .request_destructive(PendingDestructiveAction::Import(model));
        }
    }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p desktop-app`
Expected: PASS.

- [ ] **Step 5: Manual verification (real app, real file)**

```bash
cargo run -p desktop-app
```

File → Import 3D Model… → pick `crates/mesh-import/tests/fixtures/earth.glb`. Verify against the spec: both viewports render; plain drag orbits both together; wheel zooms both together; Ctrl+drag in the left viewport rotates the model and the right viewport re-captures; side/bounds/lighting controls update the conversion; Import lands an untitled dirty document in the editor; a later File → New prompts about unsaved changes. Report what was observed — this step is evidence, not ceremony.

- [ ] **Step 6: Commit**

```bash
git add crates/desktop-app
git commit -m "feat: import dialog with camera-synced mesh and conversion viewports"
```

---

### Task 9: Documentation and full-workspace verification

**Files:**
- Modify: `README.md`
- Modify: `docs/specs/depthsprite-app.md`

**Interfaces:** none — prose only.

- [ ] **Step 1: README**

Add to the "Authoring workflow" section, after the File menu sentence:

> **File → Import 3D Model…** converts a glTF/GLB file into a new model. The
> import dialog shows the source mesh and the converted result side by side
> with a shared orbit camera (drag to orbit, wheel to zoom). Ctrl+drag the
> mesh to rotate the model relative to the capture sides; buttons snap the
> rotation to 90° or apply axis presets. Each side pair can capture both
> sides, supply one side from the other (optionally mirrored), or omit
> sides. A slider sets the longest model axis in pixels, and the baked
> light's azimuth, elevation, and ambient level are adjustable. Importing
> replaces the current document (prompting for unsaved changes) with an
> untitled model.

- [ ] **Step 2: App spec**

Read `docs/specs/depthsprite-app.md` and add a matching "3D model import" section in its established contract style (declarative present tense, exact widget behaviors), covering: menu entry and file filter; load-failure message box; the modal's two viewports and shared camera; Ctrl+drag semantics; the four settings groups with ranges and defaults (longest axis 63, azimuth −35°, elevation 35°, ambient 0.25, all sides Capture); pair-mode constraints; conversion rules by reference to `docs/superpowers/specs/2026-07-17-model-import-design.md`; Import/Cancel document semantics.

- [ ] **Step 3: Full workspace verification**

Run: `cargo test --workspace`
Expected: all green. Run `cargo clippy --workspace --all-targets` and fix any new warnings.

- [ ] **Step 4: Commit**

```bash
git add README.md docs/specs/depthsprite-app.md
git commit -m "docs: document 3D model import in README and app spec"
```
