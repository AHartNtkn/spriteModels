use editor_core::{ActiveLayer, EditorDocument, EditorError, Tool};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Vec2};
use relief_core::CanonicalView;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CanvasKind {
    Color,
    Depth,
}

impl CanvasKind {
    const fn layer(self) -> ActiveLayer {
        match self {
            Self::Color => ActiveLayer::Color,
            Self::Depth => ActiveLayer::Depth,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PixelCoord {
    pub x: u32,
    pub y: u32,
}

impl PixelCoord {
    pub const fn new(x: u32, y: u32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasTransform {
    zoom: f32,
    pan: Vec2,
}

impl Default for CanvasTransform {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: Vec2::ZERO,
        }
    }
}

impl CanvasTransform {
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn pan(&self) -> Vec2 {
        self.pan
    }

    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom.clamp(0.25, 32.0);
    }

    pub fn pan_by(&mut self, delta: Vec2) {
        self.pan += delta;
    }

    pub fn chart_rect(&self, viewport: Rect, dimensions: (u32, u32)) -> Rect {
        let (width, height) = dimensions;
        if width == 0 || height == 0 {
            return Rect::from_center_size(viewport.center() + self.pan, Vec2::ZERO);
        }
        let fit = (viewport.width() / width as f32).min(viewport.height() / height as f32);
        let pixel_extent = fit * self.zoom;
        Rect::from_center_size(
            viewport.center() + self.pan,
            egui::vec2(width as f32 * pixel_extent, height as f32 * pixel_extent),
        )
    }

    pub fn pointer_to_pixel(
        &self,
        viewport: Rect,
        dimensions: (u32, u32),
        pointer: Pos2,
    ) -> Option<PixelCoord> {
        let chart = self.chart_rect(viewport, dimensions);
        if !chart.contains(pointer) || dimensions.0 == 0 || dimensions.1 == 0 {
            return None;
        }
        let pixel_width = chart.width() / dimensions.0 as f32;
        let pixel_height = chart.height() / dimensions.1 as f32;
        let x = ((pointer.x - chart.left()) / pixel_width).floor() as u32;
        let y = ((pointer.y - chart.top()) / pixel_height).floor() as u32;
        (x < dimensions.0 && y < dimensions.1).then_some(PixelCoord::new(x, y))
    }

    pub fn pixel_rect(
        &self,
        viewport: Rect,
        dimensions: (u32, u32),
        pixel: PixelCoord,
    ) -> Option<Rect> {
        if pixel.x >= dimensions.0 || pixel.y >= dimensions.1 {
            return None;
        }
        let chart = self.chart_rect(viewport, dimensions);
        let extent = egui::vec2(
            chart.width() / dimensions.0 as f32,
            chart.height() / dimensions.1 as f32,
        );
        let min = chart.min + egui::vec2(pixel.x as f32 * extent.x, pixel.y as f32 * extent.y);
        Some(Rect::from_min_size(min, extent))
    }
}

pub fn color_display(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

pub fn depth_display(pixel: [u8; 4]) -> Color32 {
    depth_display_alpha(pixel[3])
}

fn depth_display_alpha(alpha: u8) -> Color32 {
    if alpha == 0 {
        Color32::MAGENTA
    } else {
        let relief = 255 - alpha;
        let gray = ((u16::from(relief) * 255 + 127) / 254) as u8;
        Color32::from_gray(gray)
    }
}

pub fn display_pixels(
    document: &EditorDocument,
    view: CanonicalView,
    kind: CanvasKind,
) -> Vec<Color32> {
    let Some(source) = document.source(view) else {
        return Vec::new();
    };
    let (width, height) = source.dimensions();
    (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .map(|(x, y)| {
            let [red, green, blue, alpha] = source
                .rgba_at(x, y)
                .expect("coordinates come from source dimensions");
            match kind {
                CanvasKind::Color => color_display([red, green, blue]),
                CanvasKind::Depth => depth_display_alpha(alpha),
            }
        })
        .collect()
}

pub fn interpolated_pixels(from: PixelCoord, to: PixelCoord) -> Vec<PixelCoord> {
    let delta_x = i64::from(to.x) - i64::from(from.x);
    let delta_y = i64::from(to.y) - i64::from(from.y);
    let steps = delta_x.unsigned_abs().max(delta_y.unsigned_abs());
    if steps == 0 {
        return vec![from];
    }
    (0..=steps)
        .map(|step| {
            let progress = step as f64 / steps as f64;
            PixelCoord::new(
                (from.x as f64 + delta_x as f64 * progress).round() as u32,
                (from.y as f64 + delta_y as f64 * progress).round() as u32,
            )
        })
        .collect()
}

#[derive(Debug, Default)]
pub struct StrokeController {
    active: bool,
    view: Option<CanonicalView>,
    last_pixel: Option<PixelCoord>,
}

impl StrokeController {
    pub fn pointer_down(
        &mut self,
        document: &mut EditorDocument,
        view: CanonicalView,
        kind: CanvasKind,
        pixel: PixelCoord,
    ) -> Result<(), EditorError> {
        document.set_active_layer(kind.layer());
        if !document.tool().is_available_on(document.active_layer()) {
            return Ok(());
        }
        match document.tool() {
            Tool::Pencil | Tool::Eraser => {
                document.begin_stroke()?;
                self.active = true;
                self.view = Some(view);
                self.last_pixel = Some(pixel);
                if let Err(error) = self.apply(document, pixel) {
                    self.cancel(document);
                    return Err(error);
                }
            }
            Tool::Fill => {
                document.fill(view, pixel.x, pixel.y)?;
            }
            Tool::Eyedropper => {
                document.eyedrop(view, pixel.x, pixel.y)?;
            }
        }
        Ok(())
    }

    pub fn pointer_dragged(
        &mut self,
        document: &mut EditorDocument,
        pixel: PixelCoord,
    ) -> Result<(), EditorError> {
        if !self.active {
            return Ok(());
        }
        let previous = self
            .last_pixel
            .expect("active strokes have a previous pixel");
        for crossed in interpolated_pixels(previous, pixel).into_iter().skip(1) {
            if let Err(error) = self.apply(document, crossed) {
                self.cancel(document);
                return Err(error);
            }
        }
        self.last_pixel = Some(pixel);
        Ok(())
    }

    pub fn pointer_released(&mut self, document: &mut EditorDocument) -> Result<bool, EditorError> {
        if !self.active {
            return Ok(false);
        }
        self.active = false;
        self.view = None;
        self.last_pixel = None;
        document.finish_stroke()
    }

    pub fn cancel(&mut self, document: &mut EditorDocument) {
        if self.active {
            document.cancel_stroke();
        }
        self.active = false;
        self.view = None;
        self.last_pixel = None;
    }

    fn apply(&self, document: &mut EditorDocument, pixel: PixelCoord) -> Result<(), EditorError> {
        let view = self.view.expect("active strokes have a source view");
        match document.tool() {
            Tool::Pencil => {
                document.pencil_pixel(view, pixel.x, pixel.y)?;
            }
            Tool::Eraser => {
                document.erase_pixel(view, pixel.x, pixel.y)?;
            }
            Tool::Fill | Tool::Eyedropper => {}
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct CanvasPairState {
    pub transform: CanvasTransform,
    pub hover: Option<PixelCoord>,
    stroke: StrokeController,
    interaction_error: Option<String>,
}

pub struct CanvasPairOutput {
    #[cfg(test)]
    pub(crate) observation: CanvasPairObservation,
}

#[cfg(test)]
pub(crate) struct CanvasPairObservation {
    pub color: Rect,
    pub depth: Rect,
}

impl CanvasPairState {
    pub fn show_pair(
        &mut self,
        ui: &mut egui::Ui,
        document: &mut EditorDocument,
        view: CanonicalView,
        color_rect: Rect,
        depth_rect: Rect,
    ) -> CanvasPairOutput {
        let color_response = ui.allocate_rect(color_rect, Sense::click_and_drag());
        let depth_response = ui.allocate_rect(depth_rect, Sense::click_and_drag());
        let dimensions = document.source(view).map(|source| source.dimensions());

        if let Some(dimensions) = dimensions {
            self.apply_pair_input(
                ui,
                document,
                view,
                dimensions,
                &color_response,
                &depth_response,
            );
        } else {
            self.hover = None;
        }

        let transform = self.transform;
        let hover = self.hover;
        paint_canvas(
            ui,
            document,
            view,
            CanvasKind::Color,
            color_rect,
            transform,
            hover,
        );
        paint_canvas(
            ui,
            document,
            view,
            CanvasKind::Depth,
            depth_rect,
            transform,
            hover,
        );

        if let Some(error) = &self.interaction_error {
            for rect in [color_rect, depth_rect] {
                ui.painter().text(
                    rect.left_top() + egui::vec2(3.0, 3.0),
                    egui::Align2::LEFT_TOP,
                    error,
                    egui::FontId::monospace(9.0),
                    Color32::LIGHT_RED,
                );
            }
        }
        CanvasPairOutput {
            #[cfg(test)]
            observation: CanvasPairObservation {
                color: color_response.rect,
                depth: depth_response.rect,
            },
        }
    }

    fn apply_pair_input(
        &mut self,
        ui: &egui::Ui,
        document: &mut EditorDocument,
        view: CanonicalView,
        dimensions: (u32, u32),
        color_response: &egui::Response,
        depth_response: &egui::Response,
    ) {
        if color_response.dragged_by(egui::PointerButton::Middle)
            || depth_response.dragged_by(egui::PointerButton::Middle)
        {
            self.transform
                .pan_by(ui.input(|input| input.pointer.delta()));
        }
        if color_response.hovered() || depth_response.hovered() {
            let scroll = ui.input(|input| input.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.transform
                    .set_zoom(self.transform.zoom() * (scroll * 0.002).exp());
            }
        }

        let hovered_canvas = ui
            .input(|input| input.pointer.hover_pos())
            .and_then(|position| {
                if color_response.hovered() {
                    Some((CanvasKind::Color, color_response.rect, position))
                } else if depth_response.hovered() {
                    Some((CanvasKind::Depth, depth_response.rect, position))
                } else {
                    None
                }
            });
        self.hover = hovered_canvas.and_then(|(_, rect, position)| {
            self.transform.pointer_to_pixel(rect, dimensions, position)
        });

        if ui.input(|input| input.pointer.primary_pressed()) {
            if let (Some((kind, _, _)), Some(pixel)) = (hovered_canvas, self.hover) {
                let result = document
                    .select_source(view)
                    .and_then(|()| self.stroke.pointer_down(document, view, kind, pixel));
                self.capture(result);
            }
        } else if ui.input(|input| input.pointer.primary_down())
            && let Some(pixel) = self.hover
        {
            let result = self.stroke.pointer_dragged(document, pixel);
            self.capture(result);
        }
        if ui.input(|input| input.pointer.primary_released()) {
            let result = self.stroke.pointer_released(document);
            self.capture(result);
        }
    }

    fn capture<T>(&mut self, result: Result<T, EditorError>) {
        match result {
            Ok(_) => self.interaction_error = None,
            Err(error) => self.interaction_error = Some(error.to_string()),
        }
    }
}

fn paint_canvas(
    ui: &egui::Ui,
    document: &EditorDocument,
    view: CanonicalView,
    kind: CanvasKind,
    rect: Rect,
    transform: CanvasTransform,
    hover: Option<PixelCoord>,
) {
    ui.painter().rect_filled(rect, 0.0, Color32::from_gray(24));
    let Some(source) = document.source(view) else {
        return;
    };
    let dimensions = source.dimensions();
    for y in 0..dimensions.1 {
        for x in 0..dimensions.0 {
            let [red, green, blue, alpha] = source
                .rgba_at(x, y)
                .expect("coordinates come from source dimensions");
            let color = match kind {
                CanvasKind::Color => color_display([red, green, blue]),
                CanvasKind::Depth => depth_display_alpha(alpha),
            };
            if let Some(pixel_rect) = transform.pixel_rect(rect, dimensions, PixelCoord::new(x, y))
            {
                ui.painter().rect_filled(pixel_rect, 0.0, color);
            }
        }
    }

    if let Some(pixel) = hover
        && let Some(pixel_rect) = transform.pixel_rect(rect, dimensions, pixel)
    {
        ui.painter().rect_stroke(
            pixel_rect,
            0.0,
            egui::Stroke::new(1.0, Color32::YELLOW),
            egui::StrokeKind::Inside,
        );
    }
}
