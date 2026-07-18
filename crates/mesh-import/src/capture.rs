use relief_core::{AuthoredModel, Bounds, CanonicalView, Chart, RELIEF_UNITS_PER_PIXEL};

use crate::continuity::{SideContinuity, SideView};
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

/// One enabled Capture side's rasterization after the reachability filter
/// (Task 12's midplane drop) but before ownership: everything ownership
/// needs to query this side as either the observing side or a candidate
/// owner for another side's hit.
pub(crate) struct CaptureSide {
    pub(crate) view: CanonicalView,
    pub(crate) origin: [f64; 3],
    pub(crate) right: [f64; 3],
    pub(crate) down: [f64; 3],
    pub(crate) forward: [f64; 3],
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) h_max: i64,
    /// Reachability-filtered depth in model pixels, exact (not quantized):
    /// `f64::INFINITY` marks a texel that is either uncovered or whose hit
    /// quantized past `h_max` and was dropped.
    pub(crate) depth: Vec<f64>,
    /// Geometric face normal of the winning triangle at each texel;
    /// meaningless (never read) where `depth` is not finite.
    pub(crate) face_normal: Vec<[f32; 3]>,
    /// Fully encoded RGBA (relief-quantized alpha, dropped texels already
    /// `[0,0,0,0]`) before ownership decides which texels this side keeps.
    pub(crate) rgba: Vec<[u8; 4]>,
    /// Winning triangle index per covered texel (u32::MAX elsewhere),
    /// straight from the rasterizer; continuity's cross-section verdicts
    /// anchor each sample on its own triangle through this.
    pub(crate) winning: Vec<u32>,
}

impl CaptureSide {
    pub(crate) fn index(&self, x: u32, y: u32) -> usize {
        (y * self.width + x) as usize
    }

    /// The 3D point a texel's reconstructed depth corresponds to, per the
    /// registration identity `p = origin + (x+0.5)*right + (y+0.5)*down +
    /// depth*forward` (Task 11 pins this reconstruction/projection
    /// round-trip as exact).
    pub(crate) fn point_at(&self, x: u32, y: u32, depth: f64) -> [f64; 3] {
        add3(
            add3(self.origin, scale3(self.right, f64::from(x) + 0.5)),
            add3(
                scale3(self.down, f64::from(y) + 0.5),
                scale3(self.forward, depth),
            ),
        )
    }

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
}

