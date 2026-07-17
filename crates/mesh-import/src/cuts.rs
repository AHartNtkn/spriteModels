//! Point-to-mesh distance acceleration for the fabricated-wall cut pass
//! (`capture.rs`): after ownership and closure, a side's kept mask can join
//! two surface sheets that are 4-adjacent in its projection but disconnected
//! in 3D (a near sheet silhouetted over a far one). The cut pass tests each
//! such candidate pair against the mesh itself; this module is the
//! uniform-grid distance query that test uses (spec: "Fabricated-wall
//! cuts").

use relief_core::Bounds;

use crate::Triangle;
use crate::capture::{add3, dot3, scale3, sub3};

type Point = [f64; 3];

/// Closest point on triangle `(a, b, c)` to `p`, via the standard
/// region-partition method (Ericson, *Real-Time Collision Detection*,
/// section 5.1.5): `p` is classified against each of the triangle's three
/// vertex and three edge Voronoi regions using signed dot-product tests,
/// falling through to the face interior (barycentric projection) only once
/// every vertex/edge region has been ruled out.
fn closest_point_on_triangle(p: Point, a: Point, b: Point, c: Point) -> Point {
    let ab = sub3(b, a);
    let ac = sub3(c, a);
    let ap = sub3(p, a);
    let d1 = dot3(ab, ap);
    let d2 = dot3(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a; // vertex region a
    }

    let bp = sub3(p, b);
    let d3 = dot3(ab, bp);
    let d4 = dot3(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b; // vertex region b
    }

    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return add3(a, scale3(ab, v)); // edge region ab
    }

    let cp = sub3(p, c);
    let d5 = dot3(ab, cp);
    let d6 = dot3(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c; // vertex region c
    }

    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return add3(a, scale3(ac, w)); // edge region ac
    }

    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return add3(b, scale3(sub3(c, b), w)); // edge region bc
    }

    // Face interior: barycentric projection.
    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    add3(a, add3(scale3(ab, v), scale3(ac, w)))
}

fn distance_to_triangle(p: Point, tri: [Point; 3]) -> f64 {
    let closest = closest_point_on_triangle(p, tri[0], tri[1], tri[2]);
    let d = sub3(p, closest);
    dot3(d, d).sqrt()
}

/// Uniform grid over the model box, cell size exactly one texel (so cell
/// count is bounded by the model box itself: `width * height * depth <=
/// 63^3`), holding each cell's overlapping triangle indices. Built once per
/// `convert_box_space` call from its box-space triangles and shared by
/// every side's cut pass.
pub(crate) struct TriangleGrid {
    width: u32,
    height: u32,
    depth: u32,
    cells: Vec<Vec<u32>>,
    triangles: Vec<[Point; 3]>,
}

/// Clamp a box-space coordinate to a grid cell index along one axis.
/// Out-of-box query points (and the rare vertex that overshoots its bound
/// by float error from `fit`'s ceil-rounded dims) clamp into the nearest
/// edge cell rather than being rejected: the grid only ever needs to answer
/// "is there mesh near here", and clamping keeps that answer well-defined
/// everywhere instead of failing at the box boundary.
fn cell_index(value: f64, dim: u32) -> u32 {
    (value.floor() as i64).clamp(0, i64::from(dim) - 1) as u32
}

impl TriangleGrid {
    pub(crate) fn build(triangles: &[Triangle], bounds: Bounds) -> Self {
        let (width, height, depth) = (bounds.width(), bounds.height(), bounds.depth());
        let cell_count = width as usize * height as usize * depth as usize;
        let mut cells = vec![Vec::new(); cell_count];
        let index = |x: u32, y: u32, z: u32| -> usize {
            x as usize + width as usize * (y as usize + height as usize * z as usize)
        };

        let mut stored = Vec::with_capacity(triangles.len());
        for (tri_idx, triangle) in triangles.iter().enumerate() {
            let as_f64 = |p: [f32; 3]| [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])];
            let positions = [
                as_f64(triangle.positions[0]),
                as_f64(triangle.positions[1]),
                as_f64(triangle.positions[2]),
            ];
            stored.push(positions);

