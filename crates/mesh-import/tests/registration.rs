//! Cross-side capture registration: does a point captured by one canonical
//! view land at the same 3D position when reconstructed and re-projected
//! into a different view that also sees it? This is a property of
//! `rasterize` plus the `CanonicalView::frame` capture geometry alone —
//! ownership/conversion layers sit above and would only obscure it, so the
//! test drives `rasterize` directly with the exact `View`s `convert` builds.

use std::f64::consts::{FRAC_1_SQRT_2, PI};

use mesh_import::{Lighting, Material, Triangle, TriangleScene, View, rasterize};
use relief_core::{Bounds, CanonicalView};

/// UV-sphere tessellation density. Rings and segments share the same
/// angular step (pi/128 == 2*pi/256), so the chord-sagitta bound derived
/// from it applies uniformly to both sampling directions.
const RINGS: usize = 128;
const SEGMENTS: usize = 256;
const RADIUS: f64 = 31.5;
const CENTER: [f64; 3] = [31.5, 31.5, 31.5];

/// Widest possible gap between a tessellation triangle's flat face and the
/// true sphere: the sagitta of a chord spanning the tessellation's angular
/// step, `r * (1 - cos(step))`. Named so the tolerance derivation below can
/// reference it directly instead of re-deriving it inline.
fn chord_bound() -> f64 {
    RADIUS * (1.0 - (PI / RINGS as f64).cos())
}

/// Cosine cutoff for "comfortably away from the silhouette of either view":
/// texels are only checked where the analytic normal satisfies
/// `-n.axis >= CONE_COS` for both views' forward axes. Excludes the
/// silhouette band where a half-texel lateral offset in screen space
/// produces an unbounded change in reconstructed depth, while leaving a
/// wide overlap wedge on a sphere (half-angle `arccos(0.35) ~= 69.5 deg`).
const CONE_COS: f64 = 0.35;

/// Upper bound on the sphere's screen-space depth gradient `|grad(d)| =
/// |tan(theta)|` for any point admitted by `CONE_COS`: `tan(theta) =
/// sin(theta)/cos(theta)` is maximized exactly at `cos(theta) = CONE_COS`.
fn slope_bound() -> f64 {
    (1.0 - CONE_COS * CONE_COS).sqrt() / CONE_COS
}

/// Reconstructing `p` from a Front texel and projecting it into the Top
/// view lands somewhere inside the Top texel containing `p`'s projection,
/// not necessarily at that texel's center; `FRAC_1_SQRT_2` (= sqrt(2)/2) is
/// the farthest that projected point can be from the texel center whose
/// depth we compare against.
const HALF_TEXEL_DIAGONAL: f64 = FRAC_1_SQRT_2;

/// `TOL = slope_bound * half_texel_diagonal + 2 * chord_bound`: the first
/// term is the depth change induced by the worst-case lateral offset
/// between the projected point and the texel center it is compared
/// against, at the steepest admitted surface slope; the second is the
/// tessellation's chord error, counted twice because it perturbs both the
/// source reconstruction and the analytic normal used to admit the texel.
fn tolerance() -> f64 {
    slope_bound() * HALF_TEXEL_DIAGONAL + 2.0 * chord_bound()
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(a: [f64; 3]) -> [f64; 3] {
    scale(a, 1.0 / dot(a, a).sqrt())
}

fn plain_material() -> Material {
    Material {
        base_color_factor: [1.0, 1.0, 1.0, 1.0],
        base_color_texture: None,
        alpha_cutoff: None,
    }
}

/// Analytic UV sphere as a `TriangleScene`, built directly (no fixture):
/// center `(31.5, 31.5, 31.5)`, radius `31.5` — inscribed exactly in a
/// `Bounds::new(63, 63, 63)` box, touching each face at a single point.
fn sphere_scene() -> TriangleScene {
    let position = |i: usize, j: usize| -> [f64; 3] {
        let theta = i as f64 * PI / RINGS as f64;
        let phi = j as f64 * 2.0 * PI / SEGMENTS as f64;
        let (sin_t, cos_t) = theta.sin_cos();
        let (sin_p, cos_p) = phi.sin_cos();
        add(CENTER, scale([sin_t * cos_p, cos_t, sin_t * sin_p], RADIUS))
    };
    let to_tri = |a: [f64; 3], b: [f64; 3], c: [f64; 3]| -> Triangle {
        let as_f32 = |p: [f64; 3]| [p[0] as f32, p[1] as f32, p[2] as f32];
        let normal_at = |p: [f64; 3]| as_f32(normalize(sub(p, CENTER)));
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
            triangles.push(to_tri(p00, p01, p10));
            triangles.push(to_tri(p01, p11, p10));
        }
    }
    TriangleScene {
        triangles,
        materials: vec![plain_material()],
    }
}

fn unlit() -> Lighting {
    Lighting {
        direction: [0.0, 0.0, -1.0],
        ambient: 1.0,
    }
}

/// A capture view's geometry in both the `f64` precision used for this
/// test's own reconstruction/projection math, and the `f32` `View` actually
/// fed to `rasterize` (identical construction to `capture::convert_box_space`).
struct ViewGeometry {
    origin: [f64; 3],
    right: [f64; 3],
    down: [f64; 3],
    forward: [f64; 3],
    width: u32,
    height: u32,
}

