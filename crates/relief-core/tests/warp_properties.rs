use std::f64::consts::PI;

use num_rational::Ratio;
use relief_core::{
    Bounds, CanonicalView, Chart, RELIEF_UNITS_PER_PIXEL, ReliefField, SourcePoint,
    WarpCoefficients,
};

fn alpha(relief_eighths: u8) -> u8 {
    255 - relief_eighths
}

/// Source point along the inverse line at parameter `p`, reconstructed from the
/// exact per-variable `[offset, slope]` coefficients the fixed-point
/// [`relief_core::FrameInverse`] exposes.
fn source_at(variables: &[[Ratio<i64>; 2]; 3], p: Ratio<i64>) -> SourcePoint {
    SourcePoint::new(
        variables[0][0] + variables[0][1] * p,
        variables[1][0] + variables[1][1] * p,
    )
}

/// Relief along the inverse line at parameter `p`.
fn relief_at(variables: &[[Ratio<i64>; 2]; 3], p: Ratio<i64>) -> Ratio<i64> {
    variables[2][0] + variables[2][1] * p
}

#[test]
fn direct_warp_is_flat_transform_plus_relief_parallax() {
    let warp = WarpCoefficients::new([[1, 0, 10], [0, 1, 20]], [2, -1], [0, 0, 1], 3);
    let sample = warp.apply(
        SourcePoint::new(Ratio::from_integer(4), Ratio::from_integer(5)),
        Ratio::from_integer(8),
    );

    assert_eq!(sample.screen_x, Ratio::from_integer(30));
    assert_eq!(sample.screen_y, Ratio::from_integer(17));
    assert_eq!(sample.depth, Ratio::from_integer(25));
}

#[test]
fn direct_warp_keeps_fractional_source_and_relief_exact() {
    let warp = WarpCoefficients::new([[3, -2, 1], [-1, 4, -3]], [-2, 5], [2, 1, 7], -3);
    let sample = warp.apply(
        SourcePoint::new(Ratio::new(1, 3), Ratio::new(5, 2)),
        Ratio::new(7, 4),
    );

    assert_eq!(sample.screen_x, Ratio::new(-13, 2));
    assert_eq!(sample.screen_y, Ratio::new(185, 12));
    assert_eq!(sample.depth, Ratio::new(59, 12));
}

#[test]
fn owning_foreground_cell_has_an_exact_one_sided_closure_limit() {
    let chart = Chart::from_rgba(
        CanonicalView::Front,
        2,
        1,
        vec![[1, 2, 3, alpha(6)], [0, 0, 0, 0]],
    )
    .unwrap();
    let field = ReliefField::new(&chart);
    let public_boundary = SourcePoint::new(Ratio::from_integer(1), Ratio::from_integer(1));

    assert_eq!(field.sample(public_boundary.x, public_boundary.y), None);
    assert_eq!(
        field
            .foreground_cell(0, 0)
            .unwrap()
            .sample_closure(public_boundary),
        Some(Ratio::from_integer(6))
    );
}

#[test]
fn closure_limit_uses_only_the_owning_foreground_component() {
    let chart = Chart::from_rgba(
        CanonicalView::Front,
        3,
        1,
        vec![[1, 0, 0, alpha(2)], [0, 0, 0, 0], [2, 0, 0, alpha(30)]],
    )
    .unwrap();
    let field = ReliefField::new(&chart);

    assert_eq!(
        field
            .foreground_cell(0, 0)
            .unwrap()
            .sample_closure(SourcePoint::new(Ratio::from_integer(1), Ratio::new(1, 2),)),
        Some(Ratio::from_integer(2))
    );
    assert_eq!(
        field
            .foreground_cell(2, 0)
            .unwrap()
            .sample_closure(SourcePoint::new(Ratio::from_integer(2), Ratio::new(1, 2),)),
        Some(Ratio::from_integer(30))
    );
}

#[test]
fn direct_warp_accepts_exact_rational_camera_coefficients() {
    let zero = Ratio::from_integer(0);
    let warp = WarpCoefficients::from_rational(
        [
            [Ratio::new(1, 2), zero, Ratio::new(3, 2)],
            [zero, Ratio::new(1, 4), Ratio::from_integer(-2)],
        ],
        [Ratio::new(1, 8), Ratio::new(-1, 8)],
        [zero, zero, Ratio::from_integer(3)],
        Ratio::new(1, 8),
    );

    let sample = warp.apply(
        SourcePoint::new(Ratio::from_integer(2), Ratio::from_integer(4)),
        Ratio::from_integer(8),
    );

    assert_eq!(sample.screen_x, Ratio::new(7, 2));
    assert_eq!(sample.screen_y, Ratio::from_integer(-2));
    assert_eq!(sample.depth, Ratio::from_integer(4));
}

