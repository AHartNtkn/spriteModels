use crate::{Texture, TriangleScene};

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
    /// Geometric face normal of the winning triangle (normalized cross
    /// product of its edges, in the scene's own coordinate space — not
    /// rotated into screen space). `[0.0; 3]` where uncovered. Surface
    /// ownership (capture.rs) uses this to score how head-on each capture
    /// side observes a hit; it is independent of the interpolated,
    /// two-sided-flipped vertex normal used for shading below.
    pub face_normal: Vec<[f32; 3]>,
    /// Index into the scene's triangle list of the triangle that won the
    /// depth test at each texel; `u32::MAX` where `depth` is not finite.
    pub triangle: Vec<u32>,
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

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Geometric face normal of a triangle from its vertex positions alone,
/// independent of any view. Cross order is `(p2 - p0) x (p1 - p0)` — the
/// mirror of the standard `(p1-p0) x (p2-p0)` used by the glTF-space face
/// normal fallback in scene.rs. The two crates' winding conventions differ
/// because this rasterizer's screen basis uses `down` (not `up`) as its
/// second axis; this order is pinned by the axis-aligned CCW quad test
/// (`face_normal_matches_the_triangles_winding`), which fixes the sign for
/// this crate's own triangles rather than re-deriving it abstractly.
fn triangle_face_normal(positions: [[f32; 3]; 3]) -> [f32; 3] {
    let e1 = sub(positions[1], positions[0]);
    let e2 = sub(positions[2], positions[0]);
    let n = cross(e2, e1);
    let len = dot(n, n).sqrt();
    if len > 0.0 {
        [n[0] / len, n[1] / len, n[2] / len]
    } else {
        [0.0, 0.0, 0.0]
    }
}

pub fn rasterize(scene: &TriangleScene, view: &View, lighting: &Lighting) -> Raster {
    let width = view.width as usize;
    let height = view.height as usize;
    let mut depth = vec![f32::INFINITY; width * height];
    let mut color = vec![[0u8; 4]; width * height];
    let mut face_normal = vec![[0.0f32; 3]; width * height];
    let mut triangle = vec![u32::MAX; width * height];

    for (tri_idx, tri) in scene.triangles.iter().enumerate() {
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
        let mut area = (s1[0] - s0[0]) * (s2[1] - s0[1]) - (s1[1] - s0[1]) * (s2[0] - s0[0]);
        // Two-sided: a negative area is a back-facing winding, sampled by
        // negating the barycentric weights rather than culling.
        let flip = if area < 0.0 { -1.0 } else { 1.0 };
        area *= flip;
        let inv_area = 1.0 / area;
        // Barycentric weights are only computable when the area reciprocal is finite.
        // A zero or subnormal-small area has no representable parameterization in f32,
        // and a non-finite reciprocal would let 0.0 × inf = NaN weights slip past the
        // sign checks (NaN compares false) and silently write NaN depth.
        if !inv_area.is_finite() {
            continue;
        }
        // Computed once per triangle, not per pixel.
        let tri_normal = triangle_face_normal(tri.positions);

        let min_x = s0[0].min(s1[0]).min(s2[0]).floor().max(0.0) as usize;
        let max_x = (s0[0].max(s1[0]).max(s2[0]).ceil().max(0.0) as usize).min(width);
        let min_y = s0[1].min(s1[1]).min(s2[1]).floor().max(0.0) as usize;
        let max_y = (s0[1].max(s1[1]).max(s2[1]).ceil().max(0.0) as usize).min(height);
        let material = &scene.materials[tri.material];

        for py in min_y..max_y {
            let y = py as f32 + 0.5;
            for px in min_x..max_x {
                let x = px as f32 + 0.5;
                let w0 = flip * ((s1[0] - x) * (s2[1] - y) - (s1[1] - y) * (s2[0] - x)) * inv_area;
                let w1 = flip * ((s2[0] - x) * (s0[1] - y) - (s2[1] - y) * (s0[0] - x)) * inv_area;
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
                face_normal[index] = tri_normal;
                triangle[index] = tri_idx as u32;

                let mut normal = interpolate3(tri.normals);
                let len = dot(normal, normal).sqrt();
                // Any positive length normalizes to a unit vector. Zero-length normal has no
                // direction, so flip test and lambert both evaluate to 0, leaving ambient-only shading.
                if len > 0.0 {
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
        face_normal,
        triangle,
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
    let (t00, t10, t01, t11) = (
        texel(x0, y0),
        texel(x0 + 1, y0),
        texel(x0, y0 + 1),
        texel(x0 + 1, y0 + 1),
    );
    let mut out = [0.0f32; 4];
    for channel in 0..4 {
        let top = t00[channel] * (1.0 - fx) + t10[channel] * fx;
        let bottom = t01[channel] * (1.0 - fx) + t11[channel] * fx;
        out[channel] = top * (1.0 - fy) + bottom * fy;
    }
    out
}
