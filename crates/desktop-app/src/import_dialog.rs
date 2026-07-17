//! Import dialog state, recompute logic, and the modal UI: two
//! camera-synced viewports (the raw mesh in box space, and the converted
//! relief model), orientation/side/bounds/lighting settings, and the
//! accept/cancel outcome consumed by `app.rs`.

use editor_core::{EditorDocument, OrbitCamera, PreviewCache};
use eframe::egui;
use mesh_import::{
    ImportSettings, Lighting, SideMode, TriangleScene, box_space_scene, convert_box_space,
    light_direction, rasterize,
};
use relief_core::{Bounds, CanonicalView};

use crate::{
    model_view::{color_image, presentation_scale, zoom_step},
    source_grid::view_label,
};

const MODEL_DRAG_DEGREES_PER_POINT: f32 = 0.25; // same feel as camera orbit
// Two 63x4-px viewports side by side, plus the settings rows and buttons
// below, fit comfortably inside the 1280x800 minimum window (see
// `layout::minimum_window_size`); 4px/model-px keeps a 63px model legible.
const VIEWPORT_SIZE: f32 = 360.0;

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

/// The mesh viewport rasterizes directly at screen resolution every frame it
/// changes; caching avoids redoing that work on an idle frame. Keyed on
/// everything the raster depends on: the shared camera/zoom (which also
/// drive the converted viewport), `conversions` as a generation counter for
/// every setting the raster reads (rotation, lighting, bounds fit — the same
/// counter `ensure_converted` already bumps exactly once per settings
/// change), and the viewport's pixel size.
#[derive(Clone, Copy, PartialEq)]
struct MeshRasterKey {
    camera: OrbitCamera,
    zoom_milli: u32,
    conversions: u64,
    rect_width_bits: u32,
    rect_height_bits: u32,
}

#[derive(Default)]
struct MeshViewportCache {
    key: Option<MeshRasterKey>,
    texture: Option<egui::TextureHandle>,
}

#[derive(Debug)]
pub(crate) enum ImportDialogOutcome {
    KeepOpen,
    Cancel,
    Import(relief_core::AuthoredModel),
}