#[test]
fn inverse_warp_line_recovers_a_known_preimage_with_its_selected_parameter() {
    let warp = WarpCoefficients::new([[2, 1, 3], [-1, 2, 5]], [3, -2], [4, -3, 7], 5);
    let source = SourcePoint::new(Ratio::new(3, 2), Ratio::new(5, 4));
    let relief = Ratio::new(7, 3);
    let warped = warp.apply(source.clone(), relief);

    let frame = warp
        .prepare_inverse()
        .expect("front-facing affine chart transform is invertible")
        .inverse_frame(warped.screen_x, warped.screen_y, 1, 1);
    let variables = frame.variable_coefficients_exact(0, 0);

    assert_eq!(source_at(&variables, source.x), source);
    assert_eq!(relief_at(&variables, source.x), relief);
    assert_eq!(frame.depth_at(0, 0, source.x), warped.depth);
}

#[test]
fn inverse_warp_line_uses_source_y_as_parameter_when_x_and_relief_span_screen() {
    let warp = WarpCoefficients::new([[2, 0, 3], [0, 0, -4]], [0, 5], [7, 11, 13], 17);
    let source = SourcePoint::new(Ratio::new(3, 2), Ratio::new(5, 4));
    let relief = Ratio::new(7, 3);
    let warped = warp.apply(source.clone(), relief);

    let frame = warp
        .prepare_inverse()
        .expect("source x and relief form a rank-two projected map")
        .inverse_frame(warped.screen_x, warped.screen_y, 1, 1);
    let variables = frame.variable_coefficients_exact(0, 0);

    assert_eq!(source_at(&variables, source.y), source);
    assert_eq!(frame.depth_at(0, 0, source.y), warped.depth);
}

#[test]
fn inverse_warp_line_uses_source_x_as_parameter_at_exact_canonical_edge_on() {
    let warp = WarpCoefficients::new([[0, 0, 3], [0, 4, -4]], [5, 0], [7, 11, 13], 17);
    let source = SourcePoint::new(Ratio::new(3, 2), Ratio::new(5, 4));
    let relief = Ratio::new(7, 3);
    let warped = warp.apply(source.clone(), relief);

    let frame = warp
        .prepare_inverse()
        .expect("source y and relief form a rank-two projected map")
        .inverse_frame(warped.screen_x, warped.screen_y, 1, 1);
    let variables = frame.variable_coefficients_exact(0, 0);

    assert_eq!(source_at(&variables, source.x), source);
    assert_eq!(frame.depth_at(0, 0, source.x), warped.depth);
}

#[test]
fn inverse_warp_line_rejects_a_rank_one_projected_map() {
    let warp = WarpCoefficients::new([[2, 4, 3], [0, 0, -4]], [6, 0], [7, 11, 13], 17);

    // Singularity is a property of the matrix alone, so it is rejected once at
    // preparation time rather than per pixel.
    assert!(warp.prepare_inverse().is_none());
}

#[test]
fn inverse_warp_line_exposes_every_affine_coordinate_and_transient_depth() {
    let warp = WarpCoefficients::new([[2, 0, 3], [0, 0, -4]], [0, 5], [7, 11, 13], 17);
    let source = SourcePoint::new(Ratio::new(3, 2), Ratio::new(5, 4));
    let relief = Ratio::new(7, 3);
    let warped = warp.apply(source, relief);
    let frame = warp
        .prepare_inverse()
        .expect("source x and relief form a rank-two projected map")
        .inverse_frame(warped.screen_x, warped.screen_y, 1, 1);
    let variables = frame.variable_coefficients_exact(0, 0);

    assert_eq!(
        variables,
        [
            [Ratio::new(3, 2), Ratio::from_integer(0)],
            [Ratio::from_integer(0), Ratio::from_integer(1)],
            [Ratio::new(7, 3), Ratio::from_integer(0)],
        ]
    );
    assert_eq!(
        frame.depth_coefficients_exact(0, 0),
        [Ratio::new(379, 6), Ratio::from_integer(11)]
    );
    assert_eq!(relief_at(&variables, Ratio::new(5, 4)), Ratio::new(7, 3));
}

