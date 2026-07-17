use std::path::Path;
use std::sync::Arc;

use crate::ImportError;

#[derive(Clone, Debug)]
pub struct TriangleScene {
    pub triangles: Vec<Triangle>,
    pub materials: Vec<Material>,
}

#[derive(Clone, Copy, Debug)]
pub struct Triangle {
    pub positions: [[f32; 3]; 3],
    pub normals: [[f32; 3]; 3],
    pub uvs: [[f32; 2]; 3],
    pub colors: [[f32; 4]; 3],
    pub material: usize,
}

#[derive(Clone, Debug)]
pub struct Material {
    pub base_color_factor: [f32; 4],
    /// `Arc`-wrapped so cloning a `TriangleScene` (as `box_space_scene` does
    /// every settings change) never copies texel buffers.
    pub base_color_texture: Option<Arc<Texture>>,
    /// `Some(cutoff)` for glTF `alphaMode: MASK`; `None` renders opaque
    /// (OPAQUE and BLEND — the depthsprite format has no translucency).
    pub alpha_cutoff: Option<f32>,
}

#[derive(Clone, Debug)]
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
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            &buffers,
            default_material,
            &mut triangles,
        )?;
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
        Arc::new(decode_image(image))
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
            let channels = if image.format == Format::R32G32B32FLOAT {
                3
            } else {
                4
            };
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
) -> Result<(), ImportError> {
    let local = node.transform().matrix();
    let world = matrix_multiply(parent, local);
    if let Some(mesh) = node.mesh() {
        for primitive in mesh.primitives() {
            if !matches!(
                primitive.mode(),
                gltf::mesh::Mode::Triangles
                    | gltf::mesh::Mode::TriangleStrip
                    | gltf::mesh::Mode::TriangleFan
            ) {
                // Points, lines, line loops, and line strips are not
                // surfaces; they contribute no triangle geometry.
                continue;
            }
            collect_primitive(&primitive, world, buffers, default_material, triangles)?;
        }
    }
    for child in node.children() {
        collect_node(&child, world, buffers, default_material, triangles)?;
    }
    Ok(())
}

fn collect_primitive(
    primitive: &gltf::mesh::Primitive<'_>,
    world: [[f32; 4]; 4],
    buffers: &[gltf::buffer::Data],
    default_material: usize,
    triangles: &mut Vec<Triangle>,
) -> Result<(), ImportError> {
    let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()].0[..]));
    let positions: Vec<[f32; 3]> = match reader.read_positions() {
        Some(positions) => positions.collect(),
        None => return Err(ImportError::MissingPositions),
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

    for idx in faces(primitive.mode(), &indices) {
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
    Ok(())
}

/// Expands a primitive's index buffer into a list of triangle-index
/// triples, per glTF's primitive-mode conventions.
///
/// `Triangles`: plain chunks of 3. `TriangleStrip`: triangle `i` uses
/// vertices `(i, i+1, i+2)`, with the first two swapped on odd `i` — the
/// glTF strip convention that keeps every triangle's winding facing the
/// same way (consecutive strip triangles share an edge, so taking raw
/// consecutive triples would alternate winding). `TriangleFan`: triangle
/// `i` (1-indexed) uses `(0, i, i+1)`, which is already winding-consistent
/// since every triangle shares the fixed vertex 0 in the same position.
fn faces(mode: gltf::mesh::Mode, indices: &[u32]) -> Vec<[usize; 3]> {
    use gltf::mesh::Mode;
    let at = |i: u32| indices[i as usize] as usize;
    match mode {
        Mode::Triangles => indices
            .chunks_exact(3)
            .map(|face| [face[0] as usize, face[1] as usize, face[2] as usize])
            .collect(),
        Mode::TriangleStrip => {
            let count = indices.len();
            (0..count.saturating_sub(2))
                .map(|i| {
                    let i = i as u32;
                    if i.is_multiple_of(2) {
                        [at(i), at(i + 1), at(i + 2)]
                    } else {
                        [at(i + 1), at(i), at(i + 2)]
                    }
                })
                .collect()
        }
        Mode::TriangleFan => {
            let count = indices.len();
            (1..count.saturating_sub(1))
                .map(|i| {
                    let i = i as u32;
                    [at(0), at(i), at(i + 1)]
                })
                .collect()
        }
        // Only triangle-family modes reach `collect_primitive`.
        _ => Vec::new(),
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
    // The determinant scales with the cube of the transform's scale, so an
    // absolute-magnitude threshold misclassifies small-but-valid uniform
    // scales as singular. Any nonzero finite determinant yields a usable
    // inverse-transpose, and the transformed normal is renormalized
    // afterward, so no magnitude threshold is needed — only exact
    // singularity (det == 0) or a non-finite determinant (overflow/NaN from
    // the transform itself) disqualify the matrix.
    if det == 0.0 || !det.is_finite() {
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
        if (r + c).is_multiple_of(2) {
            minor
        } else {
            -minor
        }
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
