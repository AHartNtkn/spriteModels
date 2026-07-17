//! Import dialog state and recompute logic. The temporary stub modal in
//! `app.rs` only calls `ensure_converted`; the rest of this surface (drag,
//! snap, presets, and the conversion result) is proven correct by the tests
//! below and is wired into the real dialog UI in the next task, which must
//! remove this module-level allow once it consumes the surface directly.
#![allow(dead_code)]

use editor_core::{EditorDocument, OrbitCamera, PreviewCache};
use mesh_import::{ImportSettings, TriangleScene, convert};

const MODEL_DRAG_DEGREES_PER_POINT: f32 = 0.25; // same feel as camera orbit

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OrientationPreset {
    ZUpToYUp,
    FlipX,
    FlipY,
    FlipZ,
}

pub(crate) struct ConvertedPreview {
    pub document: EditorDocument,
    pub preview: PreviewCache,
}

pub(crate) struct ImportDialogState {
    pub scene: TriangleScene,
    pub file_label: String,
    pub settings: ImportSettings,
    pub camera: OrbitCamera,
    pub zoom_milli: u32,
    pub converted: Result<ConvertedPreview, String>,
    last_settings: Option<ImportSettings>,
    conversions: u64,
}

impl ImportDialogState {
    pub fn new(scene: TriangleScene, file_label: String) -> Self {
        Self {
            scene,
            file_label,
            settings: ImportSettings::default(),
            camera: OrbitCamera::default(),
            zoom_milli: 1_000,
            converted: Err(String::from("not yet converted")),
            last_settings: None,
            conversions: 0,
        }
    }

    pub fn ensure_converted(&mut self) {
        if self.last_settings.as_ref() == Some(&self.settings) {
            return;
        }
        self.last_settings = Some(self.settings.clone());
        self.conversions += 1;
        self.converted = match convert(&self.scene, &self.settings) {
            Ok(model) => Ok(ConvertedPreview {
                document: EditorDocument::from_model(model, None),
                preview: PreviewCache::default(),
            }),
            Err(error) => Err(error.to_string()),
        };
    }

    pub fn conversion_count(&self) -> u64 {
        self.conversions
    }

    pub fn orbit_drag(&mut self, dx: f32, dy: f32) {
        self.camera.drag(dx, dy);
    }

    pub fn model_drag(&mut self, dx: f32, dy: f32) {
        let basis = self.camera.basis_f32();
        let yaw = rotation_about(basis[1], dx * MODEL_DRAG_DEGREES_PER_POINT.to_radians());
        let pitch = rotation_about(basis[0], dy * MODEL_DRAG_DEGREES_PER_POINT.to_radians());
        self.settings.rotation =
            orthonormalized(multiply(pitch, multiply(yaw, self.settings.rotation)));
    }

    pub fn snap_rotation(&mut self) {
        let r = self.settings.rotation;
        let mut snapped = [[0.0f32; 3]; 3];
        let mut used = [false; 3];
        // Rows in order of their strongest component (most confident first)
        // so the strongest alignments win their axes.
        let mut order: Vec<usize> = (0..3).collect();
        let strength = |row: [f32; 3]| row.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        order.sort_by(|&a, &b| strength(r[b]).partial_cmp(&strength(r[a])).unwrap());
        let mut weakest = (order[2], 0usize, f32::INFINITY);
        for &i in &order {
            let (mut best, mut best_abs) = (usize::MAX, -1.0f32);
            for j in 0..3 {
                if !used[j] && r[i][j].abs() > best_abs {
                    best = j;
                    best_abs = r[i][j].abs();
                }
            }
            used[best] = true;
            snapped[i][best] = r[i][best].signum();
            if best_abs < weakest.2 {
                weakest = (i, best, best_abs);
            }
        }
        let det = snapped[0][0] * (snapped[1][1] * snapped[2][2] - snapped[1][2] * snapped[2][1])
            - snapped[0][1] * (snapped[1][0] * snapped[2][2] - snapped[1][2] * snapped[2][0])
            + snapped[0][2] * (snapped[1][0] * snapped[2][1] - snapped[1][1] * snapped[2][0]);
        if det < 0.0 {
            // A reflection is not an orientation; negating the least
            // confident row is the smallest change restoring det = +1.
            snapped[weakest.0][weakest.1] = -snapped[weakest.0][weakest.1];
        }
        self.settings.rotation = snapped;
    }

    pub fn apply_preset(&mut self, preset: OrientationPreset) {
        let rotation = match preset {
            // -90 about X: +y -> +z, +z -> -y (glTF Y-up from Z-up sources).
            OrientationPreset::ZUpToYUp => [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]],
            OrientationPreset::FlipX => [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]],
            OrientationPreset::FlipY => [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]],
            OrientationPreset::FlipZ => [[-1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]],
        };
        self.settings.rotation = multiply(rotation, self.settings.rotation);
    }
}