#[test]
fn inverse_warp_line_selects_the_largest_determinant_with_stable_ties() {
    let warp = WarpCoefficients::new([[1, 0, 0], [0, 1, 0]], [100, 100], [0, 0, 0], 1);
    let warped = warp.apply(
        SourcePoint::new(Ratio::from_integer(2), Ratio::from_integer(3)),
        Ratio::from_integer(5),
    );
    let frame = warp
        .prepare_inverse()
        .expect("every projected column pair has rank two")
        .inverse_frame(warped.screen_x, warped.screen_y, 1, 1);

    assert_eq!(
        frame.variable_coefficients_exact(0, 0),
        [
            [Ratio::from_integer(-1), Ratio::from_integer(1)],
            [Ratio::from_integer(0), Ratio::from_integer(1)],
            [Ratio::new(503, 100), Ratio::new(-1, 100)],
        ],
        "(x,h) must beat the smaller (x,y) determinant and win its tie with (y,h)"
    );
}

/// `(screen_right, screen_down, depth)`.
type CameraBasis = ([Ratio<i64>; 3], [Ratio<i64>; 3], [Ratio<i64>; 3]);

/// Quantizes a camera-basis component to denominator 1024, replicating the
/// editor's `quantized_ratio` (crates/editor-core/src/camera.rs). relief-core
/// cannot depend on editor-core, so the construction is copied here rather
/// than imported.
fn quantized_ratio(value: f64) -> Ratio<i64> {
    const BASIS_DENOMINATOR: i64 = 1_024;
    Ratio::new(
        (value * BASIS_DENOMINATOR as f64).round() as i64,
        BASIS_DENOMINATOR,
    )
}

/// Reconstructs the editor's orbit-camera basis for a yaw/pitch pair given in
/// millidegrees, replicating `OrbitCamera::target_view`
/// (crates/editor-core/src/camera.rs) line for line — same source of this
/// copy as [`quantized_ratio`] above.
fn editor_camera_basis(yaw_millidegrees: i32, pitch_millidegrees: i32) -> CameraBasis {
    let yaw = f64::from(yaw_millidegrees) * PI / 180_000.0;
    let pitch = f64::from(pitch_millidegrees) * PI / 180_000.0;
    let (sin_yaw, cos_yaw) = yaw.sin_cos();
    let (sin_pitch, cos_pitch) = pitch.sin_cos();

    let screen_right = [cos_yaw, 0.0, sin_yaw].map(quantized_ratio);
    let screen_down = [sin_yaw * sin_pitch, cos_pitch, -cos_yaw * sin_pitch].map(quantized_ratio);
    let depth = [-sin_yaw * cos_pitch, sin_pitch, cos_yaw * cos_pitch].map(quantized_ratio);
    (screen_right, screen_down, depth)
}

fn dot(a: &[Ratio<i64>; 3], b: &[Ratio<i64>; 3]) -> Ratio<i64> {
    a.iter()
        .zip(b)
        .fold(Ratio::from_integer(0), |sum, (left, right)| {
            sum + *left * *right
        })
}

/// Composes a camera basis with a chart's canonical frame into
/// [`WarpCoefficients`], replicating `TargetView::warp_coefficients`'
/// `compose` helper (crates/relief-render/src/presets.rs). relief-core cannot
/// depend on relief-render, so the composition is rebuilt here directly on
/// top of `CanonicalView::frame`, which relief-core does export.
fn warp_coefficients_for(
    camera: &CameraBasis,
    view: CanonicalView,
    bounds: Bounds,
) -> WarpCoefficients {
    let (screen_right, screen_down, depth) = camera;
    let frame = view.frame(bounds);
    let origin = frame.origin.map(Ratio::from_integer);
    let source_x = frame.source_u.map(Ratio::from_integer);
    let source_y = frame.source_v.map(Ratio::from_integer);
    let inward = frame.inward.map(Ratio::from_integer);
    let relief_unit = Ratio::new(1, RELIEF_UNITS_PER_PIXEL);

    WarpCoefficients::from_rational(
        [
            [
                dot(screen_right, &source_x),
                dot(screen_right, &source_y),
                dot(screen_right, &origin),
            ],
            [
                dot(screen_down, &source_x),
                dot(screen_down, &source_y),
                dot(screen_down, &origin),
            ],
        ],
        [
            dot(screen_right, &inward) * relief_unit,
            dot(screen_down, &inward) * relief_unit,
        ],
        [
            dot(depth, &source_x),
            dot(depth, &source_y),
            dot(depth, &origin),
        ],
        dot(depth, &inward) * relief_unit,
    )
}

