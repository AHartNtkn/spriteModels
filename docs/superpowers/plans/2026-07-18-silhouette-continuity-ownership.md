# Silhouette-Continuity Ownership Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace mesh-import's fabricated-wall cut pass and sheet-blind ownership with the exact per-pair continuity model from `docs/superpowers/specs/2026-07-18-silhouette-continuity-ownership-design.md`, eliminating the bunny spike/seam bug class by construction.

**Architecture:** A new `continuity.rs` labels every 4-adjacency between covered texels of a capture side as continuous or cut, from the mesh cross-section in the vertical plane through the two texel centers. Ownership becomes a fixpoint that bans the far endpoint of violated cut edges and re-owns the surface via the next-best observing side. Closure dilates only across continuous edges, never into bans, and a post-closure sweep restores the invariant. `cuts.rs` is deleted.

**Tech Stack:** Rust (edition 2024, rust 1.92), crates `mesh-import` and `relief-core` (read-only dependency), criterion for the new bench. No new external dependencies except criterion as a dev-dependency of `mesh-import`.

## Global Constraints

- No heuristics: every constant must be derived (the only one here is `JOIN_GAP = 0.5 / RELIEF_UNITS_PER_PIXEL`, half the format's relief quantum).
- No silent failures: internal impossibilities are `debug_assert!` + deterministic recovery, never swallowed; missing fixtures fail loudly.
- Tests assert correct behavior only; bug-reproduction tests must be observed RED before the fix and GREEN after.
- No legacy/compat shims: `cuts.rs` and the old passes are deleted outright, callers rewritten.
- Tests may use `crates/mesh-import/tests/fixtures/*` (committed test fixtures) but never `assets/` (user artifacts).
- Workspace gates before any commit claiming green: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.
- Commit messages end with the Claude Code trailer used in this repo (see recent `git log`).

---

### Task 1: Rasterizer records the winning triangle index

**Files:**
- Modify: `crates/mesh-import/src/raster.rs`
- Test: `crates/mesh-import/tests/raster.rs`

**Interfaces:**
- Consumes: existing `Raster`, `rasterize`.
- Produces: `Raster.triangle: Vec<u32>` — per texel, the index (into `scene.triangles`) of the triangle that won the depth test; `u32::MAX` where `depth` is not finite. Task 3's continuity builder and Task 4's `CaptureSide` rely on this exact field name and sentinel.

- [ ] **Step 1: Write the failing test**

Append to `crates/mesh-import/tests/raster.rs` (it already has `quad`, `plain_material`, `front_view` helpers; `quad(z, material)` covers x,y in [0,4] at depth `z` as triangles `[2*n, 2*n+1]` in push order):

```rust
/// The raster must identify which triangle won each texel: with a far quad
/// (triangles 0,1) fully covered by a nearer quad (triangles 2,3), every
/// covered texel's winning index is one of the near quad's triangles, and
/// uncovered texels hold the u32::MAX sentinel.
#[test]
fn raster_records_winning_triangle_indices() {
    let far = quad(5.0, 0);
    let near = quad(1.0, 0);
    let scene = TriangleScene {
        triangles: far.into_iter().chain(near).collect(),
        materials: vec![plain_material()],
    };
    let raster = rasterize(
        &scene,
        &front_view(),
        &Lighting {
            direction: [0.0, 0.0, -1.0],
            ambient: 1.0,
        },
    );
    for (i, &depth) in raster.depth.iter().enumerate() {
        if depth.is_finite() {
            assert!(
                raster.triangle[i] == 2 || raster.triangle[i] == 3,
                "texel {i}: near quad (triangles 2,3) must win, got {}",
                raster.triangle[i]
            );
            assert_eq!(depth, 1.0, "texel {i}: near quad depth");
        } else {
            assert_eq!(raster.triangle[i], u32::MAX, "uncovered texel {i}");
        }
    }
    // The 4x4 view over a quad spanning [0,4]^2 is fully covered.
    assert!(raster.depth.iter().all(|d| d.is_finite()), "full coverage");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p mesh-import --test raster raster_records_winning_triangle_indices`
Expected: FAIL to compile — `no field 'triangle' on type 'Raster'`.

- [ ] **Step 3: Implement**

In `crates/mesh-import/src/raster.rs`:

1. Add to `struct Raster` after `face_normal`:

```rust
    /// Index into the scene's triangle list of the triangle that won the
    /// depth test at each texel; `u32::MAX` where `depth` is not finite.
    pub triangle: Vec<u32>,
```

2. In `rasterize`, add alongside the other buffers:

```rust
    let mut triangle = vec![u32::MAX; width * height];
```

3. Change the triangle loop header to carry the index:

```rust
    for (tri_idx, tri) in scene.triangles.iter().enumerate() {
```

4. Next to `face_normal[index] = tri_normal;` add:

```rust
                triangle[index] = tri_idx as u32;
```

5. Add `triangle,` to the returned `Raster`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p mesh-import`
Expected: all PASS (new test included).

- [ ] **Step 5: Commit**

```bash
git add crates/mesh-import/src/raster.rs crates/mesh-import/tests/raster.rs
git commit -m "feat: rasterizer records each hit's winning triangle index"
```

---

### Task 2: Continuity module — cross-section geometry primitives

**Files:**
- Create: `crates/mesh-import/src/continuity.rs`
- Modify: `crates/mesh-import/src/lib.rs` (add `mod continuity;` after `mod capture;`)

**Interfaces:**
- Consumes: `relief_core::RELIEF_UNITS_PER_PIXEL`.
- Produces (all `pub(crate)`, consumed by Task 3 in this same file):
  - `struct CrossSegment { a: [f64; 2], b: [f64; 2], triangle: u32 }` — endpoints in `(t, d)`.
  - `fn slice_triangle(v: [[f64; 3]; 3], triangle: u32, out: &mut Vec<CrossSegment>)` — vertices as `(w, t, d)`, plane `w = 0`.
  - `fn clip_segment(seg: CrossSegment, reach: f64) -> Option<CrossSegment>` — clip to `t ∈ [0,1]`, `d ≤ reach`.
  - `fn point_segment_distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64`
  - `fn segment_segment_distance(s: &CrossSegment, t: &CrossSegment) -> f64`
  - `const JOIN_GAP: f64` = `0.5 / RELIEF_UNITS_PER_PIXEL as f64`

- [ ] **Step 1: Create the module with failing unit tests**

Create `crates/mesh-import/src/continuity.rs`:

```rust
//! Exact per-pair surface-continuity labels for capture (spec:
//! docs/superpowers/specs/2026-07-18-silhouette-continuity-ownership-design.md).
//!
//! For two 4-adjacent covered texels of one side, the verdict answers: is
//! the tent bridge between their two samples backed by continuous,
//! reachable surface? It is decided on the mesh cross-section in the
//! vertical plane through the two texel centers, restricted to the strip
//! between them. Occlusion of the in-between surface is deliberately
//! irrelevant: a bridge behind nearer geometry composites correctly via
//! transient depth; only a bridge through empty space fabricates surface.

use relief_core::RELIEF_UNITS_PER_PIXEL;

/// Half the relief quantum, in texels. Gaps below it are treated as
/// closed: the encoding cannot distinguish them from continuity (depth
/// resolves to 1/8 px, lateral position to a full texel, so half of the
/// finer of the two bounds every representable separation). Derived from
/// the format, not tuned.
pub(crate) const JOIN_GAP: f64 = 0.5 / RELIEF_UNITS_PER_PIXEL as f64;

/// One triangle's intersection with the slicing plane, in (t, d)
/// coordinates: t along the pair's center-to-center segment (0 at the
/// first center, 1 at the second), d depth along the side's forward axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CrossSegment {
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub triangle: u32,
}

/// Intersects one triangle with the plane `w = 0`, vertices given as
/// (w, t, d). The generic crossing yields one segment. A triangle lying
/// entirely in the plane contributes its three edges: its boundary
/// connects everything its interior would, so interior area is redundant
/// for connectivity. Vertices exactly on the plane are pushed once (by
/// their leading edge); the straddle test is strict, so no duplicates.
pub(crate) fn slice_triangle(v: [[f64; 3]; 3], triangle: u32, out: &mut Vec<CrossSegment>) {
    let w = [v[0][0], v[1][0], v[2][0]];
    if w == [0.0, 0.0, 0.0] {
        for (i, j) in [(0, 1), (1, 2), (2, 0)] {
            out.push(CrossSegment {
                a: [v[i][1], v[i][2]],
                b: [v[j][1], v[j][2]],
                triangle,
            });
        }
        return;
    }
    let mut points: [[f64; 2]; 2] = [[0.0; 2]; 2];
    let mut count = 0usize;
    let mut push = |p: [f64; 2]| {
        if count < 2 {
            points[count] = p;
        }
        count += 1;
    };
    for (i, j) in [(0usize, 1usize), (1, 2), (2, 0)] {
        let (wi, wj) = (w[i], w[j]);
        if wi == 0.0 {
            push([v[i][1], v[i][2]]);
        }
        if (wi < 0.0 && wj > 0.0) || (wi > 0.0 && wj < 0.0) {
            let s = wi / (wi - wj);
            push([
                v[i][1] + s * (v[j][1] - v[i][1]),
                v[i][2] + s * (v[j][2] - v[i][2]),
            ]);
        }
    }
    debug_assert!(count <= 2, "non-coplanar triangle produced {count} plane points");
    if count == 2 {
        out.push(CrossSegment {
            a: points[0],
            b: points[1],
            triangle,
        });
    }
}

/// Liang-Barsky clip of a cross segment to the strip `t in [0, 1]` and the
/// reachable half-space `d <= reach`. Returns None when nothing remains.
pub(crate) fn clip_segment(seg: CrossSegment, reach: f64) -> Option<CrossSegment> {
    let dt = seg.b[0] - seg.a[0];
    let dd = seg.b[1] - seg.a[1];
    let (mut s0, mut s1) = (0.0f64, 1.0f64);
    // Each constraint keeps points with num + s * den >= 0.
    for (num, den) in [
        (seg.a[0], dt),         // t >= 0
        (1.0 - seg.a[0], -dt),  // t <= 1
        (reach - seg.a[1], -dd), // d <= reach
    ] {
        if den == 0.0 {
            if num < 0.0 {
                return None;
            }
        } else {
            let s = -num / den;
            if den > 0.0 {
                s0 = s0.max(s);
            } else {
                s1 = s1.min(s);
            }
        }
    }
    if s0 > s1 {
        return None;
    }
    let at = |s: f64| [seg.a[0] + s * dt, seg.a[1] + s * dd];
    Some(CrossSegment {
        a: at(s0),
        b: at(s1),
        triangle: seg.triangle,
    })
}