fn multiply(a: [[f32; 3]; 3], b: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let mut out = [[0.0f32; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}

/// Rodrigues rotation matrix about a unit axis.
fn rotation_about(axis: [f32; 3], angle: f32) -> [[f32; 3]; 3] {
    let (sin, cos) = angle.sin_cos();
    let one_minus = 1.0 - cos;
    let [x, y, z] = axis;
    [
        [
            cos + x * x * one_minus,
            x * y * one_minus - z * sin,
            x * z * one_minus + y * sin,
        ],
        [
            y * x * one_minus + z * sin,
            cos + y * y * one_minus,
            y * z * one_minus - x * sin,
        ],
        [
            z * x * one_minus - y * sin,
            z * y * one_minus + x * sin,
            cos + z * z * one_minus,
        ],
    ]
}

/// Gram-Schmidt on rows: keeps incremental drag rotations from drifting
/// away from orthonormality.
fn orthonormalized(m: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let normalize = |v: [f32; 3]| {
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        [v[0] / len, v[1] / len, v[2] / len]
    };
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let r0 = normalize(m[0]);
    let p = dot(m[1], r0);
    let r1 = normalize([
        m[1][0] - p * r0[0],
        m[1][1] - p * r0[1],
        m[1][2] - p * r0[2],
    ]);
    let r2 = [
        r0[1] * r1[2] - r0[2] * r1[1],
        r0[2] * r1[0] - r0[0] * r1[2],
        r0[0] * r1[1] - r0[1] * r1[0],
    ];
    [r0, r1, r2]
}

#[cfg(test)]
mod tests {
    use super::*;
    use mesh_import::{Material, Triangle, TriangleScene};

    fn quad_scene() -> TriangleScene {
        let tri = |a: [f32; 3], b: [f32; 3], c: [f32; 3]| Triangle {
            positions: [a, b, c],
            normals: [[0.0, 0.0, -1.0]; 3],
            uvs: [[0.0, 0.0]; 3],
            colors: [[1.0, 1.0, 1.0, 1.0]; 3],
            material: 0,
        };
        TriangleScene {
            triangles: vec![
                tri([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.5]),
                tri([0.0, 0.0, 0.0], [1.0, 1.0, 0.5], [0.0, 1.0, 0.5]),
            ],
            materials: vec![Material {
                base_color_factor: [1.0, 1.0, 1.0, 1.0],
                base_color_texture: None,
                alpha_cutoff: None,
            }],
        }
    }

    #[test]
    fn conversion_runs_once_per_settings_change() {
        let mut state = ImportDialogState::new(quad_scene(), "quad.glb".into());
        state.ensure_converted();
        state.ensure_converted();
        assert_eq!(
            state.conversion_count(),
            1,
            "unchanged settings must not reconvert"
        );

        state.settings.longest_axis_pixels = 32;
        state.ensure_converted();
        assert_eq!(state.conversion_count(), 2);

        state.orbit_drag(10.0, 5.0);
        state.ensure_converted();
        assert_eq!(state.conversion_count(), 2, "camera orbit never reconverts");

        state.model_drag(10.0, 0.0);
        state.ensure_converted();
        assert_eq!(state.conversion_count(), 3, "model rotation reconverts");
    }

    #[test]
    fn model_drag_keeps_rotation_orthonormal() {
        let mut state = ImportDialogState::new(quad_scene(), "quad.glb".into());
        for _ in 0..500 {
            state.model_drag(7.3, -3.1);
        }
        let r = state.settings.rotation;
        for i in 0..3 {
            let len = (0..3).map(|j| r[i][j] * r[i][j]).sum::<f32>().sqrt();
            assert!((len - 1.0).abs() < 1e-3, "row {i} length {len}");
            for k in (i + 1)..3 {
                let dot: f32 = (0..3).map(|j| r[i][j] * r[k][j]).sum();
                assert!(dot.abs() < 1e-3, "rows {i},{k} not orthogonal: {dot}");
            }
        }
    }

    #[test]
    fn snap_lands_on_a_signed_permutation_with_determinant_one() {
        let mut state = ImportDialogState::new(quad_scene(), "quad.glb".into());
        state.model_drag(40.0, 25.0); // ~10 and ~6 degrees: near identity
        state.snap_rotation();
        let r = state.settings.rotation;
        let mut ones = 0;
        for row in r {
            for value in row {
                assert!(
                    value == 0.0 || value == 1.0 || value == -1.0,
                    "snap must produce a signed permutation, got {value}"
                );
                if value != 0.0 {
                    ones += 1;
                }
            }
        }
        assert_eq!(ones, 3);
        let det = r[0][0] * (r[1][1] * r[2][2] - r[1][2] * r[2][1])
            - r[0][1] * (r[1][0] * r[2][2] - r[1][2] * r[2][0])
            + r[0][2] * (r[1][0] * r[2][1] - r[1][1] * r[2][0]);
        assert_eq!(det, 1.0);
        // Near identity snaps TO identity.
        assert_eq!(r, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    }

    #[test]
    fn flip_presets_are_involutions_and_z_up_preset_rotates_about_x() {
        let mut state = ImportDialogState::new(quad_scene(), "quad.glb".into());
        let before = state.settings.rotation;
        state.apply_preset(OrientationPreset::FlipY);
        state.apply_preset(OrientationPreset::FlipY);
        for (after_row, before_row) in state.settings.rotation.iter().zip(before.iter()) {
            for (after, before) in after_row.iter().zip(before_row.iter()) {
                assert!((after - before).abs() < 1e-6);
            }
        }
        state.apply_preset(OrientationPreset::ZUpToYUp);
        // -90 about X maps +z to -y (box up).
        let r = state.settings.rotation;
        let mapped_z = [r[0][2], r[1][2], r[2][2]];
        assert!(
            (mapped_z[1] + 1.0).abs() < 1e-6,
            "+z must map to -y, got {mapped_z:?}"
        );
    }

    #[test]
    fn conversion_error_is_stored_not_panicked() {
        let mut state = ImportDialogState::new(quad_scene(), "quad.glb".into());
        state.settings.longest_axis_pixels = 0;
        state.ensure_converted();
        assert!(state.converted.is_err());
    }
}