/// Reproduces the screen-rectangle-origin computation `render_model` performs
/// (crates/relief-render/src/compositor.rs) — the projected extents of the
/// model bounds under this camera, centered in a `side x side` frame — so the
/// `(sx0, sy0)` passed to `inverse_frame` below match exactly what the real
/// render path passes.
fn frame_origin(camera: &CameraBasis, bounds: Bounds, side: i64) -> (Ratio<i64>, Ratio<i64>) {
    let (screen_right, screen_down, _depth) = camera;
    let axes = [
        [0i64, i64::from(bounds.width())],
        [0i64, i64::from(bounds.height())],
        [0i64, i64::from(bounds.depth())],
    ];
    let mut min_x = None;
    let mut max_x = None;
    let mut min_y = None;
    let mut max_y = None;
    for x in axes[0] {
        for y in axes[1] {
            for z in axes[2] {
                let corner = [
                    Ratio::from_integer(x),
                    Ratio::from_integer(y),
                    Ratio::from_integer(z),
                ];
                let screen_x = dot(screen_right, &corner);
                let screen_y = dot(screen_down, &corner);
                min_x = Some(min_x.map_or(screen_x, |value: Ratio<i64>| value.min(screen_x)));
                max_x = Some(max_x.map_or(screen_x, |value: Ratio<i64>| value.max(screen_x)));
                min_y = Some(min_y.map_or(screen_y, |value: Ratio<i64>| value.min(screen_y)));
                max_y = Some(max_y.map_or(screen_y, |value: Ratio<i64>| value.max(screen_y)));
            }
        }
    }
    let (min_x, max_x, min_y, max_y) = (
        min_x.unwrap(),
        max_x.unwrap(),
        min_y.unwrap(),
        max_y.unwrap(),
    );
    let offset_x = Ratio::new(side, 2) - (min_x + max_x) / 2;
    let offset_y = Ratio::new(side, 2) - (min_y + max_y) / 2;
    (Ratio::new(1, 2) - offset_x, Ratio::new(1, 2) - offset_y)
}

/// The review for Task 4's fixed-point rewrite verified, by a one-off sweep,
/// that no legitimate camera/bounds/frame combination trips the setup asserts
/// in [`AffineForm::from_rats`] and [`PreparedInverse::inverse_frame`] (see
/// `crates/relief-core/src/warp.rs`). This test commits that sweep so a
/// regression (e.g. a future change to the quantization denominator, the
/// pitch range, or the affine-form composition) is caught instead of
/// silently reintroducing a panic in the field.
///
/// The grid: every editor-reachable yaw (millidegrees, full turn) and pitch
/// (millidegrees, the editor's clamped range) at a 5000-millidegree stride,
/// crossed with the 6 canonical chart frames and 3 representative bounds
/// (a cube and the two extreme flat slabs). For each combination this runs
/// the exact setup path `render_model` runs — `prepare_inverse` then
/// `inverse_frame` with the screen-rectangle corners `render_model` computes
/// — at a frame side of 1024, larger than any native preview size. A panic
/// anywhere in this sweep fails the test.
#[test]
fn editor_camera_domain_never_trips_fixed_point_certification() {
    const FRAME_SIDE: u32 = 1024;
    const YAW_MIN: i32 = -180_000;
    const YAW_MAX: i32 = 180_000;
    const PITCH_MIN: i32 = -80_000;
    const PITCH_MAX: i32 = 80_000;
    const STEP: i32 = 5_000;

    let views = [
        CanonicalView::Front,
        CanonicalView::Back,
        CanonicalView::Left,
        CanonicalView::Right,
        CanonicalView::Top,
        CanonicalView::Bottom,
    ];
    let bounds_list = [
        Bounds::new(63, 63, 63).unwrap(),
        Bounds::new(63, 1, 63).unwrap(),
        Bounds::new(1, 63, 63).unwrap(),
    ];

    let mut combinations = 0usize;
    let mut yaw = YAW_MIN;
    while yaw < YAW_MAX {
        let mut pitch = PITCH_MIN;
        while pitch <= PITCH_MAX {
            let camera = editor_camera_basis(yaw, pitch);
            for &bounds in &bounds_list {
                let origin = frame_origin(&camera, bounds, i64::from(FRAME_SIDE));
                for &view in &views {
                    let warp = warp_coefficients_for(&camera, view, bounds);
                    let prepared = warp.prepare_inverse().unwrap_or_else(|| {
                        panic!(
                            "camera (yaw {yaw}md, pitch {pitch}md) is singular against \
                             {view:?} at bounds {bounds:?}; every editor-reachable \
                             orientation composed with a canonical chart frame is expected \
                             to be non-singular"
                        )
                    });
                    let _frame = prepared.inverse_frame(origin.0, origin.1, FRAME_SIDE, FRAME_SIDE);
                    combinations += 1;
                }
            }
            pitch += STEP;
        }
        yaw += STEP;
    }

    assert_eq!(
        combinations,
        72 * 33 * 3 * 6,
        "grid shape drifted from the documented yaw x pitch x bounds x view coverage"
    );
}