pub(crate) fn point_segment_distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let ap = [p[0] - a[0], p[1] - a[1]];
    let len2 = ab[0] * ab[0] + ab[1] * ab[1];
    let s = if len2 > 0.0 {
        ((ap[0] * ab[0] + ap[1] * ab[1]) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let q = [a[0] + s * ab[0] - p[0], a[1] + s * ab[1] - p[1]];
    (q[0] * q[0] + q[1] * q[1]).sqrt()
}

fn orient(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn segments_properly_cross(s: &CrossSegment, t: &CrossSegment) -> bool {
    let d1 = orient(t.a, t.b, s.a);
    let d2 = orient(t.a, t.b, s.b);
    let d3 = orient(s.a, s.b, t.a);
    let d4 = orient(s.a, s.b, t.b);
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// 2D distance between two segments: zero for proper crossings; touching
/// and collinear-overlap cases resolve through the endpoint-to-segment
/// minimum.
pub(crate) fn segment_segment_distance(s: &CrossSegment, t: &CrossSegment) -> f64 {
    if segments_properly_cross(s, t) {
        return 0.0;
    }
    point_segment_distance(s.a, t.a, t.b)
        .min(point_segment_distance(s.b, t.a, t.b))
        .min(point_segment_distance(t.a, s.a, s.b))
        .min(point_segment_distance(t.b, s.a, s.b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(a: [f64; 2], b: [f64; 2]) -> CrossSegment {
        CrossSegment { a, b, triangle: 0 }
    }

    /// Generic straddle: vertices at w = -1, +1, +1 produce one segment
    /// whose endpoints are the two edge crossings, linearly interpolated.
    #[test]
    fn slice_straddling_triangle_yields_one_segment() {
        let mut out = Vec::new();
        slice_triangle(
            [[-1.0, 0.0, 0.0], [1.0, 2.0, 2.0], [1.0, 0.0, 4.0]],
            7,
            &mut out,
        );
        assert_eq!(out.len(), 1);
        let s = out[0];
        assert_eq!(s.triangle, 7);
        let mut endpoints = [s.a, s.b];
        endpoints.sort_by(|p, q| p[0].total_cmp(&q[0]));
        assert_eq!(endpoints[0], [0.0, 2.0]); // edge (v2, v0) at s = 0.5
        assert_eq!(endpoints[1], [1.0, 1.0]); // edge (v0, v1) at s = 0.5
    }

    /// A vertex exactly on the plane plus a straddling opposite edge:
    /// exactly two points, no duplicate of the on-plane vertex.
    #[test]
    fn slice_vertex_on_plane_yields_one_segment() {
        let mut out = Vec::new();
        slice_triangle(
            [[0.0, 3.0, 5.0], [1.0, 0.0, 0.0], [-1.0, 2.0, 2.0]],
            0,
            &mut out,
        );
        assert_eq!(out.len(), 1);
        let s = out[0];
        assert!(s.a == [3.0, 5.0] || s.b == [3.0, 5.0], "on-plane vertex kept");
    }

    /// All vertices off-plane on the same side: no intersection.
    #[test]
    fn slice_non_crossing_triangle_yields_nothing() {
        let mut out = Vec::new();
        slice_triangle(
            [[1.0, 0.0, 0.0], [2.0, 1.0, 1.0], [0.5, 2.0, 2.0]],
            0,
            &mut out,
        );
        assert!(out.is_empty());
    }

    /// A coplanar triangle contributes its three boundary edges.
    #[test]
    fn slice_coplanar_triangle_yields_boundary() {
        let mut out = Vec::new();
        slice_triangle(
            [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            0,
            &mut out,
        );
        assert_eq!(out.len(), 3);
    }

    /// Clip to the strip: a horizontal segment overshooting both strip
    /// boundaries is trimmed to t in [0, 1] with depth preserved.
    #[test]
    fn clip_trims_to_strip() {
        let clipped = clip_segment(seg([-1.0, 1.0], [2.0, 1.0]), 5.0).expect("survives");
        let mut endpoints = [clipped.a, clipped.b];
        endpoints.sort_by(|p, q| p[0].total_cmp(&q[0]));
        assert_eq!(endpoints, [[0.0, 1.0], [1.0, 1.0]]);
    }

    /// Clip against reach: a segment whose in-strip part lies wholly past
    /// reach is removed entirely.
    #[test]
    fn clip_removes_unreachable_remainder() {
        // t: -0.5 -> 0.5, d: 0 -> 10; in-strip requires s >= 0.5 where
        // d = 5..10, but reach 4 requires s <= 0.4.
        assert_eq!(clip_segment(seg([-0.5, 0.0], [0.5, 10.0]), 4.0), None);
    }

    /// Distances: proper crossing is zero; parallel segments report their
    /// gap; degenerate (point) segments work through the endpoint path.
    #[test]
    fn segment_distances() {
        let cross_a = seg([0.0, 0.0], [1.0, 1.0]);
        let cross_b = seg([0.0, 1.0], [1.0, 0.0]);
        assert_eq!(segment_segment_distance(&cross_a, &cross_b), 0.0);

        let low = seg([0.0, 0.0], [1.0, 0.0]);
        let high = seg([0.0, 2.0], [1.0, 2.0]);
        assert_eq!(segment_segment_distance(&low, &high), 2.0);

        let point = seg([0.5, 0.5], [0.5, 0.5]);
        assert_eq!(segment_segment_distance(&low, &point), 0.5);
    }
}
```

In `crates/mesh-import/src/lib.rs`, after `mod capture;` add:

```rust
mod continuity;
```

- [ ] **Step 2: Run tests to verify they fail, then pass**

Run: `cargo test -p mesh-import continuity`
Expected on first run of the incomplete module: compile errors until the file is complete; once complete as above: all 7 tests PASS. (`JOIN_GAP`, the distance helpers, and `CrossSegment` will be flagged as dead code by clippy until Task 3 — silence nothing; instead confirm `cargo clippy -p mesh-import --all-targets -- -D warnings` only complains about dead code, and proceed to Task 3 before committing if it does. If dead-code warnings block the gate, commit Tasks 2 and 3 together at Task 3's commit step.)

- [ ] **Step 3: Commit (or defer to Task 3's commit if dead-code warnings block the clippy gate)**

```bash
git add crates/mesh-import/src/continuity.rs crates/mesh-import/src/lib.rs
git commit -m "feat: cross-section geometry primitives for continuity labels"
```

---

### Task 3: Continuity module — pair verdict and per-side edge labels

**Files:**
- Modify: `crates/mesh-import/src/continuity.rs`

**Interfaces:**
- Consumes: Task 2's primitives; `crate::Triangle`; Task 1's winning-index convention.
- Produces (`pub(crate)`, consumed by Tasks 4–6):

```rust
pub(crate) struct SideView<'a> {
    pub origin: [f64; 3],
    pub right: [f64; 3],
    pub down: [f64; 3],
    pub forward: [f64; 3],
    pub width: u32,
    pub height: u32,
    pub h_max: i64,
    pub depth: &'a [f64],   // reachability-filtered; INFINITY = uncovered
    pub winning: &'a [u32], // winning triangle per covered texel; u32::MAX elsewhere
}

pub(crate) struct SideContinuity { /* width, height, horizontal, vertical */ }
impl SideContinuity {
    /// Whether the 4-adjacent pair (ax,ay)-(bx,by) is surface-continuous.
    /// Callers must pass 4-adjacent in-bounds texels; order-insensitive.
    pub(crate) fn connected(&self, ax: u32, ay: u32, bx: u32, by: u32) -> bool;
    #[cfg(test)]
    pub(crate) fn uniform(width: u32, height: u32, value: bool) -> Self;
    #[cfg(test)]
    pub(crate) fn from_edges(width: u32, height: u32, horizontal: Vec<bool>, vertical: Vec<bool>) -> Self;
}

pub(crate) fn side_continuity(triangles: &[crate::Triangle], side: &SideView) -> SideContinuity;
```

- [ ] **Step 1: Write the failing behavior tests**

Append to `crates/mesh-import/src/continuity.rs`'s test module (new `use` lines at the top of `mod tests`):

```rust
    use crate::{Lighting, Material, Triangle, TriangleScene, View, rasterize};
    use relief_core::RELIEF_UNITS_PER_PIXEL as UNITS;

    fn tri3(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> Triangle {
        Triangle {
            positions: [a, b, c],
            normals: [[0.0, 0.0, -1.0]; 3],
            uvs: [[0.0, 0.0]; 3],
            colors: [[1.0, 1.0, 1.0, 1.0]; 3],
            material: 0,
        }
    }

    /// Planar quad from four corners (two triangles sharing the p0-p2
    /// diagonal), for slanted and axis-aligned surfaces alike.
    fn quad4(p0: [f32; 3], p1: [f32; 3], p2: [f32; 3], p3: [f32; 3]) -> [Triangle; 2] {
        [tri3(p0, p1, p2), tri3(p0, p2, p3)]
    }

    /// Axis-aligned rect at constant depth z spanning [x0,x1] x [y0,y1].
    fn rect(x0: f32, x1: f32, y0: f32, y1: f32, z: f32) -> [Triangle; 2] {
        quad4([x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z])
    }

    /// Rasterizes a front-facing view (right +x, down +y, forward +z),
    /// applies the capture reachability filter, and builds edge labels —
    /// the same pipeline capture will drive, minus color.
    fn side_labels(triangles: Vec<Triangle>, width: u32, height: u32, h_max: i64) -> SideContinuity {
        let scene = TriangleScene {
            triangles,
            materials: vec![Material {
                base_color_factor: [1.0, 1.0, 1.0, 1.0],
                base_color_texture: None,
                alpha_cutoff: None,
            }],
        };
        let raster = rasterize(
            &scene,
            &View {
                origin: [0.0; 3],
                right: [1.0, 0.0, 0.0],
                down: [0.0, 1.0, 0.0],
                forward: [0.0, 0.0, 1.0],
                scale: 1.0,
                width,
                height,
            },
            &Lighting {
                direction: [0.0, 0.0, -1.0],
                ambient: 1.0,
            },
        );
        let count = (width * height) as usize;
        let mut depth = vec![f64::INFINITY; count];
        let mut winning = vec![u32::MAX; count];
        for i in 0..count {
            let d = raster.depth[i];
            if d == f32::INFINITY {
                continue;
            }
            let relief = (f64::from(d) * UNITS as f64).round() as i64;
            if relief.max(0) > h_max {
                continue;
            }
            depth[i] = f64::from(d);
            winning[i] = raster.triangle[i];
        }
        side_continuity(
            &scene.triangles,
            &SideView {
                origin: [0.0; 3],
                right: [1.0, 0.0, 0.0],
                down: [0.0, 1.0, 0.0],
                forward: [0.0, 0.0, 1.0],
                width,
                height,
                h_max,
                depth: &depth,
                winning: &winning,
            },
        )
    }

    /// Occluding silhouette: a near surface ends over a far one with empty
    /// space between. The near-far adjacency is cut; same-surface
    /// adjacencies on either side stay connected.
    #[test]
    fn silhouette_pair_is_cut() {
        let mut triangles = Vec::new();
        triangles.extend(rect(-1.0, 1.7, -1.0, 2.0, 1.0)); // near, wins texels 0,1
        triangles.extend(rect(-1.0, 5.0, -1.0, 2.0, 5.0)); // far, wins texels 2,3
        let labels = side_labels(triangles, 4, 1, 400);
        assert!(labels.connected(0, 0, 1, 0), "near surface is one sheet");
        assert!(!labels.connected(1, 0, 2, 0), "silhouette jump must cut");
        assert!(labels.connected(2, 0, 3, 0), "far surface is one sheet");
    }

    /// A fold whose two slanted surfaces share a mesh edge is continuous
    /// across the crease, even at 45 degrees (8 relief units per texel —
    /// the case the old 10-unit threshold family could not distinguish
    /// from an occlusion).
    #[test]
    fn fold_sharing_a_mesh_edge_is_connected() {
        let mut triangles = Vec::new();
        // z = x + 2 for x in [-1, 2]; z = 6 - x for x in [2, 5]; crease
        // vertices at (2, *, 4) are shared exactly.
        triangles.extend(quad4(
            [-1.0, -1.0, 1.0],
            [2.0, -1.0, 4.0],
            [2.0, 2.0, 4.0],
            [-1.0, 2.0, 1.0],
        ));
        triangles.extend(quad4(
            [2.0, -1.0, 4.0],
            [5.0, -1.0, 1.0],
            [5.0, 2.0, 1.0],
            [2.0, 2.0, 4.0],
        ));
        let labels = side_labels(triangles, 4, 1, 400);
        for x in 0..3 {
            assert!(labels.connected(x, 0, x + 1, 0), "fold edge ({x})-({})", x + 1);
        }
    }

    /// A sub-resolution sliver occluding the middle of a same-surface pair
    /// must NOT cut it: the bridge lies on real continuous surface and the
    /// sliver is ordinary sampling loss. (A visible-depth/lower-envelope
    /// formulation fails this case; see the spec's rejected alternative.)
    #[test]
    fn sub_resolution_sliver_does_not_cut() {
        let mut triangles = Vec::new();
        triangles.extend(rect(-1.0, 5.0, -1.0, 2.0, 5.0)); // far surface
        triangles.extend(rect(1.9, 2.1, -1.0, 2.0, 1.0)); // sliver, wins no center
        let labels = side_labels(triangles, 4, 1, 400);
        assert!(labels.connected(1, 0, 2, 0), "sliver-occluded same-surface pair");
    }

    /// A silhouette clipping the corner of the segment between two texels
    /// that both sample the far surface: connected, even though the
    /// clipping surface is real and resolved elsewhere in the chart.
    #[test]
    fn corner_clip_between_same_surface_samples_is_connected() {
        let mut triangles = Vec::new();
        triangles.extend(rect(-1.0, 5.0, -1.0, 3.0, 5.0)); // far surface
        // Near spike: apex (2.0, 0.2), base y = 2.6 from x = 1 to 3. It
        // crosses the row-0 pair segment (y = 0.5, x in [1.87, 2.13]) but
        // contains no row-0 center; it wins (1,1) and (2,1).
        triangles.push(tri3([1.0, 2.6, 1.0], [3.0, 2.6, 1.0], [2.0, 0.2, 1.0]));
        let labels = side_labels(triangles, 4, 2, 400);
        assert!(
            labels.connected(1, 0, 2, 0),
            "corner-clipped same-surface pair stays connected"
        );
        assert!(!labels.connected(1, 0, 1, 1), "far-near pair is cut");
        assert!(!labels.connected(2, 0, 2, 1), "far-near pair is cut");
        assert!(labels.connected(1, 1, 2, 1), "near surface is one sheet");
    }

    /// Two towers with empty space between them: maximally discontinuous.
    #[test]
    fn empty_space_between_towers_is_cut() {
        let mut triangles = Vec::new();
        triangles.extend(rect(-1.0, 1.7, -1.0, 2.0, 1.0));
        triangles.extend(rect(2.3, 5.0, -1.0, 2.0, 1.0));
        let labels = side_labels(triangles, 4, 1, 400);
        assert!(!labels.connected(1, 0, 2, 0), "no surface between the towers");
    }

    /// A groove dipping past this side's reach disconnects the pair (the
    /// opposite chart owns the dip; bridging would fabricate a roof) —
    /// but the identical geometry within reach stays connected.
    #[test]
    fn groove_past_reach_cuts_within_reach_connects() {
        let groove = || {
            let mut triangles = Vec::new();
            triangles.extend(rect(-1.0, 1.6, -1.0, 2.0, 1.0));
            triangles.extend(quad4(
                [1.6, -1.0, 1.0],
                [2.0, -1.0, 6.0],
                [2.0, 2.0, 6.0],
                [1.6, 2.0, 1.0],
            ));
            triangles.extend(quad4(
                [2.0, -1.0, 6.0],
                [2.4, -1.0, 1.0],
                [2.4, 2.0, 1.0],
                [2.0, 2.0, 6.0],
            ));
            triangles.extend(rect(2.4, 5.0, -1.0, 2.0, 1.0));
            triangles
        };
        // reach = (h_max + 0.5) / 8: 3.06 texels at h_max 24 — the rims
        // (d = 1) are reachable, the groove bottom (d = 6) is not.
        let shallow = side_labels(groove(), 4, 1, 24);
        assert!(!shallow.connected(1, 0, 2, 0), "groove past reach must cut");
        let deep = side_labels(groove(), 4, 1, 400);
        assert!(deep.connected(1, 0, 2, 0), "groove within reach connects");
    }

    /// Cracks: a gap below JOIN_GAP (an authoring artifact the encoding
    /// cannot even represent) closes; a wider real gap cuts.
    #[test]
    fn hairline_crack_connects_wider_crack_cuts() {
        let hairline = {
            let mut triangles = Vec::new();
            triangles.extend(rect(-1.0, 2.0, -1.0, 2.0, 1.0));
            triangles.extend(rect(2.03, 5.0, -1.0, 2.0, 1.0)); // 0.03 < 1/16
            side_labels(triangles, 4, 1, 400)
        };
        assert!(hairline.connected(1, 0, 2, 0), "sub-quantum crack closes");
        let wide = {
            let mut triangles = Vec::new();
            triangles.extend(rect(-1.0, 2.0, -1.0, 2.0, 1.0));
            triangles.extend(rect(2.2, 5.0, -1.0, 2.0, 1.0)); // 0.2 > 1/16
            side_labels(triangles, 4, 1, 400)
        };
        assert!(!wide.connected(1, 0, 2, 0), "a real gap cuts");
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p mesh-import continuity`
Expected: FAIL to compile — `SideView`, `SideContinuity`, `side_continuity` not found.

- [ ] **Step 3: Implement**

Add to `crates/mesh-import/src/continuity.rs` (above the test module):

```rust
use crate::Triangle;

/// The per-side inputs continuity needs, borrowed from capture state.
pub(crate) struct SideView<'a> {
    pub origin: [f64; 3],
    pub right: [f64; 3],
    pub down: [f64; 3],
    pub forward: [f64; 3],
    pub width: u32,
    pub height: u32,
    pub h_max: i64,
    /// Reachability-filtered depth in texels; INFINITY = uncovered.
    pub depth: &'a [f64],
    /// Winning triangle per covered texel; u32::MAX elsewhere.
    pub winning: &'a [u32],
}

/// Edge labels for one side: `true` = surface-continuous. Edges touching
/// an uncovered texel are stored `false` and are never consulted.
pub(crate) struct SideContinuity {
    width: u32,
    height: u32,
    /// (width-1) * height entries; index y * (width-1) + x labels the edge
    /// (x, y)-(x+1, y).
    horizontal: Vec<bool>,
    /// width * (height-1) entries; index y * width + x labels the edge
    /// (x, y)-(x, y+1).
    vertical: Vec<bool>,
}

impl SideContinuity {
    pub(crate) fn connected(&self, ax: u32, ay: u32, bx: u32, by: u32) -> bool {
        let (x0, y0) = (ax.min(bx), ay.min(by));
        if ay == by {
            debug_assert!(ax.abs_diff(bx) == 1 && x0 + 1 < self.width);
            self.horizontal[(y0 * (self.width - 1) + x0) as usize]
        } else {
            debug_assert!(ay.abs_diff(by) == 1 && ax == bx);
            self.vertical[(y0 * self.width + x0) as usize]
        }
    }

    #[cfg(test)]
    pub(crate) fn uniform(width: u32, height: u32, value: bool) -> Self {
        Self {
            width,
            height,
            horizontal: vec![value; ((width - 1) * height) as usize],
            vertical: vec![value; (width * (height - 1)) as usize],
        }
    }

    #[cfg(test)]
    pub(crate) fn from_edges(
        width: u32,
        height: u32,
        horizontal: Vec<bool>,
        vertical: Vec<bool>,
    ) -> Self {
        assert_eq!(horizontal.len(), ((width - 1) * height) as usize);
        assert_eq!(vertical.len(), (width * (height - 1)) as usize);
        Self {
            width,
            height,
            horizontal,
            vertical,
        }
    }
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Per-texel triangle buckets from projected AABBs, clamped to the grid. A
/// zero-area projection (an edge-on wall) still lands in its column — such
/// triangles contribute no raster coverage but do carry cross-section
/// surface, exactly the case a real step wall depends on.
fn buckets(projected: &[[[f64; 3]; 3]], width: u32, height: u32) -> Vec<Vec<u32>> {
    let mut cells = vec![Vec::new(); (width * height) as usize];
    let clamp = |value: f64, dim: u32| (value.floor() as i64).clamp(0, i64::from(dim) - 1) as u32;
    for (tri_idx, tri) in projected.iter().enumerate() {
        let (mut min_u, mut max_u) = (f64::INFINITY, f64::NEG_INFINITY);
        let (mut min_v, mut max_v) = (f64::INFINITY, f64::NEG_INFINITY);
        for vertex in tri {
            min_u = min_u.min(vertex[0]);
            max_u = max_u.max(vertex[0]);
            min_v = min_v.min(vertex[1]);
            max_v = max_v.max(vertex[1]);
        }
        if max_u < 0.0 || max_v < 0.0 || min_u >= f64::from(width) || min_v >= f64::from(height) {
            continue;
        }
        let (x0, x1) = (clamp(min_u, width), clamp(max_u, width));
        let (y0, y1) = (clamp(min_v, height), clamp(max_v, height));
        for y in y0..=y1 {
            for x in x0..=x1 {
                cells[(y * width + x) as usize].push(tri_idx as u32);
            }
        }
    }
    cells
}

/// The pair verdict on assembled cross-section segments: connected iff the
/// two samples lie on one component of the join graph (segments within
/// JOIN_GAP touch — shared mesh edges coincide exactly; sub-quantum cracks
/// close; proper crossings touch at distance zero).
fn pair_connected(segments: &[CrossSegment], d0: f64, win0: u32, d1: f64, win1: u32) -> bool {
    // Same winning triangle: both samples on one plane; depth between them
    // is affine, bounded by the two in-reach endpoints, so the whole
    // bridge lies on real reachable surface.
    if win0 == win1 {
        return true;
    }
    let find = |t: f64, d: f64, tri: u32| -> Option<usize> {
        segments
            .iter()
            .position(|s| s.triangle == tri && point_segment_distance([t, d], s.a, s.b) <= JOIN_GAP)
            .or_else(|| {
                // Degenerate slices can drop a zero-length winning
                // cross-section; any segment through the sample serves.
                segments
                    .iter()
                    .position(|s| point_segment_distance([t, d], s.a, s.b) <= JOIN_GAP)
            })
    };
    let (Some(start), Some(end)) = (find(0.0, d0, win0), find(1.0, d1, win1)) else {
        debug_assert!(false, "sample point lies on no cross-section segment");
        return false;
    };
    if start == end {
        return true;
    }
    let mut reachable = vec![false; segments.len()];
    let mut queue = vec![start];
    reachable[start] = true;
    while let Some(i) = queue.pop() {
        for j in 0..segments.len() {
            if !reachable[j] && segment_segment_distance(&segments[i], &segments[j]) <= JOIN_GAP {
                if j == end {
                    return true;
                }
                reachable[j] = true;
                queue.push(j);
            }
        }
    }
    false
}

pub(crate) fn side_continuity(triangles: &[Triangle], side: &SideView) -> SideContinuity {
    let (width, height) = (side.width, side.height);
    // Project every vertex into (u, v, d) once.
    let projected: Vec<[[f64; 3]; 3]> = triangles
        .iter()
        .map(|tri| {
            tri.positions.map(|p| {
                let p = [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])];
                let rel = sub3(p, side.origin);
                [
                    dot3(rel, side.right),
                    dot3(rel, side.down),
                    dot3(rel, side.forward),
                ]
            })
        })
        .collect();
    let cells = buckets(&projected, width, height);
    let reach = (side.h_max as f64 + 0.5) / RELIEF_UNITS_PER_PIXEL as f64;
    let at = |x: u32, y: u32| (y * width + x) as usize;

    let mut horizontal = vec![false; ((width.saturating_sub(1)) * height) as usize];
    let mut vertical = vec![false; (width * height.saturating_sub(1)) as usize];
    let mut segments: Vec<CrossSegment> = Vec::new();
    let mut gathered: Vec<u32> = Vec::new();

    let mut label = |x: u32, y: u32, nx: u32, ny: u32, out: &mut Vec<bool>, out_idx: usize| {
        let (i, j) = (at(x, y), at(nx, ny));
        if !side.depth[i].is_finite() || !side.depth[j].is_finite() {
            return;
        }
        let (win0, win1) = (side.winning[i], side.winning[j]);
        if win0 == win1 {
            out[out_idx] = true;
            return;
        }
        gathered.clear();
        gathered.extend_from_slice(&cells[i]);
        gathered.extend_from_slice(&cells[j]);
        gathered.sort_unstable();
        gathered.dedup();
        segments.clear();
        // Slicing coordinates: w is the fixed screen coordinate minus its
        // plane value; t is the moving screen coordinate minus the first
        // center; d passes through.
        let horizontal_pair = ny == y;
        let (t0, w0) = if horizontal_pair {
            (f64::from(x) + 0.5, f64::from(y) + 0.5)
        } else {
            (f64::from(y) + 0.5, f64::from(x) + 0.5)
        };
        for &tri_idx in &gathered {
            let v = projected[tri_idx as usize].map(|p| {
                if horizontal_pair {
                    [p[1] - w0, p[0] - t0, p[2]]
                } else {
                    [p[0] - w0, p[1] - t0, p[2]]
                }
            });
            let before = segments.len();
            slice_triangle(v, tri_idx, &mut segments);
            // Clip in place; drop what the strip/reach excludes.
            let mut kept = before;
            for s in before..segments.len() {
                if let Some(clipped) = clip_segment(segments[s], reach) {
                    segments[kept] = clipped;
                    kept += 1;
                }
            }
            segments.truncate(kept);
        }
        out[out_idx] = pair_connected(&segments, side.depth[i], win0, side.depth[j], win1);
    };

    for y in 0..height {
        for x in 0..width {
            if x + 1 < width {
                let idx = (y * (width - 1) + x) as usize;
                label(x, y, x + 1, y, &mut horizontal, idx);
            }
            if y + 1 < height {
                let idx = (y * width + x) as usize;
                label(x, y, x, y + 1, &mut vertical, idx);
            }
        }
    }
    SideContinuity {
        width,
        height,
        horizontal,
        vertical,
    }
}
```

Note: the closure `label` borrows `segments`/`gathered` mutably while reading `cells`/`projected`; if the borrow checker rejects the closure form, inline it as a helper function taking all state as parameters — same body, no logic change.

- [ ] **Step 4: Run tests**

Run: `cargo test -p mesh-import continuity`
Expected: all continuity tests PASS (7 primitive tests + 8 behavior tests).

- [ ] **Step 5: Commit**

```bash
git add crates/mesh-import/src/continuity.rs crates/mesh-import/src/lib.rs
git commit -m "feat: exact per-pair surface-continuity labels from mesh cross-sections"
```

---

### Task 4: Ownership fixpoint with bans and rescue

**Files:**
- Modify: `crates/mesh-import/src/capture.rs`

**Interfaces:**
- Consumes: `SideContinuity`, `SideView` from Task 3; `Raster.triangle` from Task 1.
- Produces (consumed by Tasks 5–6):
  - `CaptureSide` gains `pub(crate)` visibility on the struct and its fields, plus field `winning: Vec<u32>` and method `fn continuity_view(&self) -> crate::continuity::SideView<'_>`.
  - `pub(crate) struct OwnershipState { pub kept: Vec<Vec<bool>>, pub banned: Vec<Vec<bool>> }`
  - `pub(crate) fn ownership_masks(sides: &[CaptureSide], continuity: &[SideContinuity]) -> OwnershipState`
  - `fn sees_point(t: &CaptureSide, p: [f64; 3]) -> Option<usize>` (private helper, also used by Task 6's property tests via the crate-internal test module)
- Deletes: `fn owning_mask` (its candidacy logic moves into `better_candidates`/`sees_point`; its doc comments about conditions 1–4 move with it).

- [ ] **Step 1: Write the failing test**

Append to the `#[cfg(test)] mod tests` in `crates/mesh-import/src/capture.rs`:

```rust
    use super::{CaptureSide, OwnershipState, capture_side, ownership_masks, sees_point};
    use crate::continuity::side_continuity;
    use crate::{Lighting, Material, Triangle, TriangleScene};
    use relief_core::{Bounds, CanonicalView};

    fn tri3(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> Triangle {
        Triangle {
            positions: [a, b, c],
            normals: [[0.0, -1.0, 0.0]; 3],
            uvs: [[0.0, 0.0]; 3],
            colors: [[1.0, 1.0, 1.0, 1.0]; 3],
            material: 0,
        }
    }

    fn quad4(p0: [f32; 3], p1: [f32; 3], p2: [f32; 3], p3: [f32; 3]) -> [Triangle; 2] {
        [tri3(p0, p1, p2), tri3(p0, p2, p3)]
    }

    /// Box-space tab-over-slanted-floor scene in 8x8x8 bounds (the spec's
    /// synthetic ear-over-back): a slanted floor y = 1 + 0.25 z (upward
    /// normal has a -z component, so Back observes its front face), and a
    /// horizontal tab at y = 0.5 over x,z in [2,6].
    fn tab_over_floor() -> TriangleScene {
        let mut triangles = Vec::new();
        triangles.extend(quad4(
            [0.0, 1.0, 0.0],
            [8.0, 1.0, 0.0],
            [8.0, 3.0, 8.0],
            [0.0, 3.0, 8.0],
        ));
        triangles.extend(quad4(
            [2.0, 0.5, 2.0],
            [6.0, 0.5, 2.0],
            [6.0, 0.5, 6.0],
            [2.0, 0.5, 6.0],
        ));
        TriangleScene {
            triangles,
            materials: vec![Material {
                base_color_factor: [1.0, 1.0, 1.0, 1.0],
                base_color_texture: None,
                alpha_cutoff: None,
            }],
        }
    }

    fn captured(scene: &TriangleScene, views: &[CanonicalView]) -> (Vec<CaptureSide>, OwnershipState) {
        let bounds = Bounds::new(8, 8, 8).expect("bounds");
        let lighting = Lighting {
            direction: [0.0, 0.0, -1.0],
            ambient: 1.0,
        };
        let sides: Vec<CaptureSide> = views
            .iter()
            .map(|&view| capture_side(scene, view, bounds, &lighting))
            .collect();
        let continuity: Vec<_> = sides
            .iter()
            .map(|side| side_continuity(&scene.triangles, &side.continuity_view()))
            .collect();
        let ownership = ownership_masks(&sides, &continuity);
        (sides, ownership)
    }

    /// The fixpoint's chart invariant, ban placement, and rescue on the
    /// tab-over-floor scene captured from Top and Back:
    /// - Top keeps the tab intact and is banned from the floor texels
    ///   4-adjacent to the tab across the silhouette (the far endpoints);
    /// - the banned strip behind the tab (z row 6) is reachable and
    ///   front-face-visible from Back, so Back keeps it (rescue);
    /// - the banned strip in front of the tab (z row 1) is beyond Back's
    ///   reach, so it is a hole: Back has no sample of it at all.
    #[test]
    fn fixpoint_bans_far_silhouette_texels_and_rescues_via_back() {
        let scene = tab_over_floor();
        let (sides, ownership) = captured(&scene, &[CanonicalView::Top, CanonicalView::Back]);
        let (top, back) = (&sides[0], &sides[1]);
        let top_at = |x: u32, z: u32| (z * top.width + x) as usize;

        // Tab interior: Top texels (2..=5, 2..=5) kept, never banned.
        for z in 2..=5u32 {
            for x in 2..=5u32 {
                assert!(ownership.kept[0][top_at(x, z)], "tab texel ({x},{z}) kept by Top");
                assert!(!ownership.banned[0][top_at(x, z)], "tab texel ({x},{z}) unbanned");
            }
        }
        // Far strip behind the tab (row z = 6): banned in Top, kept by
        // Back at the texel Back sees the same point through.
        for x in 2..=5u32 {
            let idx = top_at(x, 6);
            assert!(ownership.banned[0][idx], "floor texel ({x},6) banned in Top");
            assert!(!ownership.kept[0][idx], "banned texel ({x},6) not kept");
            let p = top.point_at(x, 6, top.depth[idx]);
            let back_texel = sees_point(back, p).expect("Back observes the far strip");
            assert!(
                ownership.kept[1][back_texel],
                "Back rescues the strip point behind the tab at ({x},6)"
            );
        }
        // Near strip in front of the tab (row z = 1): banned in Top and
        // beyond Back's reach — an honest hole, not a fabricated wall.
        for x in 2..=5u32 {
            let idx = top_at(x, 1);
            assert!(ownership.banned[0][idx], "floor texel ({x},1) banned in Top");
            let p = top.point_at(x, 1, top.depth[idx]);
            assert_eq!(sees_point(back, p), None, "Back cannot reach the near strip");
        }
    }

    /// Cascading bans (spec: "Fixpoint"): the tab scene plus a back-facing
    /// wall at z = 7 spanning x in [2,6], y in [0,2]. Round 1 bans Top's
    /// far strip (tab silhouette); round 2's rescue makes Back keep the
    /// strip — where it is 4-adjacent, across a cut edge, to the wall Back
    /// also keeps (wall depth 1 vs strip depth 2, separated by empty
    /// space); round 3 bans the strip in Back too. End state: the strip is
    /// banned in both observers and kept nowhere — a hole, with the
    /// invariant intact in both charts, after a genuine ban->rescue->ban
    /// cascade.
    #[test]
    fn fixpoint_cascades_bans_through_the_rescuing_side() {
        let mut scene = tab_over_floor();
        scene.triangles.extend(quad4(
            [2.0, 0.0, 7.0],
            [6.0, 0.0, 7.0],
            [6.0, 2.0, 7.0],
            [2.0, 2.0, 7.0],
        ));
        let (sides, ownership) = captured(&scene, &[CanonicalView::Top, CanonicalView::Back]);
        let (top, back) = (&sides[0], &sides[1]);
        let top_at = |x: u32, z: u32| (z * top.width + x) as usize;
        for x in 2..=5u32 {
            let idx = top_at(x, 6);
            assert!(ownership.banned[0][idx], "floor texel ({x},6) banned in Top");
            let p = top.point_at(x, 6, top.depth[idx]);
            let back_texel = sees_point(back, p).expect("Back observes the strip");
            assert!(
                ownership.banned[1][back_texel],
                "the rescued strip is banned in Back by the wall adjacency"
            );
            assert!(!ownership.kept[1][back_texel], "strip kept nowhere");
        }
        // The wall itself stays kept by Back (rows v = 0 and 1, depth 1).
        for v in 0..=1u32 {
            for x in 2..=5u32 {
                // Back texel u for box x: u = 8 - 1 - x (right = (-1,0,0)).
                let u = 7 - x;
                assert!(
                    ownership.kept[1][(v * back.width + u) as usize],
                    "wall texel ({u},{v}) kept by Back"
                );
            }
        }
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p mesh-import --lib fixpoint_bans_far_silhouette_texels_and_rescues_via_back`
Expected: FAIL to compile — `ownership_masks`, `OwnershipState`, `sees_point`, `continuity_view` not found.

- [ ] **Step 3: Implement**

In `crates/mesh-import/src/capture.rs`:

1. `use crate::continuity::{SideContinuity, SideView};` at the top.
   Visibility, granted here once so Task 6's property tests (a different
   module in this crate) compile against these internals: `capture_side`,
   `better_candidates`, `sees_point` become `pub(crate) fn`;
   `BetterCandidate` becomes `pub(crate) struct` with `pub(crate)` fields;
   `CaptureSide`'s `index`, `point_at`, and the new `continuity_view`
   methods become `pub(crate)`.
2. Make `CaptureSide` and all its fields `pub(crate)`; add field:

```rust
    /// Winning triangle index per covered texel (u32::MAX elsewhere),
    /// straight from the rasterizer; continuity's cross-section verdicts
    /// anchor each sample on its own triangle through this.
    pub(crate) winning: Vec<u32>,
```

3. In `capture_side`, alongside the `depth`/`face_normal` fills add `winning[i] = raster.triangle[i];` (buffer initialized `vec![u32::MAX; count]`), and include `winning` in the constructed value.
4. Add to `impl CaptureSide`:

```rust
    pub(crate) fn continuity_view(&self) -> SideView<'_> {
        SideView {
            origin: self.origin,
            right: self.right,
            down: self.down,
            forward: self.forward,
            width: self.width,
            height: self.height,
            h_max: self.h_max,
            depth: &self.depth,
            winning: &self.winning,
        }
    }
```

5. Replace `owning_mask` with the following (move the existing condition-2/3/4 comment blocks from `owning_mask` onto the matching lines of `sees_point`; move the sigma and "always a candidate" comments onto `better_candidates`):

```rust
/// Cross-side reference: candidate side `side` (index into the enabled
/// sides slice) would represent a sample at its texel `index`.
struct BetterCandidate {
    side: usize,
    index: usize,
}

/// Conditions 2-4 of the ownership rule for candidate side `t` and point
/// `p`: reach (quantized, identical to the capture filter), in-bounds
/// projection, and visibility against `t`'s reachability-filtered buffer
/// within the gradient-derived tolerance. Returns the texel of `t` that
/// represents `p`. Candidate collection, the fixpoint's rescue queries,
/// and the property tests all share this one definition of "t sees p".
fn sees_point(t: &CaptureSide, p: [f64; 3]) -> Option<usize> {
    let rel = sub3(p, t.origin);
    let d_t = dot3(rel, t.forward);
    let relief_t = (d_t * RELIEF_UNITS_PER_PIXEL as f64).round() as i64;
    if relief_t > t.h_max {
        return None;
    }
    let u = dot3(rel, t.right);
    let v = dot3(rel, t.down);
    let (tx, ty) = (u.floor(), v.floor());
    if tx < 0.0 || ty < 0.0 || tx >= f64::from(t.width) || ty >= f64::from(t.height) {
        return None;
    }
    let (tex_x, tex_y) = (tx as u32, ty as u32);
    let t_index = t.index(tex_x, tex_y);
    let z = t.depth[t_index];
    if !z.is_finite() {
        return None;
    }
    let grad = local_gradient(&t.depth, t.width, t.height, tex_x, tex_y, z);
    let tol = grad * std::f64::consts::FRAC_1_SQRT_2 + 1.0 / RELIEF_UNITS_PER_PIXEL as f64;
    if d_t > z + tol {
        return None;
    }
    Some(t_index)
}

/// The strictly-better-scoring candidates for one sample of side `s_idx`,
/// plus the sample's own score. "Strictly better" follows the ownership
/// ordering: higher observation score, or equal score with lower
/// canonical rank.
fn better_candidates(
    sides: &[CaptureSide],
    s_idx: usize,
    x: u32,
    y: u32,
) -> (f64, Vec<BetterCandidate>) {
    let s = &sides[s_idx];
    let idx = s.index(x, y);
    let p = s.point_at(x, y, s.depth[idx]);
    let normal = [
        f64::from(s.face_normal[idx][0]),
        f64::from(s.face_normal[idx][1]),
        f64::from(s.face_normal[idx][2]),
    ];
    let sigma = if dot3(normal, s.forward) <= 0.0 { 1.0 } else { -1.0 };
    let own_score = observation_score(sigma, normal, s.forward);
    let own_rank = s.view.rank();
    let mut better = Vec::new();
    for (t_idx, t) in sides.iter().enumerate() {
        if t_idx == s_idx {
            continue;
        }
        let score = observation_score(sigma, normal, t.forward);
        if score <= 0.0 {
            continue;
        }
        if !(score > own_score || (score == own_score && t.view.rank() < own_rank)) {
            continue;
        }
        if let Some(index) = sees_point(t, p) {
            better.push(BetterCandidate { side: t_idx, index });
        }
    }
    (own_score, better)
}

pub(crate) struct OwnershipState {
    pub kept: Vec<Vec<bool>>,
    pub banned: Vec<Vec<bool>>,
}

/// Ownership fixpoint (spec: "Ownership"): resolve keeps by descending
/// score under the current bans, ban the far endpoint of every cut edge
/// with both endpoints kept, repeat. Bans only accumulate, so the loop is
/// bounded by the sample count; the score-ordered sweep makes each
/// resolution deterministic and independent of side iteration order.
pub(crate) fn ownership_masks(
    sides: &[CaptureSide],
    continuity: &[SideContinuity],
) -> OwnershipState {
    struct Sample {
        side: usize,
        index: usize,
        better: Vec<BetterCandidate>,
    }
    let mut samples = Vec::new();
    let mut order: Vec<(f64, usize)> = Vec::new();
    for (s_idx, side) in sides.iter().enumerate() {
        for y in 0..side.height {
            for x in 0..side.width {
                let index = side.index(x, y);
                if !side.depth[index].is_finite() {
                    continue;
                }
                let (own_score, better) = better_candidates(sides, s_idx, x, y);
                order.push((own_score, samples.len()));
                samples.push(Sample {
                    side: s_idx,
                    index,
                    better,
                });
            }
        }
    }
    // Descending own score; ties by canonical rank then texel index keep
    // the sweep total-ordered and deterministic.
    order.sort_by(|a, b| {
        b.0.total_cmp(&a.0)
            .then_with(|| {
                sides[samples[a.1].side]
                    .view
                    .rank()
                    .cmp(&sides[samples[b.1].side].view.rank())
            })
            .then_with(|| samples[a.1].index.cmp(&samples[b.1].index))
    });

    let mut kept: Vec<Vec<bool>> = sides.iter().map(|s| vec![false; s.depth.len()]).collect();
    let mut banned: Vec<Vec<bool>> = sides.iter().map(|s| vec![false; s.depth.len()]).collect();
    let mut rounds = 0usize;
    loop {
        for side_kept in &mut kept {
            side_kept.fill(false);
        }
        for &(_, sample_idx) in &order {
            let sample = &samples[sample_idx];
            if banned[sample.side][sample.index] {
                continue;
            }
            let taken = sample
                .better
                .iter()
                .any(|candidate| kept[candidate.side][candidate.index]);
            kept[sample.side][sample.index] = !taken;
        }
        let mut new_bans = false;
        for (s_idx, side) in sides.iter().enumerate() {
            for y in 0..side.height {
                for x in 0..side.width {
                    for (nx, ny) in [(x + 1, y), (x, y + 1)] {
                        if nx >= side.width || ny >= side.height {
                            continue;
                        }
                        let (i, j) = (side.index(x, y), side.index(nx, ny));
                        if !kept[s_idx][i] || !kept[s_idx][j] {
                            continue;
                        }
                        if continuity[s_idx].connected(x, y, nx, ny) {
                            continue;
                        }
                        // The far endpoint yields: the near texel's edge is
                        // its surface's true silhouette from this view; the
                        // far surface continues underneath, which is what
                        // other sides can still observe. Exact depth ties
                        // (two disconnected surfaces at equal depth) need
                        // only determinism: the larger index yields.
                        let far = if side.depth[i] > side.depth[j] {
                            i
                        } else if side.depth[j] > side.depth[i] {
                            j
                        } else {
                            i.max(j)
                        };
                        if !banned[s_idx][far] {
                            banned[s_idx][far] = true;
                            new_bans = true;
                        }
                    }
                }
            }
        }
        if !new_bans {
            break;
        }
        rounds += 1;
        assert!(
            rounds <= samples.len(),
            "ownership fixpoint failed to terminate"
        );
    }
    OwnershipState { kept, banned }
}
```

6. Do NOT yet change `convert_box_space` (Task 6 rewires it). `owning_mask` is deleted here; to keep the crate compiling, temporarily change `convert_box_space`'s pass 2 to use the new fixpoint with per-side continuity but leave passes 3 (cuts) intact — no: that mixes semantics. Instead, keep compilation honest by doing the minimal true wiring now and the *removal* of cuts in Task 6:

In `convert_box_space`, replace pass 2's body:

```rust
    // Pass 2: continuity labels + ownership fixpoint + closure ring.
    let continuity: Vec<SideContinuity> = sides
        .iter()
        .map(|side| crate::continuity::side_continuity(&box_scene.triangles, &side.continuity_view()))
        .collect();
    let ownership = ownership_masks(&sides, &continuity);
    let mut masks: Vec<Vec<bool>> = Vec::with_capacity(sides.len());
    for (s_idx, side) in sides.iter().enumerate() {
        let covered: Vec<bool> = side.depth.iter().map(|d| d.is_finite()).collect();
        masks.push(dilate_keep_mask(
            &ownership.kept[s_idx],
            &covered,
            side.width,
            side.height,
        ));
    }
```

(The old `dilate_keep_mask` signature and the cut pass still run at this point; both are replaced in Tasks 5–6. Behavior in this intermediate state is not shipped — the suite must still pass, which it does because bans only remove texels the cut pass would otherwise handle and the two integration cut tests' expectations are unchanged by bans-with-no-rescuing-sides.)

- [ ] **Step 4: Run tests**

Run: `cargo test -p mesh-import`
Expected: new fixpoint test PASSES; all existing tests PASS. If `occlusion_cut_drops_the_far_strip` fails at this intermediate step, STOP and re-examine — with only Front enabled, bans reproduce exactly the ring the cut pass produced, so a failure means the fixpoint logic is wrong, not the test.

- [ ] **Step 5: Commit**

```bash
git add crates/mesh-import/src/capture.rs
git commit -m "feat: ownership fixpoint with silhouette bans and cross-side rescue"
```

---

### Task 5: Edge-aware closure and the post-closure sweep

**Files:**
- Modify: `crates/mesh-import/src/capture.rs`

**Interfaces:**
- Consumes: `SideContinuity` (Task 3), `OwnershipState` (Task 4).
- Produces (consumed by Task 6):
  - `pub(crate) struct ClosureMask { pub mask: Vec<bool>, pub support: Vec<bool> }`
  - `pub(crate) fn dilate_keep_mask(kept: &[bool], covered: &[bool], banned: &[bool], continuity: &SideContinuity, width: u32, height: u32) -> ClosureMask`
  - `pub(crate) fn enforce_closure_invariant(depth: &[f64], continuity: &SideContinuity, closure: &mut ClosureMask, width: u32, height: u32)`

- [ ] **Step 1: Update and extend the failing tests**

In `capture.rs`'s test module, rewrite the existing `dilate_keep_mask_adds_covered_orthogonal_neighbors_only` to the new signature and add the new cases (keep the existing plus-shape scenario and assertions, passing `banned = vec![false; 25]` and `SideContinuity::uniform(5, 5, true)`, asserting on `.mask`), then add:

```rust
    use crate::continuity::SideContinuity;
    use super::{ClosureMask, dilate_keep_mask, enforce_closure_invariant};

    /// Dilation must not cross a cut edge and must never re-add a banned
    /// texel: 1x3 row [kept, banned-covered, covered], where the
    /// (0)-(1) edge is continuous and (1)-(2) is cut. Neither neighbor
    /// may be added: (1) is banned, (2) is only reachable across a cut.
    #[test]
    fn dilation_respects_cut_edges_and_bans() {
        let kept = vec![true, false, false];
        let covered = vec![true, true, true];
        let banned = vec![false, true, false];
        let continuity = SideContinuity::from_edges(3, 1, vec![true, false], vec![]);
        let closure = dilate_keep_mask(&kept, &covered, &banned, &continuity, 3, 1);
        assert_eq!(closure.mask, vec![true, false, false]);
        assert_eq!(closure.support, vec![false, false, false]);
    }

    /// Post-closure sweep: a support texel across a cut edge from a kept
    /// texel is dropped; a support-support cut pair drops its far
    /// endpoint; continuous pairs are untouched.
    #[test]
    fn post_closure_sweep_drops_support_across_cut_edges() {
        // Row of 4: [kept, support, support, kept]; edges: (0)-(1)
        // continuous, (1)-(2) continuous, (2)-(3) cut.
        let mut closure = ClosureMask {
            mask: vec![true, true, true, true],
            support: vec![false, true, true, false],
        };
        let continuity = SideContinuity::from_edges(4, 1, vec![true, true, false], vec![]);
        let depth = vec![1.0, 1.0, 1.0, 5.0];
        enforce_closure_invariant(&depth, &continuity, &mut closure, 4, 1);
        // (2) is support across the cut from kept (3): dropped. (3) kept.
        assert_eq!(closure.mask, vec![true, true, false, true]);

        // Support-support across a cut: the far endpoint yields.
        let mut closure = ClosureMask {
            mask: vec![true, true, true, true],
            support: vec![false, true, true, false],
        };
        let continuity = SideContinuity::from_edges(4, 1, vec![true, false, true], vec![]);
        let depth = vec![1.0, 1.0, 5.0, 5.0];
        enforce_closure_invariant(&depth, &continuity, &mut closure, 4, 1);
        assert_eq!(closure.mask, vec![true, true, false, true]);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p mesh-import --lib dilat`
Expected: FAIL to compile (new signature/types missing).

- [ ] **Step 3: Implement**

Replace `dilate_keep_mask` and add the sweep in `capture.rs`:

```rust
pub(crate) struct ClosureMask {
    pub mask: Vec<bool>,
    pub support: Vec<bool>,
}

/// One-texel closure ring (spec: "Closure and the post-closure sweep"):
/// tent interpolation ends at the alpha-zero boundary, so abutting regions
/// of different charts need one texel of true-geometry support to meet
/// without sub-texel gaps. Dilation crosses only continuous edges (a cut
/// edge is a silhouette; bridging it is the bug this design removes) and
/// never re-adds a banned texel (that would recreate the wall its ban
/// removed, even via a continuous edge from another direction).
pub(crate) fn dilate_keep_mask(
    kept: &[bool],
    covered: &[bool],
    banned: &[bool],
    continuity: &SideContinuity,
    width: u32,
    height: u32,
) -> ClosureMask {
    let index = |x: u32, y: u32| (y * width + x) as usize;
    let mut mask = kept.to_vec();
    let mut support = vec![false; kept.len()];
    for y in 0..height {
        for x in 0..width {
            let idx = index(x, y);
            if kept[idx] || !covered[idx] || banned[idx] {
                continue;
            }
            let joins = (x > 0 && kept[index(x - 1, y)] && continuity.connected(x - 1, y, x, y))
                || (x + 1 < width && kept[index(x + 1, y)] && continuity.connected(x, y, x + 1, y))
                || (y > 0 && kept[index(x, y - 1)] && continuity.connected(x, y - 1, x, y))
                || (y + 1 < height && kept[index(x, y + 1)] && continuity.connected(x, y, x, y + 1));
            if joins {
                mask[idx] = true;
                support[idx] = true;
            }
        }
    }
    ClosureMask { mask, support }
}

/// Post-closure invariant sweep: dilation can still place support across a
/// cut edge from another surface's texels (staircase silhouettes). Drops
/// are collected first and applied after the scan, so verdicts are
/// independent of scan order. Support always yields: a covered, unbanned,
/// unkept texel exists only because a strictly better side keeps its point
/// (the fixpoint condition), so support is redundant geometry and dropping
/// it never loses surface. A kept-kept cut pair cannot occur here — the
/// fixpoint terminated without violations and closure adds no kept texels
/// — but release builds restore the invariant anyway rather than emit a
/// fabricated wall.
pub(crate) fn enforce_closure_invariant(
    depth: &[f64],
    continuity: &SideContinuity,
    closure: &mut ClosureMask,
    width: u32,
    height: u32,
) {
    let index = |x: u32, y: u32| (y * width + x) as usize;
    let mut drop = vec![false; closure.mask.len()];
    for y in 0..height {
        for x in 0..width {
            for (nx, ny) in [(x + 1, y), (x, y + 1)] {
                if nx >= width || ny >= height {
                    continue;
                }
                let (i, j) = (index(x, y), index(nx, ny));
                if !closure.mask[i] || !closure.mask[j] || continuity.connected(x, y, nx, ny) {
                    continue;
                }
                let far = if depth[i] > depth[j] {
                    i
                } else if depth[j] > depth[i] {
                    j
                } else {
                    i.max(j)
                };
                match (closure.support[i], closure.support[j]) {
                    (true, false) => drop[i] = true,
                    (false, true) => drop[j] = true,
                    (true, true) => drop[far] = true,
                    (false, false) => {
                        debug_assert!(false, "kept-kept cut pair survived the ownership fixpoint");
                        drop[far] = true;
                    }
                }
            }
        }
    }
    for (idx, &dropped) in drop.iter().enumerate() {
        if dropped {
            closure.mask[idx] = false;
            closure.support[idx] = false;
        }
    }
}
```

Update the call in `convert_box_space` (still followed by the old cut pass until Task 6):

```rust
        let closure = dilate_keep_mask(
            &ownership.kept[s_idx],
            &covered,
            &ownership.banned[s_idx],
            &continuity[s_idx],
            side.width,
            side.height,
        );
        masks.push(closure.mask);
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p mesh-import`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/mesh-import/src/capture.rs
git commit -m "feat: closure dilates only continuous unbanned edges; post-closure sweep"
```

---

### Task 6: Wire the pipeline, delete the cut pass, land the bug-first regression tests

**Files:**
- Modify: `crates/mesh-import/src/capture.rs`, `crates/mesh-import/src/lib.rs`
- Delete: `crates/mesh-import/src/cuts.rs`
- Create: `crates/mesh-import/src/property_tests.rs`
- Modify: `crates/mesh-import/tests/capture.rs` (two cut-pass tests' comments/assertions)
- Modify: `Cargo.toml` (workspace root — test-profile optimization for mesh-import)

**Interfaces:**
- Consumes: everything from Tasks 1–5.
- Produces: `pub(crate) struct CapturePipeline { pub sides: Vec<CaptureSide>, pub continuity: Vec<SideContinuity>, pub ownership: OwnershipState, pub masks: Vec<ClosureMask> }` and `pub(crate) fn run_capture(box_scene: &TriangleScene, bounds: Bounds, settings: &ImportSettings) -> CapturePipeline`, used by `convert_box_space` and the property tests.

- [ ] **Step 1: Write the bug-first regression tests and observe them RED**

**(a) Seam/rescue regression (public API)** — append to `crates/mesh-import/tests/capture.rs` (uses that file's existing `tri`, `plain_material`, `settings`, `IDENTITY` helpers):

```rust
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
    let quad = |p0: [f32; 3], p1: [f32; 3], p2: [f32; 3], p3: [f32; 3]| {
        [tri(p0, p1, p2), tri(p0, p2, p3)]
    };
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
        (model.bounds().width(), model.bounds().height(), model.bounds().depth()),
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
```

**(b) Fabricated-adjacency oracle regression (internal)** — create `crates/mesh-import/src/property_tests.rs` and register `#[cfg(test)] mod property_tests;` in `lib.rs`:

```rust
//! Whole-pipeline properties on the real GLB fixtures, checked with the
//! continuity labels as an independent oracle against the emitted charts.

#![cfg(test)]

use std::path::PathBuf;

use relief_core::CanonicalView;

use crate::capture::{ALL_VIEWS, run_capture};
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
```

(`run_capture` does not exist yet; the oracle test compiles only after Step 2's wiring. To observe (b) RED against the OLD pipeline first, temporarily write `assert_no_fabricated_adjacency` without the `run_capture` line, iterating `ALL_VIEWS` and building each side via `crate::capture::capture_side` — the exact form below in this step — run it, record the failure, then finish Step 2 and restore the final form above.)

RED observation form of the oracle (temporary, delete after observing):

```rust
    // TEMPORARY RED-OBSERVATION FORM — replaced by the run_capture form
    // in the same task, before commit.
    let lighting = crate::Lighting {
        direction: crate::light_direction(
            settings.light_azimuth_degrees,
            settings.light_elevation_degrees,
        ),
        ambient: settings.ambient,
    };
    for view in ALL_VIEWS {
        let side = crate::capture::capture_side(&box_scene, view, bounds, &lighting);
        let labels = side_continuity(&box_scene.triangles, &side.continuity_view());
        let chart = model.chart(view).expect("chart present");
        /* identical pair loop as above */
    }
```

Run: `cargo test -p mesh-import relinquished_silhouette_strip_is_rescued_by_back`
Expected: FAIL — Back chart texels at v=2 are `[0,0,0,0]` (the strip is dropped by everyone: the seam bug).

Run: `cargo test -p mesh-import bunny_charts_contain_no_fabricated_adjacency`
Expected: FAIL — fabricated adjacency reported in the Top chart near the ear (the spike bug). Record the failing coordinates in the task report.

- [ ] **Step 2: Wire the pipeline and delete the cut pass**

1. In `capture.rs`, add:

```rust
pub(crate) struct CapturePipeline {
    pub sides: Vec<CaptureSide>,
    pub continuity: Vec<SideContinuity>,
    pub ownership: OwnershipState,
    pub masks: Vec<ClosureMask>,
}

pub(crate) fn run_capture(
    box_scene: &TriangleScene,
    bounds: Bounds,
    settings: &ImportSettings,
) -> CapturePipeline {
    let lighting = Lighting {
        direction: light_direction(
            settings.light_azimuth_degrees,
            settings.light_elevation_degrees,
        ),
        ambient: settings.ambient,
    };
    let sides: Vec<CaptureSide> = ALL_VIEWS
        .into_iter()
        .filter(|&view| settings.side_modes.get(view) == SideMode::Capture)
        .map(|view| capture_side(box_scene, view, bounds, &lighting))
        .collect();
    let continuity: Vec<SideContinuity> = sides
        .iter()
        .map(|side| crate::continuity::side_continuity(&box_scene.triangles, &side.continuity_view()))
        .collect();
    let ownership = ownership_masks(&sides, &continuity);
    let masks: Vec<ClosureMask> = sides
        .iter()
        .enumerate()
        .map(|(s_idx, side)| {
            let covered: Vec<bool> = side.depth.iter().map(|d| d.is_finite()).collect();
            let mut closure = dilate_keep_mask(
                &ownership.kept[s_idx],
                &covered,
                &ownership.banned[s_idx],
                &continuity[s_idx],
                side.width,
                side.height,
            );
            enforce_closure_invariant(
                &side.depth,
                &continuity[s_idx],
                &mut closure,
                side.width,
                side.height,
            );
            debug_assert_chart_invariant(&closure.mask, &continuity[s_idx], side.width, side.height);
            closure
        })
        .collect();
    CapturePipeline {
        sides,
        continuity,
        ownership,
        masks,
    }
}

#[cfg(debug_assertions)]
fn debug_assert_chart_invariant(
    mask: &[bool],
    continuity: &SideContinuity,
    width: u32,
    height: u32,
) {
    let index = |x: u32, y: u32| (y * width + x) as usize;
    for y in 0..height {
        for x in 0..width {
            for (nx, ny) in [(x + 1, y), (x, y + 1)] {
                if nx >= width || ny >= height {
                    continue;
                }
                debug_assert!(
                    !(mask[index(x, y)] && mask[index(nx, ny)])
                        || continuity.connected(x, y, nx, ny),
                    "emitted mask violates the chart invariant at ({x},{y})-({nx},{ny})"
                );
            }
        }
    }
}

#[cfg(not(debug_assertions))]
fn debug_assert_chart_invariant(_: &[bool], _: &SideContinuity, _: u32, _: u32) {}
```

2. Rewrite `convert_box_space` to use it (replacing passes 1–3 wholesale):

```rust
pub fn convert_box_space(
    box_scene: &TriangleScene,
    bounds: Bounds,
    settings: &ImportSettings,
) -> Result<AuthoredModel, ImportError> {
    settings.side_modes.validate()?;
    let pipeline = run_capture(box_scene, bounds, settings);
    let mut charts = Vec::new();
    for (side, closure) in pipeline.sides.iter().zip(pipeline.masks.iter()) {
        let rgba: Vec<[u8; 4]> = side
            .rgba
            .iter()
            .zip(closure.mask.iter())
            .map(|(&texel, &keep)| if keep { texel } else { [0, 0, 0, 0] })
            .collect();
        let mut chart = Chart::from_rgba(side.view, side.width, side.height, rgba)?;
        let opposite_mode = settings.side_modes.get(side.view.opposite());
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

3. Delete `apply_fabricated_wall_cuts`, `CUT_CANDIDATE_UNITS`, the `use crate::cuts::TriangleGrid;` import, and `crates/mesh-import/src/cuts.rs`; remove `mod cuts;` from `lib.rs`. Make `capture_side` and `ALL_VIEWS` reachable for the property tests (`pub(crate)` — `ALL_VIEWS` is already `pub` via lib re-export). Restore the oracle test to its `run_capture` form.
4. Also add the internal coverage theorem to `property_tests.rs`:

```rust
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
                    if pipeline.ownership.kept[s_idx][idx]
                        || pipeline.ownership.banned[s_idx][idx]
                    {
                        continue;
                    }
                    let (_, better) = crate::capture::better_candidates(
                        &pipeline.sides,
                        s_idx,
                        x,
                        y,
                    );
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
```

(`better_candidates` and `BetterCandidate` are already `pub(crate)` from Task 4.)

5. Update the two existing integration tests in `tests/capture.rs`:
   - `occlusion_cut_drops_the_far_strip`: delete the `floor_relief - plate_relief > 10 ... cut candidate threshold` assertion (no threshold exists any more); update its doc comment's last paragraph to: "Plate and floor are 4-adjacent across the plate's silhouette with real empty space between them, so the pair is cut by the continuity verdict; with only Front enabled no side can rescue the ring, which stays empty." All texel expectations are unchanged.
   - `real_step_wall_is_kept`: delete its `> 10` threshold assertion; update the doc comment to say the connecting wall's cross-section joins the two shelves, so the pair is continuous and both shelves stay covered. Texel expectations unchanged.
6. Add to the root `Cargo.toml` under the existing `[profile.test.package.*]` entries (the fixture property tests capture 100k-triangle meshes; same justification as the existing entries):

```toml
[profile.test.package.mesh-import]
opt-level = 2
```

- [ ] **Step 3: Run the full suite**

Run: `cargo test -p mesh-import`
Expected: ALL PASS, including both Step 1 regressions now GREEN and the updated integration tests.

Run: `cargo test --workspace`
Expected: ALL PASS (desktop-app dialog tests exercise `convert` through the same public API).

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: silhouette-continuity ownership replaces the fabricated-wall cut pass"
```

---

### Task 7: Determinism property — rotation symmetry

**Files:**
- Modify: `crates/mesh-import/src/property_tests.rs`

**Interfaces:** consumes public `convert` only.

- [ ] **Step 1: Write the test**

The bunny's AABB is not a cube (63x63x49 scales to 8x8x7 at longest 8), so the test uses the synthetic cube-bounded tab-over-floor scene, whose asymmetric silhouettes and rescues still exercise the full pipeline. All six sides stay at their default Capture mode in both runs; only Top charts are compared.

```rust
/// No hidden order-dependence: rotating the model a quarter turn about
/// the vertical axis must produce the correspondingly rotated Top chart.
/// With R = quarter turn about y, (x,y,z) -> (z, y, -x), and cube bounds
/// n = 8, rotated box coords are X = Z0, Y = Y0, Z = n - X0, so
/// rotatedTop(u', v') == originalTop(u = n-1-v', v = u'). Only alpha
/// (relief) is compared: RGB carries box-frame lighting, which
/// legitimately differs between the two orientations.
#[test]
fn quarter_turn_rotation_maps_the_top_chart() {
    let tri3 = |a: [f32; 3], b: [f32; 3], c: [f32; 3]| crate::Triangle {
        positions: [a, b, c],
        normals: [[0.0, -1.0, 0.0]; 3],
        uvs: [[0.0, 0.0]; 3],
        colors: [[1.0, 1.0, 1.0, 1.0]; 3],
        material: 0,
    };
    let quad4 = |p0: [f32; 3], p1: [f32; 3], p2: [f32; 3], p3: [f32; 3]| {
        [tri3(p0, p1, p2), tri3(p0, p2, p3)]
    };
    let mut triangles = Vec::new();
    // Mesh-space tab-over-slanted-floor inside the unit cube (identical
    // geometry to the rescue regression in tests/capture.rs).
    triangles.extend(quad4(
        [0.0, 0.125, 0.0],
        [1.0, 0.125, 0.0],
        [1.0, 0.375, 1.0],
        [0.0, 0.375, 1.0],
    ));
    triangles.extend(quad4(
        [0.25, 0.0625, 0.25],
        [0.75, 0.0625, 0.25],
        [0.75, 0.0625, 0.75],
        [0.25, 0.0625, 0.75],
    ));
    triangles.push(tri3([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0001, 0.5, 0.0]));
    triangles.push(tri3([1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.9999, 0.5, 1.0]));
    let scene = TriangleScene {
        triangles,
        materials: vec![crate::Material {
            base_color_factor: [1.0, 1.0, 1.0, 1.0],
            base_color_texture: None,
            alpha_cutoff: None,
        }],
    };
    const IDENTITY: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    const QUARTER_TURN_Y: [[f32; 3]; 3] = [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]];
    let settings = |rotation| ImportSettings {
        rotation,
        longest_axis_pixels: 8,
        ..Default::default()
    };
    let original = crate::convert(&scene, &settings(IDENTITY)).expect("converts");
    let rotated = crate::convert(&scene, &settings(QUARTER_TURN_Y)).expect("converts");
    let n = 8u32;
    for model in [&original, &rotated] {
        let b = model.bounds();
        assert_eq!((b.width(), b.height(), b.depth()), (n, n, n), "cube bounds");
    }
    let top_a = original.chart(CanonicalView::Top).expect("top");
    let top_b = rotated.chart(CanonicalView::Top).expect("top");
    for v in 0..n {
        for u in 0..n {
            let b = top_b.rgba()[(v * n + u) as usize][3];
            let a = top_a.rgba()[(u * n + (n - 1 - v)) as usize][3];
            assert_eq!(a, b, "Top alpha mismatch at rotated ({u},{v})");
        }
    }
}
```

Note the index identity: `rotatedTop(u', v') == originalTop(u = n-1-v', v = u')`, and original chart storage is row-major `v * n + u`, hence `top_a` index `(u' * n + (n - 1 - v'))`.

- [ ] **Step 2: Run**

Run: `cargo test -p mesh-import quarter_turn_rotation_maps_the_top_chart`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/mesh-import/src/property_tests.rs
git commit -m "test: quarter-turn rotation symmetry pins capture determinism"
```

---

### Task 8: Capture benchmark

**Files:**
- Modify: `crates/mesh-import/Cargo.toml`
- Create: `crates/mesh-import/benches/capture.rs`

- [ ] **Step 1: Add the bench**

Append to `crates/mesh-import/Cargo.toml`:

```toml
[dev-dependencies]
criterion = { version = "=0.5.1", default-features = false, features = ["cargo_bench_support"] }

[[bench]]
name = "capture"
harness = false
```

(Merge into the existing `[dev-dependencies]` table — do not create a duplicate table.)

Create `crates/mesh-import/benches/capture.rs`:

```rust
//! Full-conversion benchmark on the committed GLB fixtures. The import
//! dialog re-captures during Ctrl+drag throttled to the frame rate; the
//! budget of record is ~21 ms for a six-side 63-pixel capture (spec:
//! "Rasterizer" in 2026-07-17-model-import-design.md).

use criterion::{Criterion, criterion_group, criterion_main};
use mesh_import::{ImportSettings, convert, load_scene};
use std::path::PathBuf;

fn bench_capture(c: &mut Criterion) {
    for name in ["stanford-bunny.glb", "xyzrgb_dragon.glb"] {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        assert!(path.exists(), "Missing fixture {}", path.display());
        let scene = load_scene(&path).expect("fixture loads");
        let settings = ImportSettings::default();
        c.bench_function(&format!("convert {name}"), |b| {
            b.iter(|| convert(&scene, &settings).expect("convert"));
        });
    }
}

criterion_group!(benches, bench_capture);
criterion_main!(benches);
```

- [ ] **Step 2: Run and record**

Run: `cargo bench -p mesh-import`
Expected: completes; record both mean times in the task report. Compare against the ~21 ms interactive budget. If the dragon exceeds the budget, DO NOT optimize speculatively and DO NOT relax the algorithm — report the numbers to the user and stop for direction (the design's cost profile was accepted with a benchmark gate).

- [ ] **Step 3: Commit**

```bash
git add crates/mesh-import/Cargo.toml crates/mesh-import/benches/capture.rs
git commit -m "bench: full-conversion capture benchmark on bunny and dragon"
```

---

### Task 9: Documentation and final gates

**Files:**
- Modify: `docs/superpowers/specs/2026-07-17-model-import-design.md`

- [ ] **Step 1: Rewrite the two superseded sections**

Replace the "Surface ownership" section body with:

```markdown
Full projections would store one surface region in up to six charts, so the
same feature would be drawn several times and captured obliquely by sides
that barely face it. Instead, each observed surface point is kept by the
best side that can legally hold it:

- Every hit records the face normal `n̂` of its triangle and the observation
  orientation `σ = sign(−n̂ · axis_S)`. Candidate owners for a hit are the
  enabled Capture sides observing the same oriented face, reaching the
  point, and seeing it within a gradient-derived tolerance of their own
  filtered depth buffer; the capturing side is always a candidate for its
  own hit. Preference is by observation score `σ · (−n̂ · axis_T)` (most
  head-on wins; exact ties resolve by canonical rank).
- Every 4-adjacency between covered texels of a side carries an exact
  continuity label, computed from the mesh cross-section in the vertical
  plane through the two texel centers restricted to the strip between them:
  the pair is connected iff both samples lie on one polyline component,
  with sub-half-quantum gaps closed and every path point within the side's
  reach. Occlusion of the in-between surface is irrelevant — a bridge
  behind nearer geometry composites correctly via transient depth; a
  bridge through empty space is a fabricated wall.
- Ownership is a fixpoint honoring the chart invariant that no chart keeps
  both endpoints of a cut edge: keeps resolve in descending score order
  (a side keeps its covered, unbanned texel iff no strictly better
  candidate currently keeps the point); the far endpoint of every violated
  cut edge is banned for that side; banned surface is re-owned by the next
  best observing side. A point is lost only when no enabled side observes
  it. Bans only accumulate, so the fixpoint terminates.
- After ownership, each side dilates its kept texels by one texel into
  covered, unbanned neighbors across continuous edges only — the support
  tent interpolation needs to meet the neighboring chart. A final sweep
  drops any support texel that lands across a cut edge (support is
  redundant by construction), so emitted charts satisfy the invariant:
  no two 4-adjacent covered texels are joined by a cut edge.
```

Delete the "Fabricated-wall cuts" section entirely (its **Color** paragraph and everything after belong to the following sections — keep them; only the cut-pass prose goes). Update the Testing section's synthetic/real-model bullets that reference the cut pass to reference the continuity labels and the two regression properties (fabricated adjacency, rescue/coverage).

- [ ] **Step 2: Full workspace gates**

Run, in order, expecting clean output from each:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/specs/2026-07-17-model-import-design.md
git commit -m "docs: import spec reflects silhouette-continuity ownership"
```

- [ ] **Step 4: Report for visual acceptance**

Final acceptance is the user's: import the bunny in the dialog and confirm no spikes from the back into the ear and no seam ring around the silhouette. Report completion with the bench numbers and the RED→GREEN evidence for both regression tests; do not claim the visual outcome — ask the user to verify it.
```
