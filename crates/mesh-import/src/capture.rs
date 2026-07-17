use relief_core::{AuthoredModel, Bounds, CanonicalView, Chart, RELIEF_UNITS_PER_PIXEL};

use crate::cuts::TriangleGrid;
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
        Self {
            modes: [SideMode::Capture; 6],
        }
    }
}

impl SideModes {
    pub fn get(&self, view: CanonicalView) -> SideMode {
        self.modes[view.rank() as usize]
    }

    /// Whether `view` may be set to `FromOpposite`/`FromOppositeMirrored`
    /// in the current state. The single predicate `set` enforces and
    /// `legal_modes` queries, so the two can never drift apart.
    fn allows_from_opposite(&self, view: CanonicalView) -> bool {
        self.get(view.opposite()) == SideMode::Capture
    }

    /// Sets one side's mode. `FromOpposite*` requires the opposite side to
    /// be `Capture`. Moving a side out of `Capture` resets an opposite that
    /// depended on it to `Off`.
    pub fn set(&mut self, view: CanonicalView, mode: SideMode) -> Result<(), ImportError> {
        if matches!(
            mode,
            SideMode::FromOpposite | SideMode::FromOppositeMirrored
        ) && !self.allows_from_opposite(view)
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

    /// The modes `set` would accept for this side in the current state.
    /// This is the single source of truth the UI queries; `set` remains the
    /// enforcing mutation.
    pub fn legal_modes(&self, view: CanonicalView) -> impl Iterator<Item = SideMode> + '_ {
        [
            SideMode::Capture,
            SideMode::FromOpposite,
            SideMode::FromOppositeMirrored,
            SideMode::Off,
        ]
        .into_iter()
        .filter(move |mode| {
            !matches!(
                mode,
                SideMode::FromOpposite | SideMode::FromOppositeMirrored
            ) || self.allows_from_opposite(view)
        })
    }

