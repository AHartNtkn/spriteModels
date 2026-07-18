//! Exact per-pair surface-continuity labels for capture (spec:
//! docs/superpowers/specs/2026-07-18-silhouette-continuity-ownership-design.md).
//!
//! For two 8-adjacent (orthogonally or diagonally touching) covered texels
//! of one side, the verdict answers: is the tent bridge between their two
//! samples backed by continuous, reachable surface? Diagonal pairs need a
//! label too because the renderer's tent kernel blends every texel of a
//! 4-connected component within its support, which reaches diagonal
//! centers — a diagonal contact across a silhouette fabricates surface
//! exactly like an orthogonal one. The verdict is decided on the mesh
//! cross-section in the vertical plane through the two texel centers,
//! restricted to the strip between them. Occlusion of the in-between
//! surface is deliberately irrelevant: a bridge behind nearer geometry
//! composites correctly via transient depth; only a bridge through empty
//! space fabricates surface.

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

/// Liang-Barsky clip of a cross segment to the strip `t in [0, t_max]` and
/// the reachable half-space `d <= reach`. Orthogonal edges pass `t_max =
/// 1.0`; a diagonal strip passes `t_max = SQRT_2`. `t` and `d` stay in
/// texel units (never rescaled), so `JOIN_GAP` distances remain meaningful
/// on both kinds of edge. Returns None when nothing remains.
pub(crate) fn clip_segment(seg: CrossSegment, t_max: f64, reach: f64) -> Option<CrossSegment> {
    let dt = seg.b[0] - seg.a[0];
    let dd = seg.b[1] - seg.a[1];
    let (mut s0, mut s1) = (0.0f64, 1.0f64);
    // Each constraint keeps points with num + s * den >= 0.
    for (num, den) in [
        (seg.a[0], dt),          // t >= 0
        (t_max - seg.a[0], -dt), // t <= t_max
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
///
/// Diagonal labels exist because the renderer's tent kernel blends every
/// texel of a 4-connected component within its support, which reaches
/// diagonal centers: two diagonally-touching kept texels straddling a
/// silhouette blend across free space at their shared corner exactly as an
/// orthogonal pair would.
pub(crate) struct SideContinuity {
    width: u32,
    height: u32,
    /// (width-1) * height entries; index y * (width-1) + x labels the edge
    /// (x, y)-(x+1, y).
    horizontal: Vec<bool>,
    /// width * (height-1) entries; index y * width + x labels the edge
    /// (x, y)-(x, y+1).
    vertical: Vec<bool>,
    /// (width-1) * (height-1) entries; index y * (width-1) + x labels the
    /// edge (x, y)-(x+1, y+1).
    diag_down_right: Vec<bool>,
    /// (width-1) * (height-1) entries; index y * (width-1) + x labels the
    /// edge (x+1, y)-(x, y+1).
    diag_down_left: Vec<bool>,
}

impl SideContinuity {
    pub(crate) fn connected(&self, ax: u32, ay: u32, bx: u32, by: u32) -> bool {
        let (x0, y0) = (ax.min(bx), ay.min(by));
        if ay == by {
            debug_assert!(ax.abs_diff(bx) == 1 && x0 + 1 < self.width && ay < self.height);
            self.horizontal[(y0 * (self.width - 1) + x0) as usize]
        } else if ax == bx {
            debug_assert!(ay.abs_diff(by) == 1 && ax < self.width && y0 + 1 < self.height);
            self.vertical[(y0 * self.width + x0) as usize]
        } else {
            debug_assert!(
                ax.abs_diff(bx) == 1
                    && ay.abs_diff(by) == 1
                    && x0 + 1 < self.width
                    && y0 + 1 < self.height
            );
            let idx = (y0 * (self.width - 1) + x0) as usize;
            // The endpoint at x = x0 sits at y = y0 for a down-right edge
            // ((x0,y0)-(x0+1,y0+1)) and at y = y0+1 for a down-left edge
            // ((x0+1,y0)-(x0,y0+1)); order-insensitive in (ax,ay)/(bx,by).
            let y_at_x0 = if ax == x0 { ay } else { by };
            if y_at_x0 == y0 {
                self.diag_down_right[idx]
            } else {
                self.diag_down_left[idx]
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn uniform(width: u32, height: u32, value: bool) -> Self {
        Self {
            width,
            height,
            horizontal: vec![value; ((width - 1) * height) as usize],
            vertical: vec![value; (width * (height - 1)) as usize],
            diag_down_right: vec![value; ((width - 1) * (height - 1)) as usize],
            diag_down_left: vec![value; ((width - 1) * (height - 1)) as usize],
        }
    }

    #[cfg(test)]
    pub(crate) fn from_edges(
        width: u32,
        height: u32,
        horizontal: Vec<bool>,
        vertical: Vec<bool>,
        diag_down_right: Vec<bool>,
        diag_down_left: Vec<bool>,
    ) -> Self {
        assert_eq!(horizontal.len(), ((width - 1) * height) as usize);
        assert_eq!(vertical.len(), (width * (height - 1)) as usize);
        let diag_len = ((width - 1) * (height - 1)) as usize;
        assert_eq!(diag_down_right.len(), diag_len);
        assert_eq!(diag_down_left.len(), diag_len);
        Self {
            width,
            height,
            horizontal,
            vertical,
            diag_down_right,
            diag_down_left,
        }
    }
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Per-triangle projected extrema, one array per coordinate so the
/// line-filter and strip-window tests — random-access lookups by triangle
/// index — each touch a dense 16-byte entry instead of one wide struct:
/// `u`, `v` (the AABB axes) and `u+v`, `v-u` (the diagonal pairs'
/// plane/strip coordinates, taken per vertex so they bound the true
/// diagonal extent, not the looser box corners). Every entry is
/// `[lo, hi]`.
struct TriBounds {
    u: Vec<[f64; 2]>,
    v: Vec<[f64; 2]>,
    upv: Vec<[f64; 2]>,
    vmu: Vec<[f64; 2]>,
}

/// Per-texel triangle buckets from projected AABBs, clamped to the grid. A
/// zero-area projection (an edge-on wall) still lands in its column — such
/// triangles contribute no raster coverage but do carry cross-section
/// surface, exactly the case a real step wall depends on. Also returns the
/// per-triangle bounds the pair loop uses to skip triangles that provably
/// contribute no clipped cross-section segment.
fn buckets(projected: &[[[f64; 3]; 3]], width: u32, height: u32) -> (Vec<Vec<u32>>, TriBounds) {
    let mut cells = vec![Vec::new(); (width * height) as usize];
    let mut bounds = TriBounds {
        u: Vec::with_capacity(projected.len()),
        v: Vec::with_capacity(projected.len()),
        upv: Vec::with_capacity(projected.len()),
        vmu: Vec::with_capacity(projected.len()),
    };
    let clamp = |value: f64, dim: u32| (value.floor() as i64).clamp(0, i64::from(dim) - 1) as u32;
    for (tri_idx, tri) in projected.iter().enumerate() {
        let mut u = [f64::INFINITY, f64::NEG_INFINITY];
        let mut v = [f64::INFINITY, f64::NEG_INFINITY];
        let mut upv = [f64::INFINITY, f64::NEG_INFINITY];
        let mut vmu = [f64::INFINITY, f64::NEG_INFINITY];
        for vertex in tri {
            u = [u[0].min(vertex[0]), u[1].max(vertex[0])];
            v = [v[0].min(vertex[1]), v[1].max(vertex[1])];
            let s = vertex[0] + vertex[1];
            upv = [upv[0].min(s), upv[1].max(s)];
            let d = vertex[1] - vertex[0];
            vmu = [vmu[0].min(d), vmu[1].max(d)];
        }
        // Negated-OR form, not the De Morgan'd conjunction: with NaN
        // bounds (a malformed scene) every comparison is false, so the
        // triangle is NOT skipped and buckets at cell (0,0) through the
        // saturating casts in `clamp` — the pre-optimization behavior.
        let skip =
            u[1] < 0.0 || v[1] < 0.0 || u[0] >= f64::from(width) || v[0] >= f64::from(height);
        if !skip {
            let (x0, x1) = (clamp(u[0], width), clamp(u[1], width));
            let (y0, y1) = (clamp(v[0], height), clamp(v[1], height));
            for y in y0..=y1 {
                for x in x0..=x1 {
                    cells[(y * width + x) as usize].push(tri_idx as u32);
                }
            }
        }
        bounds.u.push(u);
        bounds.v.push(v);
        bounds.upv.push(upv);
        bounds.vmu.push(vmu);
    }
    (cells, bounds)
}

/// Lazily-built plane-filtered bucket lists for the cells of one slicing
/// line. Every pair of one line slices with the same plane, so the plane
/// part of the per-triangle skip test is evaluated once per cell per line
/// instead of once per cell per pair; cells no pair on the line needs
/// (uncovered regions) are never filtered at all.
struct LineFilter {
    data: Vec<u32>,
    /// Per line position: filtered range into `data`, or `UNBUILT`.
    ranges: Vec<(u32, u32)>,
}

const UNBUILT: (u32, u32) = (u32::MAX, u32::MAX);

impl LineFilter {
    fn new() -> Self {
        Self {
            data: Vec::new(),
            ranges: Vec::new(),
        }
    }

    fn reset(&mut self, line_len: usize) {
        self.data.clear();
        self.ranges.clear();
        self.ranges.resize(line_len, UNBUILT);
    }

    fn get(&mut self, pos: usize, cell: &[u32], keep: impl Fn(u32) -> bool) -> (u32, u32) {
        if self.ranges[pos] == UNBUILT {
            let start = self.data.len() as u32;
            self.data.extend(cell.iter().copied().filter(|&t| keep(t)));
            self.ranges[pos] = (start, self.data.len() as u32);
        }
        self.ranges[pos]
    }

    fn slice(&self, range: (u32, u32)) -> &[u32] {
        &self.data[range.0 as usize..range.1 as usize]
    }
}

/// Merges two sorted, duplicate-free bucket lists into `out` (sorted,
/// deduplicated) — same result as concatenate + sort + dedup without the
/// per-pair sort.
fn merge_buckets(a: &[u32], b: &[u32], out: &mut Vec<u32>) {
    out.clear();
    out.reserve(a.len() + b.len());
    let (mut i, mut j) = (0usize, 0usize);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => {
                out.push(a[i]);
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                out.push(b[j]);
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                out.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }
    out.extend_from_slice(&a[i..]);
    out.extend_from_slice(&b[j..]);
}

/// Reusable buffers for `pair_connected`, so the per-pair loop performs no
/// allocation once the vectors have grown to the working size.
#[derive(Default)]
struct PairScratch {
    /// Per-segment bounding interval [t_lo, t_hi, d_lo, d_hi].
    boxes: Vec<[f64; 4]>,
    reachable: Vec<bool>,
    queue: Vec<usize>,
}

/// Distance queries against `JOIN_GAP` are skipped when bounding intervals
/// are separated by more than this padded gap. Separation in a single
/// coordinate lower-bounds the true distance, and the computed distance
/// (a correctly-rounded chain over coordinates < 2^7 texels) sits within
/// 2^-45 texels of the true one, so a pad of 2^-20 texels — 2^25 times
/// that bound — guarantees every skipped query would have compared above
/// JOIN_GAP.
const GAP_PAD: f64 = JOIN_GAP + PRUNE_MARGIN;

/// Rounding slack for interval-based pruning, in texels: far above the
/// < 2^-45 texel error of any computed coordinate or distance in this
/// module (coordinates are < 2^7 texels in f64), far below any geometric
/// feature the verdicts depend on (JOIN_GAP = 2^-4 texels).
const PRUNE_MARGIN: f64 = 1.0 / (1 << 20) as f64;

/// The pair verdict on assembled cross-section segments: connected iff the
/// two samples lie on one component of the join graph (segments within
/// JOIN_GAP touch — shared mesh edges coincide exactly; sub-quantum cracks
/// close; proper crossings touch at distance zero).
///
/// Precondition: callers must already have handled the `win0 == win1` case
/// (same winning triangle implies connected without walking the join graph)
/// before gathering segments and calling this function. `t1` is the second
/// sample's strip coordinate (1.0 for an orthogonal pair, `SQRT_2` for a
/// diagonal pair); the first sample is always at `t = 0`.
fn pair_connected(
    segments: &[CrossSegment],
    d0: f64,
    win0: u32,
    t1: f64,
    d1: f64,
    win1: u32,
    scratch: &mut PairScratch,
) -> bool {
    let boxes = &mut scratch.boxes;
    boxes.clear();
    boxes.extend(segments.iter().map(|s| {
        [
            s.a[0].min(s.b[0]),
            s.a[0].max(s.b[0]),
            s.a[1].min(s.b[1]),
            s.a[1].max(s.b[1]),
        ]
    }));
    // A point (or box) farther than GAP_PAD from a segment's bounding
    // interval in either coordinate cannot pass the JOIN_GAP comparison;
    // skipping it leaves every `position` result unchanged.
    let point_near = |bx: &[f64; 4], t: f64, d: f64| {
        t >= bx[0] - GAP_PAD && t <= bx[1] + GAP_PAD && d >= bx[2] - GAP_PAD && d <= bx[3] + GAP_PAD
    };
    let find = |t: f64, d: f64, tri: u32| -> Option<usize> {
        segments
            .iter()
            .zip(boxes.iter())
            .position(|(s, bx)| {
                s.triangle == tri
                    && point_near(bx, t, d)
                    && point_segment_distance([t, d], s.a, s.b) <= JOIN_GAP
            })
            .or_else(|| {
                // Degenerate slices can drop a zero-length winning
                // cross-section; any segment through the sample serves.
                segments.iter().zip(boxes.iter()).position(|(s, bx)| {
                    point_near(bx, t, d) && point_segment_distance([t, d], s.a, s.b) <= JOIN_GAP
                })
            })
    };
    let (Some(start), Some(end)) = (find(0.0, d0, win0), find(t1, d1, win1)) else {
        debug_assert!(false, "sample point lies on no cross-section segment");
        return false;
    };
    if start == end {
        return true;
    }
    let reachable = &mut scratch.reachable;
    reachable.clear();
    reachable.resize(segments.len(), false);
    let queue = &mut scratch.queue;
    queue.clear();
    queue.push(start);
    reachable[start] = true;
    while let Some(i) = queue.pop() {
        let bi = boxes[i];
        for j in 0..segments.len() {
            if reachable[j] {
                continue;
            }
            let bj = &boxes[j];
            // Interval separation beyond GAP_PAD in either coordinate
            // guarantees the distance comparison below would fail.
            if bi[0] > bj[1] + GAP_PAD
                || bj[0] > bi[1] + GAP_PAD
                || bi[2] > bj[3] + GAP_PAD
                || bj[2] > bi[3] + GAP_PAD
            {
                continue;
            }
            if segment_segment_distance(&segments[i], &segments[j]) <= JOIN_GAP {
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
    let (cells, tri_bounds) = buckets(&projected, width, height);
    let reach = (side.h_max as f64 + 0.5) / RELIEF_UNITS_PER_PIXEL as f64;
    let at = |x: u32, y: u32| (y * width + x) as usize;

    let mut horizontal = vec![false; ((width.saturating_sub(1)) * height) as usize];
    let mut vertical = vec![false; (width * height.saturating_sub(1)) as usize];
    let mut diag_down_right =
        vec![false; ((width.saturating_sub(1)) * height.saturating_sub(1)) as usize];
    let mut diag_down_left =
        vec![false; ((width.saturating_sub(1)) * height.saturating_sub(1)) as usize];
    let mut segments: Vec<CrossSegment> = Vec::new();
    let mut gathered: Vec<u32> = Vec::new();
    let mut scratch = PairScratch::default();

    let mut line_filter = LineFilter::new();

    // Labels one pair (x0,y0)-(x1,y1) whose center-to-center direction is
    // (dxf, dyf) with length `t_max`: (1,0)/1.0 and (0,1)/1.0 for the
    // orthogonal cases, (1,1)/SQRT_2 and (-1,1)/SQRT_2 for the diagonal
    // ones. `t` runs from 0 at the first center to `t_max` at the second;
    // `w` is the perpendicular offset, sliced at w = 0. Negating `w` (as
    // happens for the vertical case below relative to a literal
    // application of the rotation formula) does not change the verdict:
    // `slice_triangle`'s straddle test and interpolation factor are both
    // invariant under a global sign flip of the plane coordinate.
    //
    // Callers have already applied the coverage and win0 == win1 early
    // outs and the per-line PLANE half of the triangle skip (see the
    // direction passes below); `fa`/`fb` are the two texels' plane-
    // filtered bucket lists. The strip-window half of the skip stays
    // here because the window moves with the pair along the line.
    let mut label =
        |x0: u32, y0: u32, x1: u32, y1: u32, t_max: f64, fa: &[u32], fb: &[u32]| -> bool {
            let (i, j) = (at(x0, y0), at(x1, y1));
            let (win0, win1) = (side.winning[i], side.winning[j]);
            merge_buckets(fa, fb, &mut gathered);
            segments.clear();
            let c0u = f64::from(x0) + 0.5;
            let c0v = f64::from(y0) + 0.5;
            let dxf = (f64::from(x1) - f64::from(x0)) / t_max;
            let dyf = (f64::from(y1) - f64::from(y0)) / t_max;
            // Strip-window skip: `clip_segment` returns None when both
            // endpoint `t` values sit strictly outside [0, t_max] on the same
            // side by more than rounding; endpoint `t` values are convex
            // interpolations of vertex `t` values up to < 2^-45 texel rounding
            // (coordinates are < 2^7 texels; the clip solve's divide-and-
            // remultiply stays within the same ulp scale), so extrema outside
            // the window by PRUNE_MARGIN guarantee None. Diagonal windows are
            // tested in the unscaled u+v / v-u coordinates, where the window
            // [0, t_max] maps to width 2 exactly and PRUNE_MARGIN still
            // dwarfs the bound.
            for &tri_idx in &gathered {
                let skip = if y0 == y1 {
                    // Horizontal: strip t = u - c0u.
                    let b = tri_bounds.u[tri_idx as usize];
                    b[1] < c0u - PRUNE_MARGIN || b[0] > c0u + 1.0 + PRUNE_MARGIN
                } else if x0 == x1 {
                    // Vertical: strip t = v - c0v.
                    let b = tri_bounds.v[tri_idx as usize];
                    b[1] < c0v - PRUNE_MARGIN || b[0] > c0v + 1.0 + PRUNE_MARGIN
                } else if x1 > x0 {
                    // Down-right diagonal: strip t = ((u + v) - (c0u + c0v))
                    // / sqrt(2), in-window u + v range [c0u + c0v, c0u + c0v
                    // + 2].
                    let sum = c0u + c0v;
                    let b = tri_bounds.upv[tri_idx as usize];
                    b[1] < sum - PRUNE_MARGIN || b[0] > sum + 2.0 + PRUNE_MARGIN
                } else {
                    // Down-left diagonal: strip t = ((v - u) - (c0v - c0u))
                    // / sqrt(2), in-window v - u range [c0v - c0u, c0v - c0u
                    // + 2].
                    let diff = c0v - c0u;
                    let b = tri_bounds.vmu[tri_idx as usize];
                    b[1] < diff - PRUNE_MARGIN || b[0] > diff + 2.0 + PRUNE_MARGIN
                };
                if skip {
                    continue;
                }
                // Orthogonal pairs specialize the generic rotation
                // `t = du * dxf + dv * dyf`, `w = du * -dyf + dv * dxf`:
                // with (dxf, dyf) exactly (1.0, 0.0) or (0.0, 1.0), the
                // dropped `±0.0` product terms and the exact negation
                // (`x * -1.0 == -x` in IEEE 754) can only change the sign
                // of a zero, which no downstream test observes — slicing
                // compares `w` against 0.0 sign-insensitively, and `t`
                // only ever feeds arithmetic and magnitude comparisons.
                let v = if y0 == y1 {
                    projected[tri_idx as usize].map(|p| [p[1] - c0v, p[0] - c0u, p[2]])
                } else if x0 == x1 {
                    projected[tri_idx as usize].map(|p| [-(p[0] - c0u), p[1] - c0v, p[2]])
                } else {
                    projected[tri_idx as usize].map(|p| {
                        let du = p[0] - c0u;
                        let dv = p[1] - c0v;
                        let t = du * dxf + dv * dyf;
                        let w = du * -dyf + dv * dxf;
                        [w, t, p[2]]
                    })
                };
                let before = segments.len();
                slice_triangle(v, tri_idx, &mut segments);
                // Clip in place; drop what the strip/reach excludes.
                let mut kept = before;
                for s in before..segments.len() {
                    if let Some(clipped) = clip_segment(segments[s], t_max, reach) {
                        segments[kept] = clipped;
                        kept += 1;
                    }
                }
                segments.truncate(kept);
            }
            pair_connected(
                &segments,
                side.depth[i],
                win0,
                t_max,
                side.depth[j],
                win1,
                &mut scratch,
            )
        };

    // Coverage early-out shared by all four passes: pairs touching an
    // uncovered texel keep their `false` label without any work.
    let covered = |i: usize, j: usize| side.depth[i].is_finite() && side.depth[j].is_finite();

    // Four direction passes, one slicing line at a time. Every pair on a
    // line slices triangles with that line's plane, so the PLANE half of
    // the per-triangle skip — evaluated per cell per pair in a fused
    // sweep — is hoisted into a lazily-built per-line filtered bucket
    // list per cell. The plane tests are the exact/margined conditions
    // documented at `buckets`' call site: exact extrema-vs-center
    // comparisons for orthogonal lines (correctly rounded subtraction
    // preserves order and maps only equal operands to zero, and the
    // orthogonal `w` equals the plain coordinate difference up to the
    // sign of a zero), PRUNE_MARGIN-padded comparisons for diagonal
    // lines whose `w` carries < 2^-45 texels of product rounding.
    // Filtering commutes with the sorted merge (one predicate per line,
    // applied per element), so `gathered` sees the same triangles in the
    // same order as a per-pair filter would produce. Pass order does not
    // matter: labels are written to four disjoint arrays and no verdict
    // reads another pair's label.

    // Horizontal pass: row y slices with plane v = y + 0.5.
    for y in 0..height {
        if width < 2 {
            break;
        }
        line_filter.reset(width as usize);
        let c0v = f64::from(y) + 0.5;
        let keep = |t: u32| {
            let b = tri_bounds.v[t as usize];
            !(b[0] > c0v || b[1] < c0v)
        };
        for x in 0..width - 1 {
            let idx = (y * (width - 1) + x) as usize;
            let (i, j) = (at(x, y), at(x + 1, y));
            if !covered(i, j) {
                continue;
            }
            if side.winning[i] == side.winning[j] {
                horizontal[idx] = true;
                continue;
            }
            let ra = line_filter.get(x as usize, &cells[i], keep);
            let rb = line_filter.get(x as usize + 1, &cells[j], keep);
            horizontal[idx] = label(
                x,
                y,
                x + 1,
                y,
                1.0,
                line_filter.slice(ra),
                line_filter.slice(rb),
            );
        }
    }

    // Vertical pass: column x slices with plane u = x + 0.5.
    for x in 0..width {
        if height < 2 {
            break;
        }
        line_filter.reset(height as usize);
        let c0u = f64::from(x) + 0.5;
        let keep = |t: u32| {
            let b = tri_bounds.u[t as usize];
            !(b[0] > c0u || b[1] < c0u)
        };
        for y in 0..height - 1 {
            let idx = (y * width + x) as usize;
            let (i, j) = (at(x, y), at(x, y + 1));
            if !covered(i, j) {
                continue;
            }
            if side.winning[i] == side.winning[j] {
                vertical[idx] = true;
                continue;
            }
            let ra = line_filter.get(y as usize, &cells[i], keep);
            let rb = line_filter.get(y as usize + 1, &cells[j], keep);
            vertical[idx] = label(
                x,
                y,
                x,
                y + 1,
                1.0,
                line_filter.slice(ra),
                line_filter.slice(rb),
            );
        }
    }

    // Down-right diagonal pass: the line through cells (sx + k, sy + k)
    // slices with plane v - u = sy - sx (exact: both centers carry the
    // same +0.5, which cancels exactly in f64).
    // Down-left diagonal pass: the line through cells (cx, s - cx)
    // slices with plane u + v = s + 1 (exact: the two +0.5 halves sum to
    // an integer-plus-one exactly).
    if width >= 2 && height >= 2 {
        let starts = (0..height)
            .map(|sy| (0u32, sy))
            .chain((1..width).map(|sx| (sx, 0u32)));
        for (sx, sy) in starts {
            let len = (width - sx).min(height - sy);
            if len < 2 {
                continue;
            }
            line_filter.reset(len as usize);
            let diff = f64::from(sy) - f64::from(sx);
            let keep = |t: u32| {
                let b = tri_bounds.vmu[t as usize];
                !(b[0] > diff + PRUNE_MARGIN || b[1] < diff - PRUNE_MARGIN)
            };
            for k in 0..len - 1 {
                let (x, y) = (sx + k, sy + k);
                let idx = (y * (width - 1) + x) as usize;
                let (i, j) = (at(x, y), at(x + 1, y + 1));
                if !covered(i, j) {
                    continue;
                }
                if side.winning[i] == side.winning[j] {
                    diag_down_right[idx] = true;
                    continue;
                }
                let ra = line_filter.get(k as usize, &cells[i], keep);
                let rb = line_filter.get(k as usize + 1, &cells[j], keep);
                diag_down_right[idx] = label(
                    x,
                    y,
                    x + 1,
                    y + 1,
                    std::f64::consts::SQRT_2,
                    line_filter.slice(ra),
                    line_filter.slice(rb),
                );
            }
        }
        // Anti-diagonal lines indexed by the cell-coordinate sum s of the
        // pair's two endpoint cells; anchors (x, s - 1 - x).
        for s in 1..width + height - 2 {
            let cx_min = s.saturating_sub(height - 1);
            let cx_max = (width - 1).min(s);
            if cx_min >= cx_max {
                continue;
            }
            line_filter.reset((cx_max - cx_min + 1) as usize);
            let sum = f64::from(s) + 1.0;
            let keep = |t: u32| {
                let b = tri_bounds.upv[t as usize];
                !(b[0] > sum + PRUNE_MARGIN || b[1] < sum - PRUNE_MARGIN)
            };
            for x in cx_min..cx_max {
                // Anchor (x, y) with y = s - 1 - x; endpoints (x + 1, y)
                // and (x, y + 1), the line's cells at cx = x + 1 and x —
                // both in bounds for every x in [cx_min, cx_max).
                let y = s - 1 - x;
                let idx = (y * (width - 1) + x) as usize;
                let (i, j) = (at(x + 1, y), at(x, y + 1));
                if !covered(i, j) {
                    continue;
                }
                if side.winning[i] == side.winning[j] {
                    diag_down_left[idx] = true;
                    continue;
                }
                let ra = line_filter.get((x + 1 - cx_min) as usize, &cells[i], keep);
                let rb = line_filter.get((x - cx_min) as usize, &cells[j], keep);
                diag_down_left[idx] = label(
                    x + 1,
                    y,
                    x,
                    y + 1,
                    std::f64::consts::SQRT_2,
                    line_filter.slice(ra),
                    line_filter.slice(rb),
                );
            }
        }
    }
    SideContinuity {
        width,
        height,
        horizontal,
        vertical,
        diag_down_right,
        diag_down_left,
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
        let clipped = clip_segment(seg([-1.0, 1.0], [2.0, 1.0]), 1.0, 5.0).expect("survives");
        let mut endpoints = [clipped.a, clipped.b];
        endpoints.sort_by(|p, q| p[0].total_cmp(&q[0]));
        assert_eq!(endpoints, [[0.0, 1.0], [1.0, 1.0]]);
    }

    /// Clip to a diagonal strip: `t_max = SQRT_2` keeps the segment's
    /// interior instead of truncating it to [0, 1]. The Liang-Barsky
    /// solve divides then re-multiplies by `dt`, so the recovered
    /// endpoint at an irrational `t_max` carries a last-bit rounding
    /// error; compare with a tolerance far below `JOIN_GAP` rather than
    /// bit-exact equality.
    #[test]
    fn clip_trims_to_diagonal_strip() {
        let t_max = std::f64::consts::SQRT_2;
        let clipped = clip_segment(seg([-1.0, 1.0], [2.0, 1.0]), t_max, 5.0).expect("survives");
        let mut endpoints = [clipped.a, clipped.b];
        endpoints.sort_by(|p, q| p[0].total_cmp(&q[0]));
        let expected = [[0.0, 1.0], [t_max, 1.0]];
        for (got, want) in endpoints.iter().zip(expected.iter()) {
            assert!(
                (got[0] - want[0]).abs() < 1e-12 && (got[1] - want[1]).abs() < 1e-12,
                "endpoint {got:?} not within tolerance of {want:?}"
            );
        }
    }

    /// Clip against reach: a segment whose in-strip part lies wholly past
    /// reach is removed entirely.
    #[test]
    fn clip_removes_unreachable_remainder() {
        // t: -0.5 -> 0.5, d: 0 -> 10; in-strip requires s >= 0.5 where
        // d = 5..10, but reach 4 requires s <= 0.4.
        assert_eq!(clip_segment(seg([-0.5, 0.0], [0.5, 10.0]), 1.0, 4.0), None);
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
    /// across the crease, even at 45 degrees (8 relief units per texel).
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

    /// A diagonal near/far pair: on a 2x2 grid, a near quad (depth 1)
    /// covers only texel (0,0)'s center — it spans x,y in [-1, 1.3], so
    /// its far edge stops short of (1,0)'s and (0,1)'s centers at x = 1.5
    /// and y = 1.5 respectively, let alone (1,1)'s at (1.5, 1.5). A far
    /// quad (depth 5) spans the whole grid, so (1,0), (0,1) and (1,1) are
    /// far-only while (0,0) is near (nearer wins). The two quads are flat,
    /// parallel, and share no edge, so their cross-sections never touch:
    /// the diagonal (0,0)-(1,1) pair — near sample at t=0, d=1; far sample
    /// at t=SQRT_2, d=5 — must be cut, exactly like the orthogonal case in
    /// `silhouette_pair_is_cut`.
    #[test]
    fn diagonal_silhouette_pair_is_cut() {
        let mut triangles = Vec::new();
        triangles.extend(rect(-1.0, 1.3, -1.0, 1.3, 1.0)); // near, covers only (0,0)
        triangles.extend(rect(-1.0, 5.0, -1.0, 5.0, 5.0)); // far, covers the whole grid
        let labels = side_labels(triangles, 2, 2, 400);
        assert!(
            !labels.connected(0, 0, 1, 1),
            "diagonal near/far pair must cut: the near quad terminates \
             mid-strip with no wall, and the far plane never meets it"
        );
    }

    /// A plane tilted 45 degrees along the diagonal direction, z =
    /// 0.5*(x+y), split into two triangles along the anti-diagonal x+y =
    /// 2.1 (not through any texel center) so the (0,0) and (1,1) samples
    /// provably land on different triangles — the win0 == win1 fast path
    /// cannot fire, forcing the verdict to walk the join graph across the
    /// shared triangle edge. The plane is continuous, so the diagonal pair
    /// must connect.
    ///
    /// Corners: P0=(-1,-1), P1=(3,-0.9), P2=(3,3), P3=(-0.9,3), each with
    /// z = 0.5*(x+y) (P0: -1, P1: 1.05, P2: 3, P3: 1.05). Triangle A =
    /// (P0,P1,P3) covers x+y <= 2.1 within the outer square — (0,0)'s
    /// center (0.5,0.5) (sum 1.0) as well as (1,0)'s (1.5,0.5) and (0,1)'s
    /// (0.5,1.5) (sum 2.0 each). Triangle B = (P1,P2,P3) covers the rest —
    /// (1,1)'s center (1.5,1.5) (sum 3.0).
    #[test]
    fn diagonal_tilted_plane_is_connected() {
        let z = |x: f32, y: f32| 0.5 * (x + y);
        let p0 = [-1.0, -1.0, z(-1.0, -1.0)];
        let p1 = [3.0, -0.9, z(3.0, -0.9)];
        let p2 = [3.0, 3.0, z(3.0, 3.0)];
        let p3 = [-0.9, 3.0, z(-0.9, 3.0)];
        let triangles = vec![tri3(p0, p1, p3), tri3(p1, p2, p3)];
        let labels = side_labels(triangles, 2, 2, 400);
        assert!(
            labels.connected(0, 0, 1, 1),
            "tilted diagonal plane must connect across the shared triangle edge"
        );
    }
}