pub(crate) fn capture_side(
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
    let mut winning = vec![u32::MAX; count];
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
        winning[i] = raster.triangle[i];
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
        winning,
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
/// tolerance in `sees_point` below.
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

/// Cross-side reference: candidate side `side` (index into the enabled
/// sides slice) would represent a sample at its texel `index`.
pub(crate) struct BetterCandidate {
    pub(crate) side: usize,
    pub(crate) index: usize,
}

/// Conditions 2-4 of the ownership rule for candidate side `t` and point
/// `p`: reach (quantized, identical to the capture filter), in-bounds
/// projection, and visibility against `t`'s reachability-filtered buffer
/// within the gradient-derived tolerance. Returns the texel of `t` that
/// represents `p`. Candidate collection, the fixpoint's rescue queries,
/// and the property tests all share this one definition of "t sees p".
pub(crate) fn sees_point(t: &CaptureSide, p: [f64; 3]) -> Option<usize> {
    let rel = sub3(p, t.origin);
    // Condition 2: reach, identical quantized rule to the reachability
    // filter; origin_T lies on T's face plane so this dot IS d_T(p).
    let d_t = dot3(rel, t.forward);
    // No `.max(0)` floor here, unlike pass 1's defensive clamp: `p` is
    // reconstructed from S's own in-bounds screen/depth sample, so it lies
    // inside the model box and its distance to any face plane is
    // non-negative by construction.
    let relief_t = (d_t * RELIEF_UNITS_PER_PIXEL as f64).round() as i64;
    if relief_t > t.h_max {
        return None;
    }
    // Condition 3: in-bounds.
    let u = dot3(rel, t.right);
    let v = dot3(rel, t.down);
    let (tx, ty) = (u.floor(), v.floor());
    if tx < 0.0 || ty < 0.0 || tx >= f64::from(t.width) || ty >= f64::from(t.height) {
        return None;
    }
    let (tex_x, tex_y) = (tx as u32, ty as u32);
    let t_index = t.index(tex_x, tex_y);
    // Condition 4: visible — T's filtered depth at the projected texel is
    // finite and within tolerance of d_T(p).
    let z = t.depth[t_index];
    if !z.is_finite() {
        return None;
    }
    let grad = local_gradient(&t.depth, t.width, t.height, tex_x, tex_y, z);
    // half-texel-diagonal times local slope bounds the first-hit surface's
    // variation across the texel footprint (p projects up to sqrt(2)/2 from
    // the compared center), plus one relief quantum (1/8 px) absorbing the
    // quantization asymmetry between the two sides' rules. Using the max
    // difference is deliberately conservative toward "visible": consistent
    // overlap composites harmlessly, a hole shows background.
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
pub(crate) fn better_candidates(
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
    // sigma = +1 when S observes the front face (-n.forward_S >= 0); exact
    // 0 (grazing incidence) is conventionally front.
    let sigma = if dot3(normal, s.forward) <= 0.0 {
        1.0
    } else {
        -1.0
    };
    // S is always a candidate for its own hit; no seen-and-reachable point
    // is orphaned when its ideal side is occluded or disabled.
    let own_score = observation_score(sigma, normal, s.forward);
    let own_rank = s.view.rank();
    let mut better = Vec::new();
    for (t_idx, t) in sides.iter().enumerate() {
        if t_idx == s_idx {
            continue;
        }
        // Condition 1: same oriented face.
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
    let state = OwnershipState { kept, banned };
    // The resolution pass above skips banned samples before it can mark
    // them kept, so a texel can never be both; this holds the invariant
    // Task 5's rewiring (kept vs. banned-but-unrescued hole) depends on.
    debug_assert!(
        state
            .kept
            .iter()
            .zip(&state.banned)
            .all(|(k, b)| k.iter().zip(b).all(|(&kept, &banned)| !(kept && banned))),
        "a texel must never be both kept and banned"
    );
    state
}

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
                || (y + 1 < height
                    && kept[index(x, y + 1)]
                    && continuity.connected(x, y, x, y + 1));
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

pub(crate) struct CapturePipeline {
    pub sides: Vec<CaptureSide>,
    /// Only the property tests' coverage theorem reads ownership after
    /// construction; `convert_box_space` needs only `sides` and `masks`,
    /// and every other consumer (the fabricated-adjacency oracle) recomputes
    /// continuity/ownership independently rather than trusting the
    /// pipeline's own labels, which is the point of an outside oracle.
    #[cfg(test)]
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
        .map(|side| {
            crate::continuity::side_continuity(&box_scene.triangles, &side.continuity_view())
        })
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
            debug_assert_chart_invariant(
                &closure.mask,
                &continuity[s_idx],
                side.width,
                side.height,
            );
            closure
        })
        .collect();
    CapturePipeline {
        sides,
        #[cfg(test)]
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

#[cfg(test)]
mod tests {
    use super::{ClosureMask, dilate_keep_mask, enforce_closure_invariant};
    use crate::continuity::SideContinuity;

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

    fn captured(
        scene: &TriangleScene,
        views: &[CanonicalView],
    ) -> (Vec<CaptureSide>, OwnershipState) {
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
                assert!(
                    ownership.kept[0][top_at(x, z)],
                    "tab texel ({x},{z}) kept by Top"
                );
                assert!(
                    !ownership.banned[0][top_at(x, z)],
                    "tab texel ({x},{z}) unbanned"
                );
            }
        }
        // Far strip behind the tab (row z = 6): banned in Top, kept by
        // Back at the texel Back sees the same point through.
        for x in 2..=5u32 {
            let idx = top_at(x, 6);
            assert!(
                ownership.banned[0][idx],
                "floor texel ({x},6) banned in Top"
            );
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
            assert!(
                ownership.banned[0][idx],
                "floor texel ({x},1) banned in Top"
            );
            let p = top.point_at(x, 1, top.depth[idx]);
            assert_eq!(
                sees_point(back, p),
                None,
                "Back cannot reach the near strip"
            );
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
            assert!(
                ownership.banned[0][idx],
                "floor texel ({x},6) banned in Top"
            );
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

        let banned = vec![false; (width * height) as usize];
        let continuity = SideContinuity::uniform(width, height, true);
        let closure = dilate_keep_mask(&kept, &covered, &banned, &continuity, width, height);
        let dilated = &closure.mask;

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

    /// Dilation must not cross a cut edge, must never re-add a banned
    /// texel, and must cross a continuous edge into a covered unbanned
    /// neighbor. Three 1x3 rows isolate each rule.
    #[test]
    fn dilation_respects_cut_edges_and_bans() {
        // Cut edge blocks: [kept, covered, covered], edge (0)-(1) cut.
        let kept = vec![true, false, false];
        let covered = vec![true, true, true];
        let banned = vec![false, false, false];
        let continuity = SideContinuity::from_edges(3, 1, vec![false, true], vec![]);
        let closure = dilate_keep_mask(&kept, &covered, &banned, &continuity, 3, 1);
        assert_eq!(
            closure.mask,
            vec![true, false, false],
            "cut edge blocks dilation"
        );

        // Ban blocks even across a continuous edge: (1) is banned.
        let banned = vec![false, true, false];
        let continuity = SideContinuity::from_edges(3, 1, vec![true, false], vec![]);
        let closure = dilate_keep_mask(&kept, &covered, &banned, &continuity, 3, 1);
        assert_eq!(
            closure.mask,
            vec![true, false, false],
            "ban blocks dilation"
        );
        assert_eq!(closure.support, vec![false, false, false]);

        // Continuous edge admits: (1) becomes support; (2) stays out
        // (its only candidate neighbor (1) is support, not kept).
        let banned = vec![false, false, false];
        let continuity = SideContinuity::from_edges(3, 1, vec![true, true], vec![]);
        let closure = dilate_keep_mask(&kept, &covered, &banned, &continuity, 3, 1);
        assert_eq!(
            closure.mask,
            vec![true, true, false],
            "continuous edge dilates one ring"
        );
        assert_eq!(closure.support, vec![false, true, false]);
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

        // Support-support across a cut at exactly equal depth: the tie
        // yields the larger index, matching the ownership pass's rule.
        let mut closure = ClosureMask {
            mask: vec![true, true, true, true],
            support: vec![false, true, true, false],
        };
        let continuity = SideContinuity::from_edges(4, 1, vec![true, false, true], vec![]);
        let depth = vec![1.0, 3.0, 3.0, 5.0];
        enforce_closure_invariant(&depth, &continuity, &mut closure, 4, 1);
        assert_eq!(closure.mask, vec![true, true, false, true]);
    }
}