    pub fn validate(&self) -> Result<(), ImportError> {
        for view in ALL_VIEWS {
            if matches!(
                self.get(view),
                SideMode::FromOpposite | SideMode::FromOppositeMirrored
            ) && !self.allows_from_opposite(view)
            {
                return Err(ImportError::UnsatisfiedOpposite {
                    side: view,
                    opposite: view.opposite(),
                });
            }
        }
        if ALL_VIEWS
            .iter()
            .all(|&view| self.get(view) != SideMode::Capture)
        {
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
            // glTF is +Y-up, +Z-toward-viewer; the box frame's y points down
            // and its front face looks along +z. Identity would import
            // models upside down and back-to-front, so the default is the
            // half-turn about X that maps the two conventions onto each
            // other (spec: "Fitting").
            rotation: [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]],
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
    if scene.triangles.is_empty() {
        return Err(ImportError::NoTriangles);
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
    let dims = [
        bounds.width() as f32,
        bounds.height() as f32,
        bounds.depth() as f32,
    ];
    // Center the mesh inside the box on each axis.
    let offset = [
        (dims[0] - extents[0] * scale) / 2.0,
        (dims[1] - extents[1] * scale) / 2.0,
        (dims[2] - extents[2] * scale) / 2.0,
    ];
    Ok(Fit {
        bounds,
        scale,
        rotated_min: min,
        offset,
    })
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
    let (box_scene, bounds) =
        box_space_scene(scene, settings.rotation, settings.longest_axis_pixels)?;
    convert_box_space(&box_scene, bounds, settings)
}

/// The capture half of `convert`, taking an already-fitted box-space scene
/// and its bounds directly. Shared with the import dialog, which computes
/// `box_space_scene` once for its mesh preview and feeds the same result
/// here instead of paying for the mesh -> box-space transform twice per
/// settings change.
pub(crate) fn add3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(crate) fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(crate) fn scale3(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

pub(crate) fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Euclidean distance between two box-space points, in texels (box-space
/// units are texels by construction: `fit`'s `dim()` maps the scaled mesh
/// extent directly onto the bounds' integer texel counts).
pub(crate) fn distance3(a: [f64; 3], b: [f64; 3]) -> f64 {
    dot3(sub3(a, b), sub3(a, b)).sqrt()
}

/// One enabled Capture side's rasterization after the reachability filter
/// (Task 12's midplane drop) but before ownership: everything ownership
/// needs to query this side as either the observing side or a candidate
/// owner for another side's hit.
struct CaptureSide {
    view: CanonicalView,
    origin: [f64; 3],
    right: [f64; 3],
    down: [f64; 3],
    forward: [f64; 3],
    width: u32,
    height: u32,
    h_max: i64,
    /// Reachability-filtered depth in model pixels, exact (not quantized):
    /// `f64::INFINITY` marks a texel that is either uncovered or whose hit
    /// quantized past `h_max` and was dropped.
    depth: Vec<f64>,
    /// Geometric face normal of the winning triangle at each texel;
    /// meaningless (never read) where `depth` is not finite.
    face_normal: Vec<[f32; 3]>,
    /// Fully encoded RGBA (relief-quantized alpha, dropped texels already
    /// `[0,0,0,0]`) before ownership decides which texels this side keeps.
    rgba: Vec<[u8; 4]>,
}

impl CaptureSide {
    fn index(&self, x: u32, y: u32) -> usize {
        (y * self.width + x) as usize
    }

    /// The 3D point a texel's reconstructed depth corresponds to, per the
    /// registration identity `p = origin + (x+0.5)*right + (y+0.5)*down +
    /// depth*forward` (Task 11 pins this reconstruction/projection
    /// round-trip as exact).
    fn point_at(&self, x: u32, y: u32, depth: f64) -> [f64; 3] {
        add3(
            add3(self.origin, scale3(self.right, f64::from(x) + 0.5)),
            add3(
                scale3(self.down, f64::from(y) + 0.5),
                scale3(self.forward, depth),
            ),
        )
    }
}

fn capture_side(
    box_scene: &TriangleScene,
    view: CanonicalView,
    bounds: Bounds,
    lighting: &Lighting,
) -> CaptureSide {
    let frame = view.frame(bounds);
    let (width, height) = view.dimensions(bounds);
    let raster = rasterize(
        box_scene,
        &View {
            origin: frame.origin.map(|c| c as f32),
            right: frame.source_u.map(|c| c as f32),
            down: frame.source_v.map(|c| c as f32),
            forward: frame.inward.map(|c| c as f32),
            scale: 1.0,
            width,
            height,
        },
        lighting,
    );
    let h_max = i64::from(view.maximum_inward_depth(bounds));
    let count = (width * height) as usize;
    let mut depth = vec![f64::INFINITY; count];
    let mut face_normal = vec![[0.0f32; 3]; count];
    let mut rgba = vec![[0u8; 4]; count];
    for i in 0..count {
        let d = raster.depth[i];
        if d == f32::INFINITY {
            continue;
        }
        // depth is in model pixels from the face plane; float error can dip
        // epsilon-negative, the max(0) floor handles it.
        let relief = (f64::from(d) * RELIEF_UNITS_PER_PIXEL as f64).round() as i64;
        let relief = relief.max(0);
        // A post-quantization relief beyond h_max lies past the midplane,
        // which is exactly the region the opposing side reaches
        // (d_front > D/2 <=> d_back < D/2); range-checking after
        // quantization keeps the exact-midplane hit (relief == h_max) on
        // both sides, preserving the format's opposing-charts-meet-at-the-
        // midplane guarantee. Dropping instead of clamping avoids
        // fabricating geometry at a false depth. Applying this filter here,
        // before ownership, means every cross-side query in pass 2 sees
        // post-filter state, exactly like Task 12 intends.
        if relief > h_max {
            continue;
        }
        depth[i] = f64::from(d);
        face_normal[i] = raster.face_normal[i];
        let color = raster.color[i];
        rgba[i] = [color[0], color[1], color[2], (255 - relief) as u8];
    }
    let as_f64 = |v: [i64; 3]| [v[0] as f64, v[1] as f64, v[2] as f64];
    CaptureSide {
        view,
        origin: as_f64(frame.origin),
        right: as_f64(frame.source_u),
        down: as_f64(frame.source_v),
        forward: as_f64(frame.inward),
        width,
        height,
        h_max,
        depth,
        face_normal,
        rgba,
    }
}

/// `score(T) = sigma * (-dot(n, forward_T))` — how head-on side `T` would
/// observe a hit with face normal `n` and observation orientation `sigma`.
/// For `T == S` (the side that actually captured the hit) this reduces
/// algebraically to `|dot(n, forward_S)|` regardless of `sigma`'s sign
/// (whichever branch of `sigma` applies, the two negations cancel), which
/// is exactly the spec's "S's own score" rule. So `S` needs no special-case
/// formula, only the "always a candidate" exemption from the
/// score>0/reach/bounds/visible gate that every other side must clear.
fn observation_score(sigma: f64, normal: [f64; 3], forward_t: [f64; 3]) -> f64 {
    sigma * -dot3(normal, forward_t)
}

/// Max absolute one-sided finite difference between `z` (the filtered depth
/// at `(x, y)`) and its up-to-4 in-bounds finite 4-neighbors; `0.0` if none
/// are finite. Bounds the depth buffer's local slope for the visibility
/// tolerance in `owning_mask` below.
fn local_gradient(depth: &[f64], width: u32, height: u32, x: u32, y: u32, z: f64) -> f64 {
    let mut grad = 0.0f64;
    let offsets = [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)];
    for (dx, dy) in offsets {
        let nx = x as i64 + dx;
        let ny = y as i64 + dy;
        if nx < 0 || ny < 0 || nx >= i64::from(width) || ny >= i64::from(height) {
            continue;
        }
        let neighbor = depth[(ny as u32 * width + nx as u32) as usize];
        if neighbor.is_finite() {
            grad = grad.max((z - neighbor).abs());
        }
    }
    grad
}

/// Ownership pass 2 for one side `S` (`sides[s_idx]`): for each of `S`'s
/// covered texels, decides whether `S` is the best-facing candidate among
/// the enabled Capture sides that observe the same oriented face, can
/// reach the reconstructed point, and see it (are not occluded there).
/// Returns a per-texel keep mask the same shape as `S`'s buffers.
fn owning_mask(sides: &[CaptureSide], s_idx: usize) -> Vec<bool> {
    let s = &sides[s_idx];
    let count = (s.width * s.height) as usize;
    let mut kept = vec![false; count];
    for y in 0..s.height {
        for x in 0..s.width {
            let idx = s.index(x, y);
            let depth = s.depth[idx];
            if !depth.is_finite() {
                continue;
            }
            let p = s.point_at(x, y, depth);
            let normal = [
                f64::from(s.face_normal[idx][0]),
                f64::from(s.face_normal[idx][1]),
                f64::from(s.face_normal[idx][2]),
            ];
            // sigma = +1 when S observes the front face (-n.forward_S >=
            // 0); exact 0 (grazing incidence) is conventionally front.
            let sigma = if dot3(normal, s.forward) <= 0.0 {
                1.0
            } else {
                -1.0
            };

            // S is always a candidate for its own hit; no seen-and-
            // reachable point is orphaned when its ideal side is occluded
            // or disabled.
            let mut owner = s.view;
            let mut best_score = observation_score(sigma, normal, s.forward);
            let mut best_rank = s.view.rank();

            for t in sides {
                if t.view == s.view {
                    continue;
                }
                // Condition 1: same oriented face.
                let candidate_score = observation_score(sigma, normal, t.forward);
                if candidate_score <= 0.0 {
                    continue;
                }
                let rel = sub3(p, t.origin);
                // Condition 2: reach, identical quantized rule to the
                // reachability filter; origin_T lies on T's face plane so
                // this dot IS d_T(p).
                let d_t = dot3(rel, t.forward);
                // No `.max(0)` floor here, unlike pass 1's defensive clamp:
                // `p` is reconstructed from S's own in-bounds screen/depth
                // sample, so it lies inside the model box and its distance
                // to any face plane is non-negative by construction.
                let relief_t = (d_t * RELIEF_UNITS_PER_PIXEL as f64).round() as i64;
                if relief_t > t.h_max {
                    continue;
                }
                // Condition 3: in-bounds.
                let u = dot3(rel, t.right);
                let v = dot3(rel, t.down);
                let (tx, ty) = (u.floor(), v.floor());
                if tx < 0.0 || ty < 0.0 || tx >= f64::from(t.width) || ty >= f64::from(t.height) {
                    continue;
                }
                let (tex_x, tex_y) = (tx as u32, ty as u32);
                let t_index = t.index(tex_x, tex_y);
                // Condition 4: visible — T's filtered depth at the
                // projected texel is finite and within tolerance of d_T(p).
                let z = t.depth[t_index];
                if !z.is_finite() {
                    continue;
                }
                let grad = local_gradient(&t.depth, t.width, t.height, tex_x, tex_y, z);
                // half-texel-diagonal times local slope bounds the first-
                // hit surface's variation across the texel footprint (p
                // projects up to sqrt(2)/2 from the compared center), plus
                // one relief quantum (1/8 px) absorbing the quantization
                // asymmetry between the two sides' rules. Using the max
                // difference is deliberately conservative toward "visible":
                // consistent overlap composites harmlessly, a hole shows
                // background.
                let tol =
                    grad * std::f64::consts::FRAC_1_SQRT_2 + 1.0 / RELIEF_UNITS_PER_PIXEL as f64;
                if d_t > z + tol {
                    continue;
                }

                if candidate_score > best_score
                    || (candidate_score == best_score && t.view.rank() < best_rank)
                {
                    best_score = candidate_score;
                    best_rank = t.view.rank();
                    owner = t.view;
                }
            }

            kept[idx] = owner == s.view;
        }
    }
    kept
}

/// Dilates `kept` by one texel (4-neighborhood), intersected with `covered`
/// so dilation never adds a texel the side never actually reached. Tent
/// interpolation's support extends one texel past the last kept center, so
/// without this ring a strict ownership partition would open sub-texel gaps
/// where differently-owned regions abut; because the ring is drawn only
/// from `covered` texels it carries true (reachability-filtered) geometry,
/// so the resulting overlap is consistent, not synthesized.
pub(crate) fn dilate_keep_mask(
    kept: &[bool],
    covered: &[bool],
    width: u32,
    height: u32,
) -> Vec<bool> {
    let index = |x: u32, y: u32| (y * width + x) as usize;
    let mut out = kept.to_vec();
    for y in 0..height {
        for x in 0..width {
            let idx = index(x, y);
            if kept[idx] || !covered[idx] {
                continue;
            }
            let has_kept_neighbor = (x > 0 && kept[index(x - 1, y)])
                || (x + 1 < width && kept[index(x + 1, y)])
                || (y > 0 && kept[index(x, y - 1)])
                || (y + 1 < height && kept[index(x, y + 1)]);
            if has_kept_neighbor {
                out[idx] = true;
            }
        }
    }
    out
}

/// Largest relief difference a single continuous best-faced sheet can
/// produce between 4-adjacent texels: ownership guarantees an owned
/// sample's surface slope is <= 45 degrees (the ownership rule's own
/// candidacy bound), so one continuous sheet's relief can change by at most
/// one pixel (8 relief units, `RELIEF_UNITS_PER_PIXEL`) across one texel of
/// lateral travel, plus 2 units absorbing the pair's two independent
/// depth-quantization roundings. A larger gap is either a real steep wall
/// (a fallback-owned cavity side) or an occlusion cut — the wall-reality
/// test, not this threshold, tells them apart (spec: "Fabricated-wall
/// cuts").
const CUT_CANDIDATE_UNITS: i64 = 10;

/// Fabricated-wall cuts (spec: "Fabricated-wall cuts"), run after ownership
/// and closure and before chart encoding: every 4-adjacent pair of kept
/// texels whose relief differs by more than `CUT_CANDIDATE_UNITS` is a
/// candidate discontinuity, tested by sampling the open segment between the
/// pair's two reconstructed 3D points against the mesh (`grid`). Fabricated
/// pairs (some interior sample farther than one texel from any triangle)
/// have their far (deeper) texel collected into `drop`, which is applied to
/// `kept` only after the full scan — so a texel dropped by one pair cannot
/// change the candidacy or verdict of another pair scanned later, making
/// the result independent of scan order (spec: "two-phase"). Cuts never run
/// twice and closure never re-runs afterward, so a cut cannot be re-bridged.
fn apply_fabricated_wall_cuts(side: &CaptureSide, kept: &mut [bool], grid: &TriangleGrid) {
    let relief_at = |idx: usize| i64::from(255 - side.rgba[idx][3]);
    let mut drop = vec![false; kept.len()];

    let mut test_pair = |x0: u32, y0: u32, x1: u32, y1: u32| {
        let i0 = side.index(x0, y0);
        let i1 = side.index(x1, y1);
        if !kept[i0] || !kept[i1] {
            return;
        }
        let relief0 = relief_at(i0);
        let relief1 = relief_at(i1);
        if (relief0 - relief1).abs() <= CUT_CANDIDATE_UNITS {
            return;
        }
        let (near, far) = if relief0 <= relief1 {
            ((x0, y0, i0), (x1, y1, i1))
        } else {
            ((x1, y1, i1), (x0, y0, i0))
        };
        let p_near = side.point_at(near.0, near.1, side.depth[near.2]);
        let p_far = side.point_at(far.0, far.1, side.depth[far.2]);
        let len = distance3(p_far, p_near);
        // The pair's own endpoints lie on surface by construction and are
        // excluded; at least one interior probe is always taken, even for
        // immediately adjacent texels, so a real wall spanning exactly one
        // texel of lateral travel still gets tested once.
        let samples = (len.ceil() as i64 - 1).max(1);
        let mut real = true;
        for k in 1..=samples {
            let t = k as f64 / (samples + 1) as f64;
            // `q` is a convex combination of `p_near` and `p_far`, both
            // reconstructed from an in-bounds texel and a depth that
            // cleared the reachability filter (relief <= h_max, i.e.
            // within the model box on the forward axis, per
            // `capture_side`'s own filter); since the model box is convex,
            // every interior sample stays inside it too, so
            // `within_one_texel`'s out-of-box clamp is a defensive
            // fallback here, never live on this call path.
            let q = add3(p_near, scale3(sub3(p_far, p_near), t));
            if !grid.within_one_texel(q) {
                real = false;
                break;
            }
        }
        if !real {
            drop[far.2] = true;
        }
    };

    for y in 0..side.height {
        for x in 0..side.width {
            if x + 1 < side.width {
                test_pair(x, y, x + 1, y);
            }
            if y + 1 < side.height {
                test_pair(x, y, x, y + 1);
            }
        }
    }

    for (idx, &dropped) in drop.iter().enumerate() {
        if dropped {
            kept[idx] = false;
        }
    }
}

pub fn convert_box_space(
    box_scene: &TriangleScene,
    bounds: Bounds,
    settings: &ImportSettings,
) -> Result<AuthoredModel, ImportError> {
    settings.side_modes.validate()?;
    let lighting = Lighting {
        direction: light_direction(
            settings.light_azimuth_degrees,
            settings.light_elevation_degrees,
        ),
        ambient: settings.ambient,
    };

    // Pass 1: rasterize and reachability-filter every enabled Capture side.
    // Ownership (pass 2) runs only across this set; `From opposite`/`Off`
    // sides play no role.
    let sides: Vec<CaptureSide> = ALL_VIEWS
        .into_iter()
        .filter(|&view| settings.side_modes.get(view) == SideMode::Capture)
        .map(|view| capture_side(box_scene, view, bounds, &lighting))
        .collect();

    // Pass 2: ownership + one-texel closure ring, per side.
    let mut masks: Vec<Vec<bool>> = Vec::with_capacity(sides.len());
    for (s_idx, side) in sides.iter().enumerate() {
        let kept = owning_mask(&sides, s_idx);
        let covered: Vec<bool> = side.depth.iter().map(|d| d.is_finite()).collect();
        masks.push(dilate_keep_mask(&kept, &covered, side.width, side.height));
    }

    // Pass 3: fabricated-wall cuts, run after closure (so dilation cannot
    // re-bridge a cut) and before chart encoding. The triangle grid is
    // built once from every box-space triangle, independent of which sides
    // are enabled for capture.
    let grid = TriangleGrid::build(&box_scene.triangles, bounds);
    for (side, mask) in sides.iter().zip(masks.iter_mut()) {
        apply_fabricated_wall_cuts(side, mask, &grid);
    }

    let mut charts = Vec::new();
    for (side, mask) in sides.iter().zip(masks.iter()) {
        let rgba: Vec<[u8; 4]> = side
            .rgba
            .iter()
            .zip(mask.iter())
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

#[cfg(test)]
mod tests {
    use super::dilate_keep_mask;

    /// 5x5 grid; `kept` is a plus shape centered at (2,2). `covered` is
    /// everything except (0,2) — a texel that borders the kept texel (1,2)
    /// but was never reached by this side, so it must stay excluded even
    /// though it geometrically borders a kept texel. Dilation must add
    /// exactly the plus's other seven orthogonal neighbors (its full
    /// one-texel ring, minus the uncovered exception) and nothing else.
    #[test]
    fn dilate_keep_mask_adds_covered_orthogonal_neighbors_only() {
        let width = 5u32;
        let height = 5u32;
        let at = |x: u32, y: u32| (y * width + x) as usize;

        let mut covered = vec![true; (width * height) as usize];
        covered[at(0, 2)] = false;

        let plus = [(2u32, 2u32), (1, 2), (3, 2), (2, 1), (2, 3)];
        let mut kept = vec![false; (width * height) as usize];
        for &(x, y) in &plus {
            kept[at(x, y)] = true;
        }

        let dilated = dilate_keep_mask(&kept, &covered, width, height);

        let ring_added = [(1u32, 1u32), (3, 1), (1, 3), (3, 3), (4, 2), (2, 0), (2, 4)];
        for &(x, y) in &ring_added {
            assert!(
                dilated[at(x, y)],
                "({x},{y}) borders a kept, covered texel and must be dilated in"
            );
        }
        assert!(
            !dilated[at(0, 2)],
            "(0,2) borders kept (1,2) but was never covered by this side; \
             dilation must not fabricate it"
        );
        for &(x, y) in &plus {
            assert!(dilated[at(x, y)], "({x},{y}) was already kept");
        }

        let mut expected_true: Vec<usize> = plus
            .iter()
            .chain(ring_added.iter())
            .map(|&(x, y)| at(x, y))
            .collect();
        expected_true.sort_unstable();
        expected_true.dedup();
        for (idx, &is_dilated) in dilated.iter().enumerate() {
            assert_eq!(
                is_dilated,
                expected_true.contains(&idx),
                "texel {idx} dilation mismatch"
            );
        }
    }
}