fn view_geometry(view: CanonicalView, bounds: Bounds) -> (ViewGeometry, View) {
    let frame = view.frame(bounds);
    let (width, height) = view.dimensions(bounds);
    let as_f64 = |v: [i64; 3]| [v[0] as f64, v[1] as f64, v[2] as f64];
    let geometry = ViewGeometry {
        origin: as_f64(frame.origin),
        right: as_f64(frame.source_u),
        down: as_f64(frame.source_v),
        forward: as_f64(frame.inward),
        width,
        height,
    };
    let raster_view = View {
        origin: frame.origin.map(|c| c as f32),
        right: frame.source_u.map(|c| c as f32),
        down: frame.source_v.map(|c| c as f32),
        forward: frame.inward.map(|c| c as f32),
        scale: 1.0,
        width,
        height,
    };
    (geometry, raster_view)
}

struct Offender {
    x: u32,
    y: u32,
    expected: f64,
    got: Option<f64>,
    delta: f64,
}

/// For every `from`-view texel whose reconstructed 3D point faces both
/// `from` and `to` comfortably away from either silhouette, projects that
/// point into `to` and compares its depth there against the depth `to`
/// actually recorded at the texel containing the projection. Returns the
/// number of texels checked and any texel whose delta exceeds `TOL`
/// (worst offenders first).
fn check_cross_side_registration(
    scene: &TriangleScene,
    bounds: Bounds,
    from: CanonicalView,
    to: CanonicalView,
) -> (usize, Vec<Offender>) {
    let lighting = unlit();
    let (from_geo, from_raster_view) = view_geometry(from, bounds);
    let (to_geo, to_raster_view) = view_geometry(to, bounds);
    let from_raster = rasterize(scene, &from_raster_view, &lighting);
    let to_raster = rasterize(scene, &to_raster_view, &lighting);

    let tol = tolerance();
    let mut participating = 0usize;
    let mut offenders = Vec::new();

    for y in 0..from_geo.height {
        for x in 0..from_geo.width {
            let index = (y * from_geo.width + x) as usize;
            let depth = from_raster.depth[index];
            if !depth.is_finite() {
                continue;
            }
            let p = add(
                add(from_geo.origin, scale(from_geo.right, x as f64 + 0.5)),
                add(
                    scale(from_geo.down, y as f64 + 0.5),
                    scale(from_geo.forward, depth as f64),
                ),
            );
            let normal = normalize(sub(p, CENTER));
            let faces_from = -dot(normal, from_geo.forward) >= CONE_COS;
            let faces_to = -dot(normal, to_geo.forward) >= CONE_COS;
            if !(faces_from && faces_to) {
                continue;
            }
            participating += 1;

            let rel = sub(p, to_geo.origin);
            let u = dot(rel, to_geo.right);
            let v = dot(rel, to_geo.down);
            let expected = dot(rel, to_geo.forward);

            let tx = u.floor();
            let ty = v.floor();
            if tx < 0.0 || ty < 0.0 || tx >= to_geo.width as f64 || ty >= to_geo.height as f64 {
                offenders.push(Offender {
                    x,
                    y,
                    expected,
                    got: None,
                    delta: f64::INFINITY,
                });
                continue;
            }
            let to_index = (ty as u32 * to_geo.width + tx as u32) as usize;
            let got_depth = to_raster.depth[to_index];
            if !got_depth.is_finite() {
                offenders.push(Offender {
                    x,
                    y,
                    expected,
                    got: None,
                    delta: f64::INFINITY,
                });
                continue;
            }
            let delta = (got_depth as f64 - expected).abs();
            if delta > tol {
                offenders.push(Offender {
                    x,
                    y,
                    expected,
                    got: Some(got_depth as f64),
                    delta,
                });
            }
        }
    }
    offenders.sort_by(|a, b| b.delta.partial_cmp(&a.delta).unwrap());
    (participating, offenders)
}

fn assert_registration(from: CanonicalView, to: CanonicalView) {
    let scene = sphere_scene();
    let bounds = Bounds::new(63, 63, 63).unwrap();
    let (participating, offenders) = check_cross_side_registration(&scene, bounds, from, to);

    // Guards against a vacuous pass: the cone restriction should still
    // leave a substantial overlap wedge on a sphere inscribed in the box.
    assert!(
        participating >= 300,
        "{from:?} -> {to:?}: only {participating} texels fell inside the overlap cone \
         (expected at least a few hundred) — the check is vacuous"
    );

    if !offenders.is_empty() {
        let tol = tolerance();
        let worst: Vec<String> = offenders
            .iter()
            .take(5)
            .map(|o| {
                let got = o.got.map_or_else(
                    || "none (no finite depth / out of bounds)".to_string(),
                    |g| format!("{g:.4}"),
                );
                format!(
                    "  texel ({}, {}): expected {:.4}, got {got}, delta {:.4}",
                    o.x, o.y, o.expected, o.delta
                )
            })
            .collect();
        panic!(
            "{from:?} -> {to:?}: {} / {participating} texels exceeded TOL={tol:.4}\n{}",
            offenders.len(),
            worst.join("\n")
        );
    }
}

#[test]
fn cross_side_registration_front_to_top() {
    assert_registration(CanonicalView::Front, CanonicalView::Top);
}

#[test]
fn cross_side_registration_top_to_front() {
    assert_registration(CanonicalView::Top, CanonicalView::Front);
}