            let mut min = positions[0];
            let mut max = positions[0];
            for &p in &positions[1..] {
                for axis in 0..3 {
                    min[axis] = min[axis].min(p[axis]);
                    max[axis] = max[axis].max(p[axis]);
                }
            }
            let (min_x, min_y, min_z) = (
                cell_index(min[0], width),
                cell_index(min[1], height),
                cell_index(min[2], depth),
            );
            let (max_x, max_y, max_z) = (
                cell_index(max[0], width),
                cell_index(max[1], height),
                cell_index(max[2], depth),
            );
            for z in min_z..=max_z {
                for y in min_y..=max_y {
                    for x in min_x..=max_x {
                        cells[index(x, y, z)].push(tri_idx as u32);
                    }
                }
            }
        }

        Self {
            width,
            height,
            depth,
            cells,
            triangles: stored,
        }
    }

    /// True iff some triangle lies within one texel of `q` — one texel
    /// being the discretization's own resolution, the tolerance the
    /// wall-reality test judges "on the mesh" against.
    ///
    /// Containment argument for scanning only the 3x3x3 cell block around
    /// `q`'s own cell: cells are exactly one texel wide, so `q`'s cell
    /// index along any axis is `cx = floor(q)`, putting `q` itself in
    /// `[cx, cx+1)`. A triangle whose cell index along that axis is `<=
    /// cx-2` has every point with that coordinate `< cx-1`, so its gap to
    /// `q` on that axis alone already exceeds `cx - (cx-1) = 1`; symmetric
    /// reasoning covers `>= cx+2`. So no triangle outside the 3x3x3 block
    /// can be within distance 1.0 of `q`, and the block is exact
    /// containment, not an approximation.
    pub(crate) fn within_one_texel(&self, q: Point) -> bool {
        let (cx, cy, cz) = (
            i64::from(cell_index(q[0], self.width)),
            i64::from(cell_index(q[1], self.height)),
            i64::from(cell_index(q[2], self.depth)),
        );
        for dz in -1..=1i64 {
            let z = cz + dz;
            if z < 0 || z >= i64::from(self.depth) {
                continue;
            }
            for dy in -1..=1i64 {
                let y = cy + dy;
                if y < 0 || y >= i64::from(self.height) {
                    continue;
                }
                for dx in -1..=1i64 {
                    let x = cx + dx;
                    if x < 0 || x >= i64::from(self.width) {
                        continue;
                    }
                    let idx = x as usize
                        + self.width as usize * (y as usize + self.height as usize * z as usize);
                    for &tri_idx in &self.cells[idx] {
                        if distance_to_triangle(q, self.triangles[tri_idx as usize]) <= 1.0 {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::TriangleGrid;
    use crate::Triangle;
    use relief_core::Bounds;

    fn triangle(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> Triangle {
        Triangle {
            positions: [a, b, c],
            normals: [[0.0, 0.0, 1.0]; 3],
            uvs: [[0.0, 0.0]; 3],
            colors: [[1.0, 1.0, 1.0, 1.0]; 3],
            material: 0,
        }
    }

    /// A single right triangle in the z = 0 plane with legs 10 texels
    /// long, inside a 12x12x12 grid (comfortably larger than the
    /// triangle's own extent, exercising clamping at the query points
    /// below without the triangle itself needing to touch the boundary).
    #[test]
    fn triangle_grid_distance_query() {
        let tri = triangle([0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]);
        let bounds = Bounds::new(12, 12, 12).expect("bounds");
        let grid = TriangleGrid::build(&[tri], bounds);

        // (2,2) is inside the triangle's footprint (2 + 2 = 4 < 10), so a
        // point 0.5 texels above the face is a pure face-region query.
        assert!(
            grid.within_one_texel([2.0, 2.0, 0.5]),
            "a point 0.5 texels off the face must be within the one-texel tolerance"
        );

        // Same footprint column, 2.0 texels off: exceeds tolerance.
        assert!(
            !grid.within_one_texel([2.0, 2.0, 2.0]),
            "a point 2.0 texels off the face must exceed the one-texel tolerance"
        );

        // Diagonally outside vertex a = (0,0,0): the closest point on the
        // triangle is the vertex itself (Ericson vertex region, the d1<=0
        // && d2<=0 branch), at distance sqrt(0.3^2 + 0.3^2) ~= 0.424.
        assert!(
            grid.within_one_texel([-0.3, -0.3, 0.0]),
            "a point near a vertex must resolve via the vertex region and stay within tolerance"
        );
    }
}
