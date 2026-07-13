use num_rational::Ratio;
use relief_render::{CameraBasis, RenderRequest, TargetView};

use crate::jobs::GenerationCounter;

const PREVIEW_SIDE: u32 = 96;
const MAX_ZOOM: u32 = 4;
const RATIONAL_DENOMINATOR: i64 = 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ViewPreset {
    Front,
    Top,
    Side,
    Isometric,
}

pub(crate) struct ViewportState {
    generations: GenerationCounter,
    yaw_degrees: i32,
    pitch_degrees: i32,
    zoom: u32,
    target: TargetView,
    current_view_name: &'static str,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self {
            generations: GenerationCounter::default(),
            yaw_degrees: 45,
            pitch_degrees: 30,
            zoom: 1,
            target: TargetView::isometric_v1(),
            current_view_name: "Isometric v1",
        }
    }
}

impl ViewportState {
    pub(crate) fn generation(&self) -> u64 {
        self.generations.current()
    }

    pub(crate) fn drag(&mut self, delta_x: i32, delta_y: i32) -> Option<u64> {
        if delta_x == 0 && delta_y == 0 {
            return None;
        }
        self.yaw_degrees = (self.yaw_degrees + delta_x).rem_euclid(360);
        self.pitch_degrees = (self.pitch_degrees + delta_y).clamp(-89, 89);
        self.target = free_target(self.yaw_degrees, self.pitch_degrees);
        self.current_view_name = "Free orbit";
        Some(self.generations.advance())
    }

    pub(crate) fn wheel(&mut self, steps: i32) -> Option<u64> {
        let updated = (i64::from(self.zoom) + i64::from(steps)).clamp(1, i64::from(MAX_ZOOM));
        let updated = updated as u32;
        if updated == self.zoom {
            return None;
        }
        self.zoom = updated;
        Some(self.generations.advance())
    }

    pub(crate) fn select_preset(&mut self, preset: ViewPreset) -> u64 {
        let (target, name) = match preset {
            ViewPreset::Front => (TargetView::front_v1(), "Front"),
            ViewPreset::Top => (TargetView::top_v1(), "Top"),
            ViewPreset::Side => (TargetView::right_v1(), "Side"),
            ViewPreset::Isometric => (TargetView::isometric_v1(), "Isometric v1"),
        };
        self.target = target;
        self.current_view_name = name;
        self.generations.advance()
    }

    pub(crate) fn document_changed(&mut self) -> u64 {
        self.generations.advance()
    }

    pub(crate) fn zoom(&self) -> u32 {
        self.zoom
    }

    #[cfg(test)]
    pub(crate) fn target(&self) -> &TargetView {
        &self.target
    }

    pub(crate) fn current_view_name(&self) -> &str {
        self.current_view_name
    }

    pub(crate) fn request(&self) -> RenderRequest {
        let side = PREVIEW_SIDE * self.zoom;
        RenderRequest::new(side, side, self.target.clone())
    }
}

fn free_target(yaw_degrees: i32, pitch_degrees: i32) -> TargetView {
    let yaw = f64::from(yaw_degrees).to_radians();
    let pitch = f64::from(pitch_degrees).to_radians();
    let (sin_yaw, cos_yaw) = yaw.sin_cos();
    let (sin_pitch, cos_pitch) = pitch.sin_cos();
    TargetView::from_camera(CameraBasis::new(
        [ratio(cos_yaw), ratio(0.0), ratio(sin_yaw)],
        [
            ratio(sin_yaw * sin_pitch),
            ratio(cos_pitch),
            ratio(-cos_yaw * sin_pitch),
        ],
        [
            ratio(-sin_yaw * cos_pitch),
            ratio(sin_pitch),
            ratio(cos_yaw * cos_pitch),
        ],
    ))
}

fn ratio(value: f64) -> Ratio<i64> {
    Ratio::new(
        (value * RATIONAL_DENOMINATOR as f64).round() as i64,
        RATIONAL_DENOMINATOR,
    )
}

pub(crate) struct ViewportInput {
    pub(crate) drag: Option<(i32, i32)>,
    pub(crate) wheel_steps: i32,
}

pub(crate) fn show(
    ui: &mut eframe::egui::Ui,
    texture: Option<&eframe::egui::TextureHandle>,
) -> ViewportInput {
    use eframe::egui::{Color32, Rect, Sense, Vec2, pos2};

    let side = ui.available_width().min(ui.available_height()).max(96.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::drag());
    let tile = 16.0;
    let rows = (rect.height() / tile).ceil() as usize;
    let columns = (rect.width() / tile).ceil() as usize;
    for row in 0..rows {
        for column in 0..columns {
            let color = if (row + column) % 2 == 0 {
                Color32::from_gray(58)
            } else {
                Color32::from_gray(88)
            };
            let tile_rect = Rect::from_min_max(
                pos2(
                    rect.left() + column as f32 * tile,
                    rect.top() + row as f32 * tile,
                ),
                pos2(
                    (rect.left() + (column + 1) as f32 * tile).min(rect.right()),
                    (rect.top() + (row + 1) as f32 * tile).min(rect.bottom()),
                ),
            );
            ui.painter().rect_filled(tile_rect, 0.0, color);
        }
    }
    if let Some(texture) = texture {
        ui.painter().image(
            texture.id(),
            rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }

    let delta = response.drag_delta();
    let drag = response.dragged().then(|| {
        let x = delta.x.round() as i32;
        let y = -delta.y.round() as i32;
        (x, y)
    });
    let scroll = if response.hovered() {
        ui.input(|input| input.smooth_scroll_delta.y)
    } else {
        0.0
    };
    let wheel_steps = match scroll.total_cmp(&0.0) {
        std::cmp::Ordering::Greater => 1,
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
    };
    ViewportInput { drag, wheel_steps }
}

#[cfg(test)]
mod tests {
    use relief_render::TargetView;

    use super::{ViewPreset, ViewportState};

    #[test]
    fn every_effective_camera_mutation_advances_generation() {
        let mut viewport = ViewportState::default();
        assert_eq!(viewport.generation(), 0);

        assert_eq!(viewport.drag(12, -4), Some(1));
        assert_eq!(viewport.wheel(1), Some(2));
        assert_eq!(viewport.select_preset(ViewPreset::Top), 3);
        assert_eq!(viewport.select_preset(ViewPreset::Side), 4);
        assert_eq!(viewport.select_preset(ViewPreset::Front), 5);
        assert_eq!(viewport.drag(0, 0), None);
        assert_eq!(viewport.wheel(0), None);
        assert_eq!(viewport.generation(), 5);
    }

    #[test]
    fn fixed_isometric_uses_the_authoritative_v1_preset() {
        let mut viewport = ViewportState::default();
        let generation = viewport.select_preset(ViewPreset::Isometric);

        assert_eq!(generation, 1);
        assert_eq!(viewport.target(), &TargetView::isometric_v1());
        assert_eq!(viewport.current_view_name(), "Isometric v1");
    }

    #[test]
    fn wheel_zoom_is_integer_and_bounded() {
        let mut viewport = ViewportState::default();
        assert_eq!(viewport.zoom(), 1);
        assert_eq!(viewport.wheel(-1), None);
        assert_eq!(viewport.zoom(), 1);
        assert_eq!(viewport.wheel(3), Some(1));
        assert_eq!(viewport.zoom(), 4);
        assert_eq!(viewport.wheel(1), None);
    }

    #[test]
    fn document_change_invalidates_in_flight_render() {
        let mut viewport = ViewportState::default();
        assert_eq!(viewport.document_changed(), 1);
        assert_eq!(viewport.generation(), 1);
    }
}