pub(crate) struct ImportDialogState {
    pub scene: TriangleScene,
    pub file_label: String,
    pub settings: ImportSettings,
    pub camera: OrbitCamera,
    pub zoom_milli: u32,
    pub converted: Result<ConvertedPreview, String>,
    /// Persistent across reconversion (unlike `ConvertedPreview`, which is
    /// rebuilt every settings change) so the converted-viewport texture
    /// handle is reused via `TextureHandle::set` instead of reallocated.
    converted_texture: Option<egui::TextureHandle>,
    converted_uploaded_generation: Option<u64>,
    last_settings: Option<ImportSettings>,
    conversions: u64,
    box_space: Result<(TriangleScene, Bounds), String>,
    mesh_viewport_cache: MeshViewportCache,
    #[cfg(test)]
    pub(crate) mesh_viewport_rect: egui::Rect,
    #[cfg(test)]
    pub(crate) converted_viewport_rect: egui::Rect,
    #[cfg(test)]
    pub(crate) cancel_button_rect: egui::Rect,
    #[cfg(test)]
    pub(crate) import_button_rect: egui::Rect,
    #[cfg(test)]
    pub(crate) snap_button_rect: egui::Rect,
    /// Z-up -> Y-up, Flip X, Flip Y, Flip Z, in that order.
    #[cfg(test)]
    pub(crate) preset_button_rects: [egui::Rect; 4],
    #[cfg(test)]
    pub(crate) bounds_slider_rect: egui::Rect,
    #[cfg(test)]
    pub(crate) bounds_label_text: String,
    /// Azimuth, elevation, ambient, in that order.
    #[cfg(test)]
    pub(crate) light_slider_rects: [egui::Rect; 3],
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
            converted_texture: None,
            converted_uploaded_generation: None,
            last_settings: None,
            conversions: 0,
            box_space: Err(String::from("not yet converted")),
            mesh_viewport_cache: MeshViewportCache::default(),
            #[cfg(test)]
            mesh_viewport_rect: egui::Rect::NOTHING,
            #[cfg(test)]
            converted_viewport_rect: egui::Rect::NOTHING,
            #[cfg(test)]
            cancel_button_rect: egui::Rect::NOTHING,
            #[cfg(test)]
            import_button_rect: egui::Rect::NOTHING,
            #[cfg(test)]
            snap_button_rect: egui::Rect::NOTHING,
            #[cfg(test)]
            preset_button_rects: [egui::Rect::NOTHING; 4],
            #[cfg(test)]
            bounds_slider_rect: egui::Rect::NOTHING,
            #[cfg(test)]
            bounds_label_text: String::new(),
            #[cfg(test)]
            light_slider_rects: [egui::Rect::NOTHING; 3],
        }
    }

    pub fn ensure_converted(&mut self) {
        if self.last_settings.as_ref() == Some(&self.settings) {
            return;
        }
        self.last_settings = Some(self.settings.clone());
        self.conversions += 1;
        // A new conversion is landing: the converted-viewport texture (if
        // any) now belongs to the previous model. `PreviewCache` generation
        // counters restart at zero for the fresh `ConvertedPreview` below,
        // so without this reset a stale texture could be mistaken for
        // already matching the new frame's generation and never re-upload.
        self.converted_uploaded_generation = None;
        let box_space = box_space_scene(
            &self.scene,
            self.settings.rotation,
            self.settings.longest_axis_pixels,
        );
        self.converted = match &box_space {
            Ok((box_scene, bounds)) => convert_box_space(box_scene, *bounds, &self.settings)
                .map(|model| ConvertedPreview {
                    document: EditorDocument::from_model(model, None),
                    preview: PreviewCache::default(),
                })
                .map_err(|error| error.to_string()),
            Err(error) => Err(error.to_string()),
        };
        self.box_space = box_space.map_err(|error| error.to_string());
    }

    pub fn converted_model(&mut self) -> Option<relief_core::AuthoredModel> {
        match &self.converted {
            Ok(converted) => Some(converted.document.to_model()),
            Err(_) => None,
        }
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

    /// Snaps to the nearest axis-aligned orientation. There are only 24
    /// signed permutation matrices with determinant +1 (6 permutations x 8
    /// sign patterns, 4 of which give det +1 for each permutation), so the
    /// nearest one is found by exhaustive search rather than any greedy
    /// heuristic. Nearest in Frobenius norm is equivalent to maximizing the
    /// Frobenius inner product `sum(R[i][j] * S[i][j])`, because every
    /// candidate R has the same norm (sqrt(3)): `|R - S|^2 = |R|^2 - 2<R,S>
    /// + |S|^2`, so minimizing `|R - S|` over a fixed-norm candidate set is
    /// the same as maximizing `<R, S>`.
    pub fn snap_rotation(&mut self) {
        let r = self.settings.rotation;
        let mut best: Option<([[f32; 3]; 3], f32)> = None;
        for perm in SIGNED_PERMUTATION_BASES {
            for signs in 0u8..8 {
                let mut candidate = [[0.0f32; 3]; 3];
                for (i, &column) in perm.iter().enumerate() {
                    candidate[i][column] = if signs & (1 << i) == 0 { 1.0 } else { -1.0 };
                }
                if determinant(candidate) != 1.0 {
                    continue;
                }
                let inner: f32 = (0..3)
                    .flat_map(|i| (0..3).map(move |j| (i, j)))
                    .map(|(i, j)| candidate[i][j] * r[i][j])
                    .sum();
                // Strict `>` keeps the first-enumerated candidate on an
                // exact tie (measure-zero for real drags, reachable from
                // preset states), giving a deterministic result.
                let replace = match &best {
                    Some((_, best_inner)) => inner > *best_inner,
                    None => true,
                };
                if replace {
                    best = Some((candidate, inner));
                }
            }
        }
        self.settings.rotation = best
            .expect("4 of the 8 sign patterns give determinant +1 for every permutation")
            .0;
    }

    pub fn apply_preset(&mut self, preset: OrientationPreset) {
        let rotation = match preset {
            // +90 degrees about X under this module's Rodrigues convention
            // (rotation_about([1,0,0], angle)): maps +y -> +z, +z -> -y (the
            // box's "up" axis), converting Z-up sources to Y-up.
            OrientationPreset::ZUpToYUp => [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]],
            OrientationPreset::FlipX => [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]],
            OrientationPreset::FlipY => [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]],
            OrientationPreset::FlipZ => [[-1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]],
        };
        self.settings.rotation = multiply(rotation, self.settings.rotation);
    }

    pub fn show(&mut self, context: &egui::Context) -> ImportDialogOutcome {
        self.ensure_converted();
        let mut outcome = ImportDialogOutcome::KeepOpen;
        egui::Modal::new("import-3d-model-modal".into()).show(context, |ui| {
            ui.heading(format!("Import 3D Model — {}", self.file_label));
            ui.horizontal(|ui| {
                let mesh_rect = allocate_viewport(ui, "import-mesh-viewport");
                self.handle_viewport_input(ui, mesh_rect, true);
                self.draw_mesh_viewport(ui, mesh_rect);
                let converted_rect = allocate_viewport(ui, "import-converted-viewport");
                self.handle_viewport_input(ui, converted_rect, false);
                self.draw_converted_viewport(ui, converted_rect);
                #[cfg(test)]
                {
                    self.mesh_viewport_rect = mesh_rect;
                    self.converted_viewport_rect = converted_rect;
                }
            });
            ui.separator();
            self.show_settings(ui);
            ui.separator();
            ui.horizontal(|ui| {
                let cancel = ui.button("Cancel");
                #[cfg(test)]
                {
                    self.cancel_button_rect = cancel.rect;
                }
                if cancel.clicked() {
                    outcome = ImportDialogOutcome::Cancel;
                }
                let importable = self.converted.is_ok();
                let import = ui.add_enabled(importable, egui::Button::new("Import"));
                #[cfg(test)]
                {
                    self.import_button_rect = import.rect;
                }
                if import.clicked()
                    && let Some(model) = self.converted_model()
                {
                    outcome = ImportDialogOutcome::Import(model);
                }
            });
        });
        outcome
    }

    fn handle_viewport_input(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        is_mesh_viewport: bool,
    ) {
        let id = ui.id().with(("import-viewport-interact", is_mesh_viewport));
        let response = ui.interact(rect, id, egui::Sense::drag());
        if response.dragged_by(egui::PointerButton::Primary) {
            let delta = response.drag_delta();
            let ctrl = ui.input(|input| input.modifiers.ctrl);
            if ctrl && is_mesh_viewport {
                self.model_drag(delta.x, delta.y);
            } else {
                self.orbit_drag(delta.x, delta.y);
            }
        }
        if response.hovered() {
            let wheel_delta = ui.input_mut(|input| {
                let delta = input.smooth_scroll_delta.y;
                input.smooth_scroll_delta.y = 0.0;
                delta
            });
            if wheel_delta != 0.0 {
                self.zoom_milli = zoom_step(self.zoom_milli, wheel_delta);
            }
        }
    }

    fn draw_mesh_viewport(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        ui.painter()
            .rect_filled(rect, 4.0, egui::Color32::from_gray(24));
        let bounds = match &self.box_space {
            Ok((_, bounds)) => *bounds,
            Err(message) => {
                paint_error(ui, rect, message);
                return;
            }
        };
        let diagonal = box_diagonal(bounds);
        let camera = self.camera;
        let zoom_milli = self.zoom_milli;
        let fallback_scale =
            || presentation_scale(egui::vec2(diagonal, diagonal), rect.size(), zoom_milli);
        let pixels_per_model_px = match &mut self.converted {
            Ok(converted) => converted
                .preview
                .frame(&converted.document, camera)
                .ok()
                .map(|frame| {
                    let native = egui::vec2(
                        frame.framebuffer().width() as f32,
                        frame.framebuffer().height() as f32,
                    );
                    presentation_scale(native, rect.size(), zoom_milli)
                })
                .unwrap_or_else(fallback_scale),
            Err(_) => fallback_scale(),
        };

        let key = MeshRasterKey {
            camera,
            zoom_milli,
            conversions: self.conversions,
            rect_width_bits: rect.width().to_bits(),
            rect_height_bits: rect.height().to_bits(),
        };
        if self.mesh_viewport_cache.key != Some(key) {
            let (box_scene, _) = self
                .box_space
                .as_ref()
                .expect("checked Ok above; box_space did not change between the two reads");
            let basis = camera.basis_f32();
            let [right, down, forward] = basis;
            let box_center = [
                bounds.width() as f32 / 2.0,
                bounds.height() as f32 / 2.0,
                bounds.depth() as f32 / 2.0,
            ];
            let scale = pixels_per_model_px as f32;
            let half_w = rect.width() / 2.0 / scale;
            let half_h = rect.height() / 2.0 / scale;
            let origin = [
                box_center[0] - half_w * right[0] - half_h * down[0],
                box_center[1] - half_w * right[1] - half_h * down[1],
                box_center[2] - half_w * right[2] - half_h * down[2],
            ];
            let lighting = Lighting {
                direction: light_direction(
                    self.settings.light_azimuth_degrees,
                    self.settings.light_elevation_degrees,
                ),
                ambient: self.settings.ambient,
            };
            let view = mesh_import::View {
                origin,
                right,
                down,
                forward,
                scale,
                width: rect.width().round() as u32,
                height: rect.height().round() as u32,
            };
            let raster = rasterize(box_scene, &view, &lighting);
            let image = mesh_raster_image(&raster);
            if let Some(texture) = &mut self.mesh_viewport_cache.texture {
                texture.set(image, egui::TextureOptions::LINEAR);
            } else {
                self.mesh_viewport_cache.texture = Some(ui.ctx().load_texture(
                    "depthsprite-import-mesh-preview",
                    image,
                    egui::TextureOptions::LINEAR,
                ));
            }
            self.mesh_viewport_cache.key = Some(key);
        }
        if let Some(texture) = &self.mesh_viewport_cache.texture {
            ui.painter().with_clip_rect(rect).image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }

    fn draw_converted_viewport(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        ui.painter()
            .rect_filled(rect, 4.0, egui::Color32::from_gray(24));
        let camera = self.camera;
        let zoom_milli = self.zoom_milli;
        let converted = match &mut self.converted {
            Ok(converted) => converted,
            Err(message) => {
                paint_error(ui, rect, message);
                return;
            }
        };
        let frame = match converted.preview.frame(&converted.document, camera) {
            Ok(frame) => frame,
            Err(error) => {
                paint_error(ui, rect, &error.to_string());
                return;
            }
        };
        let generation = frame.generation();
        let native_size = egui::vec2(
            frame.framebuffer().width() as f32,
            frame.framebuffer().height() as f32,
        );
        let image = (self.converted_uploaded_generation != Some(generation))
            .then(|| color_image(frame.framebuffer()));
        if let Some(image) = image {
            if let Some(texture) = &mut self.converted_texture {
                texture.set(image, egui::TextureOptions::NEAREST);
            } else {
                self.converted_texture = Some(ui.ctx().load_texture(
                    "depthsprite-import-converted-preview",
                    image,
                    egui::TextureOptions::NEAREST,
                ));
            }
            self.converted_uploaded_generation = Some(generation);
        }
        let scale = presentation_scale(native_size, rect.size(), zoom_milli);
        let image_rect = egui::Rect::from_center_size(rect.center(), native_size * scale as f32);
        if let Some(texture) = &self.converted_texture {
            ui.painter().with_clip_rect(rect).image(
                texture.id(),
                image_rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }

    fn show_settings(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Orientation:");
            let snap = ui.button("Snap to 90°");
            #[cfg(test)]
            {
                self.snap_button_rect = snap.rect;
            }
            if snap.clicked() {
                self.snap_rotation();
            }
            let presets = [
                ("Z-up → Y-up", OrientationPreset::ZUpToYUp),
                ("Flip X", OrientationPreset::FlipX),
                ("Flip Y", OrientationPreset::FlipY),
                ("Flip Z", OrientationPreset::FlipZ),
            ];
            #[cfg(test)]
            let mut preset_index = 0usize;
            // The counter is read only under `cfg(test)`; `.enumerate()`
            // would leave it unused (and clippy-flagged the other way) in a
            // non-test build.
            #[allow(clippy::explicit_counter_loop)]
            for (label, preset) in presets {
                let button = ui.button(label);
                #[cfg(test)]
                {
                    self.preset_button_rects[preset_index] = button.rect;
                    preset_index += 1;
                }
                if button.clicked() {
                    self.apply_preset(preset);
                }
            }
            ui.label("Ctrl+drag the mesh to rotate the model");
        });
        ui.separator();

        ui.label("Sides:");
        for (a, b) in [
            (CanonicalView::Front, CanonicalView::Back),
            (CanonicalView::Left, CanonicalView::Right),
            (CanonicalView::Top, CanonicalView::Bottom),
        ] {
            ui.horizontal(|ui| {
                self.show_side_combo(ui, a);
                self.show_side_combo(ui, b);
            });
        }
        ui.separator();

        ui.horizontal(|ui| {
            let _slider = ui.add(
                egui::Slider::new(&mut self.settings.longest_axis_pixels, 1..=63)
                    .text("longest axis"),
            );
            #[cfg(test)]
            {
                self.bounds_slider_rect = _slider.rect;
            }
            let label = match &self.box_space {
                Ok((_, bounds)) => format!(
                    "W {} × H {} × D {}",
                    bounds.width(),
                    bounds.height(),
                    bounds.depth()
                ),
                Err(error) => error.clone(),
            };
            #[cfg(test)]
            {
                self.bounds_label_text = label.clone();
            }
            ui.label(label);
        });
        ui.separator();

        ui.horizontal(|ui| {
            let _azimuth = ui.add(
                egui::Slider::new(&mut self.settings.light_azimuth_degrees, -180.0..=180.0)
                    .text("azimuth"),
            );
            let _elevation = ui.add(
                egui::Slider::new(&mut self.settings.light_elevation_degrees, -90.0..=90.0)
                    .text("elevation"),
            );
            let _ambient =
                ui.add(egui::Slider::new(&mut self.settings.ambient, 0.0..=1.0).text("ambient"));
            #[cfg(test)]
            {
                self.light_slider_rects = [_azimuth.rect, _elevation.rect, _ambient.rect];
            }
        });
    }

    fn show_side_combo(&mut self, ui: &mut egui::Ui, view: CanonicalView) {
        let current = self.settings.side_modes.get(view);
        let legal: Vec<SideMode> = self.settings.side_modes.legal_modes(view).collect();
        ui.label(view_label(view));
        egui::ComboBox::from_id_salt(("import-side-mode", view))
            .selected_text(side_mode_label(current))
            .show_ui(ui, |ui| {
                for mode in legal {
                    if ui
                        .selectable_label(current == mode, side_mode_label(mode))
                        .clicked()
                    {
                        self.settings.side_modes.set(view, mode).expect(
                            "legal_modes and set share one predicate \
                             (SideModes::allows_from_opposite), so every mode this loop \
                             offers is accepted by construction",
                        );
                    }
                }
            });
    }
}

