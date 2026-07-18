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

use crate::Triangle;
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
    debug_assert!(
        count <= 2,
        "non-coplanar triangle produced {count} plane points"
    );
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
        (seg.a[0], dt),          // t >= 0
        (1.0 - seg.a[0], -dt),   // t <= 1
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
///
/// Precondition: callers must already have handled the `win0 == win1` case
/// (same winning triangle implies connected without walking the join graph)
/// before gathering segments and calling this function.
fn pair_connected(segments: &[CrossSegment], d0: f64, win0: u32, d1: f64, win1: u32) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Lighting, Material, Triangle, TriangleScene, View, rasterize};
    use relief_core::RELIEF_UNITS_PER_PIXEL as UNITS;

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
        assert!(
            s.a == [3.0, 5.0] || s.b == [3.0, 5.0],
            "on-plane vertex kept"
        );
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
    fn side_labels(
        triangles: Vec<Triangle>,
        width: u32,
        height: u32,
        h_max: i64,
    ) -> SideContinuity {
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
            assert!(
                labels.connected(x, 0, x + 1, 0),
                "fold edge ({x})-({})",
                x + 1
            );
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
        assert!(
            labels.connected(1, 0, 2, 0),
            "sliver-occluded same-surface pair"
        );
    }

    /// A silhouette clipping the corner of the segment between two texels
    /// that both sample the far surface: connected, even though the
    /// clipping surface is real and resolved elsewhere in the chart. The far
    /// surface is deliberately split into two coplanar rects sharing the
    /// x = 2 edge so the two row-0 samples provably land on different
    /// triangles — the win0 == win1 fast path cannot fire, forcing the
    /// verdict to walk the join graph across the shared edge under the
    /// clipping spike.
    #[test]
    fn corner_clip_between_same_surface_samples_is_connected() {
        let mut triangles = Vec::new();
        triangles.extend(rect(-1.0, 2.0, -1.0, 3.0, 5.0)); // far, left half
        triangles.extend(rect(2.0, 5.0, -1.0, 3.0, 5.0)); // far, right half
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
        assert!(
            !labels.connected(1, 0, 2, 0),
            "no surface between the towers"
        );
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
}
