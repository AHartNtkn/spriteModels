use std::f64::consts::PI;

use num_rational::Ratio;
use relief_render::{CameraBasis, TargetView};

const DEFAULT_YAW_MILLIDEGREES: i32 = 45_000;
const DEFAULT_PITCH_MILLIDEGREES: i32 = 35_264;
const DRAG_MILLIDEGREES_PER_POINT: f64 = 250.0;
const MIN_PITCH_MILLIDEGREES: i32 = -80_000;
const MAX_PITCH_MILLIDEGREES: i32 = 80_000;
const BASIS_DENOMINATOR: i64 = 1_024;
const FULL_TURN_MILLIDEGREES: i128 = 360_000;
const HALF_TURN_MILLIDEGREES: i128 = 180_000;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct OrbitCamera {
    yaw_millidegrees: i32,
    pitch_millidegrees: i32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct OrbitOrientation {
    yaw_millidegrees: i32,
    pitch_millidegrees: i32,
}

impl OrbitCamera {
    pub fn drag(&mut self, delta_x: f32, delta_y: f32) {
        let yaw_delta = quantized_delta(delta_x, DRAG_MILLIDEGREES_PER_POINT);
        let pitch_delta = quantized_delta(delta_y, DRAG_MILLIDEGREES_PER_POINT);
        self.yaw_millidegrees = normalized_yaw(i128::from(self.yaw_millidegrees) + yaw_delta);
        self.pitch_millidegrees = (i128::from(self.pitch_millidegrees) + pitch_delta).clamp(
            i128::from(MIN_PITCH_MILLIDEGREES),
            i128::from(MAX_PITCH_MILLIDEGREES),
        ) as i32;
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn orientation(self) -> OrbitOrientation {
        OrbitOrientation {
            yaw_millidegrees: self.yaw_millidegrees,
            pitch_millidegrees: self.pitch_millidegrees,
        }
    }

    pub fn target_view(self) -> TargetView {
        let yaw = millidegrees_to_radians(self.yaw_millidegrees);
        let pitch = millidegrees_to_radians(self.pitch_millidegrees);
        let (sin_yaw, cos_yaw) = yaw.sin_cos();
        let (sin_pitch, cos_pitch) = pitch.sin_cos();

        let screen_right = [cos_yaw, 0.0, sin_yaw].map(quantized_ratio);
        let screen_down =
            [sin_yaw * sin_pitch, cos_pitch, -cos_yaw * sin_pitch].map(quantized_ratio);
        let depth = [-sin_yaw * cos_pitch, sin_pitch, cos_yaw * cos_pitch].map(quantized_ratio);

        TargetView::from_camera(CameraBasis::new(screen_right, screen_down, depth))
    }

    /// The camera basis as plain floats: rows are screen-right, screen-down,
    /// and view-forward in world coordinates. Same trigonometry as
    /// `target_view` without ratio quantization; used by the import dialog's
    /// mesh rasterizer.
    pub fn basis_f32(self) -> [[f32; 3]; 3] {
        let yaw = millidegrees_to_radians(self.yaw_millidegrees);
        let pitch = millidegrees_to_radians(self.pitch_millidegrees);
        let (sin_yaw, cos_yaw) = yaw.sin_cos();
        let (sin_pitch, cos_pitch) = pitch.sin_cos();
        [
            [cos_yaw as f32, 0.0, sin_yaw as f32],
            [
                (sin_yaw * sin_pitch) as f32,
                cos_pitch as f32,
                (-cos_yaw * sin_pitch) as f32,
            ],
            [
                (-sin_yaw * cos_pitch) as f32,
                sin_pitch as f32,
                (cos_yaw * cos_pitch) as f32,
            ],
        ]
    }
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            yaw_millidegrees: DEFAULT_YAW_MILLIDEGREES,
            pitch_millidegrees: DEFAULT_PITCH_MILLIDEGREES,
        }
    }
}

fn quantized_delta(delta: f32, units_per_input: f64) -> i128 {
    if delta.is_finite() {
        i128::from((f64::from(delta) * units_per_input).round() as i64)
    } else {
        0
    }
}

fn normalized_yaw(yaw: i128) -> i32 {
    ((yaw + HALF_TURN_MILLIDEGREES).rem_euclid(FULL_TURN_MILLIDEGREES) - HALF_TURN_MILLIDEGREES)
        as i32
}

fn millidegrees_to_radians(angle: i32) -> f64 {
    f64::from(angle) * PI / 180_000.0
}

fn quantized_ratio(value: f64) -> Ratio<i64> {
    Ratio::new(
        (value * BASIS_DENOMINATOR as f64).round() as i64,
        BASIS_DENOMINATOR,
    )
}