fn allocate_viewport(ui: &mut egui::Ui, id_source: &str) -> egui::Rect {
    ui.push_id(id_source, |ui| {
        ui.allocate_exact_size(
            egui::vec2(VIEWPORT_SIZE, VIEWPORT_SIZE),
            egui::Sense::drag(),
        )
    })
    .inner
    .0
}

fn paint_error(ui: &egui::Ui, rect: egui::Rect, message: &str) {
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        message,
        egui::FontId::monospace(12.0),
        egui::Color32::LIGHT_RED,
    );
}

fn box_diagonal(bounds: Bounds) -> f32 {
    let extents = [
        bounds.width() as f32,
        bounds.height() as f32,
        bounds.depth() as f32,
    ];
    extents
        .iter()
        .map(|extent| extent * extent)
        .sum::<f32>()
        .sqrt()
}

fn mesh_raster_image(raster: &mesh_import::Raster) -> egui::ColorImage {
    egui::ColorImage::new(
        [raster.width as usize, raster.height as usize],
        raster
            .color
            .iter()
            .map(|pixel| {
                egui::Color32::from_rgba_unmultiplied(pixel[0], pixel[1], pixel[2], pixel[3])
            })
            .collect(),
    )
}

fn side_mode_label(mode: SideMode) -> &'static str {
    match mode {
        SideMode::Capture => "Capture",
        SideMode::Off => "Off",
        SideMode::FromOpposite => "From opposite",
        SideMode::FromOppositeMirrored => "From opposite, mirrored",
    }
}

/// The 6 permutations of `{0, 1, 2}`: `SIGNED_PERMUTATION_BASES[k][i]` is the
/// column assigned to row `i`, the unsigned skeleton `snap_rotation` fills
/// in with all 8 sign patterns per permutation.
const SIGNED_PERMUTATION_BASES: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];

fn determinant(m: [[f32; 3]; 3]) -> f32 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
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
    use mesh_import::{Material, Triangle, TriangleScene, derived_bounds};

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
            state.conversions, 1,
            "unchanged settings must not reconvert"
        );

        state.settings.longest_axis_pixels = 32;
        state.ensure_converted();
        assert_eq!(state.conversions, 2);

        state.orbit_drag(10.0, 5.0);
        state.ensure_converted();
        assert_eq!(state.conversions, 2, "camera orbit never reconverts");

        state.model_drag(10.0, 0.0);
        state.ensure_converted();
        assert_eq!(state.conversions, 3, "model rotation reconverts");
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

    /// All 48 signed permutation matrices (6 permutations x 8 sign patterns),
    /// restricted to the 24 with determinant +1 (proper rotations). This is
    /// the definition of "axis-aligned orientation" independent of whatever
    /// algorithm `snap_rotation` uses to search it, so comparing against it
    /// is not duplicated production logic.
    fn signed_permutation_candidates_with_determinant_one() -> Vec<[[f32; 3]; 3]> {
        const PERMUTATIONS: [[usize; 3]; 6] = [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ];
        let mut candidates = Vec::new();
        for perm in PERMUTATIONS {
            for signs in 0u8..8 {
                let mut candidate = [[0.0f32; 3]; 3];
                for (i, &column) in perm.iter().enumerate() {
                    candidate[i][column] = if signs & (1 << i) == 0 { 1.0 } else { -1.0 };
                }
                let det = candidate[0][0]
                    * (candidate[1][1] * candidate[2][2] - candidate[1][2] * candidate[2][1])
                    - candidate[0][1]
                        * (candidate[1][0] * candidate[2][2] - candidate[1][2] * candidate[2][0])
                    + candidate[0][2]
                        * (candidate[1][0] * candidate[2][1] - candidate[1][1] * candidate[2][0]);
                if det == 1.0 {
                    candidates.push(candidate);
                }
            }
        }
        candidates
    }

    fn frobenius_inner(a: [[f32; 3]; 3], b: [[f32; 3]; 3]) -> f32 {
        (0..3)
            .flat_map(|i| (0..3).map(move |j| (i, j)))
            .map(|(i, j)| a[i][j] * b[i][j])
            .sum()
    }

    #[test]
    fn snap_picks_the_nearest_axis_aligned_orientation() {
        let candidates = signed_permutation_candidates_with_determinant_one();
        assert_eq!(
            candidates.len(),
            24,
            "6 permutations x 4 sign patterns each"
        );

        let inputs: Vec<[[f32; 3]; 3]> = [
            // Reviewer's counterexample: the greedy row-order + det-fixup
            // algorithm picks permutation (2,1,0) (inner product 1.700)
            // when permutation (1,2,0) (inner product ~2.015) is nearer.
            [
                [0.659, -0.666, -0.350],
                [0.321, 0.670, -0.669],
                [0.680, 0.329, 0.655],
            ],
            [[1.0, 0.1, 0.05], [0.0, 1.0, 0.2], [0.0, 0.0, 1.0]],
            [[0.2, 0.9, 0.1], [0.9, -0.2, 0.05], [0.05, 0.1, -0.99]],
            [[-0.5, 0.5, 0.7], [0.7, 0.7, 0.0], [-0.5, 0.5, -0.7]],
            [[0.9, 0.1, 0.1], [0.1, 0.9, -0.1], [-0.1, 0.1, 0.9]],
            [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            [
                [0.577, 0.577, 0.577],
                [0.577, -0.789, 0.211],
                [-0.577, -0.211, 0.789],
            ],
            [
                [0.408, 0.408, 0.816],
                [-0.707, 0.707, 0.0],
                [-0.577, -0.577, 0.577],
            ],
        ]
        .into_iter()
        .map(orthonormalized)
        .collect();

        for input in inputs {
            let mut state = ImportDialogState::new(quad_scene(), "quad.glb".into());
            state.settings.rotation = input;
            state.snap_rotation();
            let snapped = state.settings.rotation;

            let mut ones = 0;
            for row in snapped {
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
            let det = snapped[0][0]
                * (snapped[1][1] * snapped[2][2] - snapped[1][2] * snapped[2][1])
                - snapped[0][1] * (snapped[1][0] * snapped[2][2] - snapped[1][2] * snapped[2][0])
                + snapped[0][2] * (snapped[1][0] * snapped[2][1] - snapped[1][1] * snapped[2][0]);
            assert_eq!(det, 1.0);

            let snapped_inner = frobenius_inner(snapped, input);
            for candidate in &candidates {
                let candidate_inner = frobenius_inner(*candidate, input);
                assert!(
                    // f32 summation order differs between production and
                    // this independent test enumeration; 1e-4 is far below
                    // the smallest real gap between distinct candidates for
                    // these inputs and only absorbs rounding noise.
                    snapped_inner >= candidate_inner - 1e-4,
                    "snap {snapped:?} (inner {snapped_inner}) is not nearest to {input:?}; \
                     candidate {candidate:?} scores {candidate_inner}"
                );
            }
        }
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
        // +90 about X (this module's Rodrigues convention) maps +z to -y (box up).
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

    #[test]
    fn import_outcome_carries_the_converted_model_and_cancel_carries_nothing() {
        let mut state = ImportDialogState::new(quad_scene(), "quad.glb".into());
        state.ensure_converted();
        let model = state.converted_model().expect("conversion succeeded");
        assert_eq!(model.bounds().width(), 63);

        let mut broken = ImportDialogState::new(quad_scene(), "quad.glb".into());
        broken.settings.longest_axis_pixels = 0;
        broken.ensure_converted();
        assert!(
            broken.converted_model().is_none(),
            "no model while conversion errors"
        );
    }

    fn run_dialog_frame(
        context: &egui::Context,
        state: &mut ImportDialogState,
        events: Vec<egui::Event>,
    ) -> ImportDialogOutcome {
        run_dialog_frame_with_modifiers(context, state, events, egui::Modifiers::NONE)
    }

    /// `egui::Event::PointerButton::modifiers` only labels that one event;
    /// what `ui.input(|i| i.modifiers)` reports each frame comes from
    /// `RawInput::modifiers`, which must be set on every frame ctrl is held
    /// for (including pure-motion frames, whose events carry no modifiers
    /// field at all).
    fn run_dialog_frame_with_modifiers(
        context: &egui::Context,
        state: &mut ImportDialogState,
        events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
    ) -> ImportDialogOutcome {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 1000.0),
            )),
            events,
            modifiers,
            ..Default::default()
        };
        let mut outcome = ImportDialogOutcome::KeepOpen;
        let _ = context.run_ui(input, |_ui| {
            outcome = state.show(context);
        });
        outcome
    }

    fn pointer_button(position: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn ctrl_pointer_button(position: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::CTRL,
        }
    }

    fn click(
        context: &egui::Context,
        state: &mut ImportDialogState,
        position: egui::Pos2,
    ) -> ImportDialogOutcome {
        run_dialog_frame(
            context,
            state,
            vec![
                egui::Event::PointerMoved(position),
                pointer_button(position, true),
                pointer_button(position, false),
            ],
        )
    }

    /// `egui::Modal` repositions itself once after the first frame it is
    /// shown (its placement settles as its content's measured size becomes
    /// final), so a rect captured on the very first frame can be stale.
    /// Rects are stable from the second frame on; tests read rects only
    /// after settling.
    fn settle(context: &egui::Context, state: &mut ImportDialogState) {
        run_dialog_frame(context, state, Vec::new());
        run_dialog_frame(context, state, Vec::new());
    }

    #[test]
    fn cancel_button_click_returns_cancel_outcome() {
        let context = egui::Context::default();
        let mut state = ImportDialogState::new(quad_scene(), "quad.glb".into());
        settle(&context, &mut state);
        assert!(state.cancel_button_rect.is_positive());

        let center = state.cancel_button_rect.center();
        let outcome = click(&context, &mut state, center);

        assert!(matches!(outcome, ImportDialogOutcome::Cancel));
    }

    #[test]
    fn import_button_is_disabled_while_broken_and_returns_the_model_once_fixed() {
        let context = egui::Context::default();
        let mut state = ImportDialogState::new(quad_scene(), "quad.glb".into());
        // Turning off every side makes `convert` fail with `NoCaptureSides`
        // while leaving `longest_axis_pixels` inside its slider's clamped
        // range, so the broken state survives re-rendering.
        let default_side_modes = state.settings.side_modes;
        for view in [
            CanonicalView::Front,
            CanonicalView::Back,
            CanonicalView::Left,
            CanonicalView::Right,
            CanonicalView::Top,
            CanonicalView::Bottom,
        ] {
            state.settings.side_modes.set(view, SideMode::Off).unwrap();
        }
        settle(&context, &mut state);
        assert!(
            state.converted.is_err(),
            "no captured sides must fail conversion"
        );

        let center = state.import_button_rect.center();
        let broken_outcome = click(&context, &mut state, center);
        assert!(
            matches!(broken_outcome, ImportDialogOutcome::KeepOpen),
            "a disabled Import button must not be clickable, got {broken_outcome:?}"
        );

        state.settings.side_modes = default_side_modes;
        settle(&context, &mut state);

        let center = state.import_button_rect.center();
        let outcome = click(&context, &mut state, center);
        match outcome {
            ImportDialogOutcome::Import(model) => assert_eq!(model.bounds().width(), 63),
            _ => panic!("expected an Import outcome once conversion succeeds"),
        }
    }

    #[test]
    fn plain_drag_orbits_the_shared_camera_from_the_mesh_viewport() {
        let context = egui::Context::default();
        let mut state = ImportDialogState::new(quad_scene(), "quad.glb".into());
        settle(&context, &mut state);
        let default_camera = state.camera;
        let default_rotation = state.settings.rotation;
        let center = state.mesh_viewport_rect.center();

        run_dialog_frame(
            &context,
            &mut state,
            vec![
                egui::Event::PointerMoved(center),
                pointer_button(center, true),
            ],
        );
        run_dialog_frame(
            &context,
            &mut state,
            vec![egui::Event::PointerMoved(center + egui::vec2(16.0, 7.0))],
        );

        assert_ne!(
            state.camera, default_camera,
            "plain drag in the mesh viewport must orbit the shared camera"
        );
        assert_eq!(
            state.settings.rotation, default_rotation,
            "plain drag must not rotate the model"
        );
    }

    #[test]
    fn plain_drag_orbits_the_shared_camera_from_the_converted_viewport() {
        let context = egui::Context::default();
        let mut state = ImportDialogState::new(quad_scene(), "quad.glb".into());
        settle(&context, &mut state);
        let default_camera = state.camera;
        let center = state.converted_viewport_rect.center();

        run_dialog_frame(
            &context,
            &mut state,
            vec![
                egui::Event::PointerMoved(center),
                pointer_button(center, true),
            ],
        );
        run_dialog_frame(
            &context,
            &mut state,
            vec![egui::Event::PointerMoved(center + egui::vec2(9.0, -4.0))],
        );

        assert_ne!(
            state.camera, default_camera,
            "plain drag in the converted viewport must also orbit the shared camera"
        );
    }

    #[test]
    fn ctrl_drag_in_the_mesh_viewport_rotates_the_model_not_the_camera() {
        let context = egui::Context::default();
        let mut state = ImportDialogState::new(quad_scene(), "quad.glb".into());
        settle(&context, &mut state);
        let default_camera = state.camera;
        let default_rotation = state.settings.rotation;
        let center = state.mesh_viewport_rect.center();

        run_dialog_frame_with_modifiers(
            &context,
            &mut state,
            vec![
                egui::Event::PointerMoved(center),
                ctrl_pointer_button(center, true),
            ],
            egui::Modifiers::CTRL,
        );
        run_dialog_frame_with_modifiers(
            &context,
            &mut state,
            vec![egui::Event::PointerMoved(center + egui::vec2(20.0, 0.0))],
            egui::Modifiers::CTRL,
        );

        assert_eq!(
            state.camera, default_camera,
            "ctrl+drag in the mesh viewport must not move the camera"
        );
        assert_ne!(
            state.settings.rotation, default_rotation,
            "ctrl+drag in the mesh viewport must rotate the model"
        );
    }

    #[test]
    fn ctrl_drag_in_the_converted_viewport_still_orbits_the_camera() {
        let context = egui::Context::default();
        let mut state = ImportDialogState::new(quad_scene(), "quad.glb".into());
        settle(&context, &mut state);
        let default_camera = state.camera;
        let default_rotation = state.settings.rotation;
        let center = state.converted_viewport_rect.center();

        run_dialog_frame_with_modifiers(
            &context,
            &mut state,
            vec![
                egui::Event::PointerMoved(center),
                ctrl_pointer_button(center, true),
            ],
            egui::Modifiers::CTRL,
        );
        run_dialog_frame_with_modifiers(
            &context,
            &mut state,
            vec![egui::Event::PointerMoved(center + egui::vec2(20.0, 0.0))],
            egui::Modifiers::CTRL,
        );

        assert_ne!(
            state.camera, default_camera,
            "ctrl+drag outside the mesh viewport still orbits the camera"
        );
        assert_eq!(
            state.settings.rotation, default_rotation,
            "ctrl+drag outside the mesh viewport must not rotate the model"
        );
    }

    #[test]
    fn snap_button_click_lands_on_the_nearest_axis_aligned_orientation() {
        let context = egui::Context::default();
        let mut state = ImportDialogState::new(quad_scene(), "quad.glb".into());
        state.model_drag(40.0, 25.0); // ~10 and ~6 degrees: near identity
        settle(&context, &mut state);

        let center = state.snap_button_rect.center();
        click(&context, &mut state, center);

        assert_eq!(
            state.settings.rotation,
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
        );
    }

    #[test]
    fn orientation_preset_buttons_apply_the_matching_preset() {
        let context = egui::Context::default();
        for (index, preset) in [
            OrientationPreset::ZUpToYUp,
            OrientationPreset::FlipX,
            OrientationPreset::FlipY,
            OrientationPreset::FlipZ,
        ]
        .into_iter()
        .enumerate()
        {
            let mut via_button = ImportDialogState::new(quad_scene(), "quad.glb".into());
            settle(&context, &mut via_button);
            let center = via_button.preset_button_rects[index].center();
            click(&context, &mut via_button, center);

            let mut via_method = ImportDialogState::new(quad_scene(), "quad.glb".into());
            via_method.apply_preset(preset);

            assert_eq!(
                via_button.settings.rotation, via_method.settings.rotation,
                "preset button {index} must apply {preset:?}"
            );
        }
    }

    #[test]
    fn bounds_slider_updates_longest_axis_pixels_and_label_reflects_derived_bounds() {
        let context = egui::Context::default();
        let mut state = ImportDialogState::new(quad_scene(), "quad.glb".into());
        settle(&context, &mut state);
        let slider_rect = state.bounds_slider_rect;
        assert!(slider_rect.is_positive());

        let target = slider_rect.left_center() + egui::vec2(slider_rect.width() * 0.1, 0.0);
        run_dialog_frame(
            &context,
            &mut state,
            vec![
                egui::Event::PointerMoved(target),
                pointer_button(target, true),
            ],
        );
        run_dialog_frame(&context, &mut state, vec![pointer_button(target, false)]);

        assert_ne!(
            state.settings.longest_axis_pixels, 63,
            "dragging near the low end of the slider must lower the setting"
        );
        let expected_label = match derived_bounds(
            &state.scene,
            state.settings.rotation,
            state.settings.longest_axis_pixels,
        ) {
            Ok(bounds) => format!(
                "W {} × H {} × D {}",
                bounds.width(),
                bounds.height(),
                bounds.depth()
            ),
            Err(error) => error.to_string(),
        };
        assert_eq!(state.bounds_label_text, expected_label);
    }

    #[test]
    fn ambient_slider_updates_the_lighting_setting() {
        let context = egui::Context::default();
        let mut state = ImportDialogState::new(quad_scene(), "quad.glb".into());
        settle(&context, &mut state);
        let ambient_rect = state.light_slider_rects[2];
        assert!(ambient_rect.is_positive());

        let target = ambient_rect.left_center() + egui::vec2(ambient_rect.width() * 0.1, 0.0);
        run_dialog_frame(
            &context,
            &mut state,
            vec![
                egui::Event::PointerMoved(target),
                pointer_button(target, true),
            ],
        );
        run_dialog_frame(&context, &mut state, vec![pointer_button(target, false)]);

        assert_ne!(state.settings.ambient, ImportSettings::default().ambient);
    }
}
