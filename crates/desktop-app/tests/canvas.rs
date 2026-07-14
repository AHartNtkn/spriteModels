use desktop_app::canvas::{
    CanvasKind, CanvasTransform, PixelCoord, StrokeController, depth_display, display_pixels,
    interpolated_pixels,
};
use editor_core::{ActiveLayer, EditorDocument, Tool};
use eframe::egui::{self, Color32, Pos2, Rect, pos2};
use relief_core::{Bounds, CanonicalView};

const VIEW: CanonicalView = CanonicalView::Front;

fn document(width: u32, height: u32) -> EditorDocument {
    EditorDocument::new(Bounds::new(width, height, 1).unwrap(), VIEW)
}

fn pixels(document: &EditorDocument) -> Vec<[u8; 4]> {
    document.source(VIEW).unwrap().rgba().to_vec()
}

#[test]
fn depth_display_maps_empty_near_and_far_relief_exactly() {
    assert_eq!(depth_display([19, 29, 39, 0]), Color32::MAGENTA);
    assert_eq!(depth_display([19, 29, 39, 255]), Color32::BLACK);
    assert_eq!(depth_display([19, 29, 39, 1]), Color32::WHITE);
}

#[test]
fn color_projection_shows_stored_rgb_even_when_depth_is_empty() {
    let mut document = document(1, 1);
    document.set_current_rgb([12, 34, 56]);
    document.begin_stroke().unwrap();
    document.pencil_pixel(VIEW, 0, 0).unwrap();
    document.finish_stroke().unwrap();

    assert_eq!(
        display_pixels(&document, VIEW, CanvasKind::Color),
        [Color32::from_rgb(12, 34, 56)]
    );
    assert_eq!(
        display_pixels(&document, VIEW, CanvasKind::Depth),
        [Color32::MAGENTA]
    );
}

#[test]
fn paired_canvases_use_one_transform_for_the_same_chart_coordinate() {
    let mut transform = CanvasTransform::default();
    transform.set_zoom(1.5);
    transform.pan_by(egui::vec2(6.0, -3.0));
    let color_rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(96.0, 64.0));
    let depth_rect = color_rect.translate(egui::vec2(0.0, 90.0));
    let local_pointer = Pos2::new(58.0, 36.0);

    let color = transform.pointer_to_pixel(color_rect, (4, 2), local_pointer);
    let depth =
        transform.pointer_to_pixel(depth_rect, (4, 2), local_pointer + egui::vec2(0.0, 90.0));

    assert_eq!(color, depth);
    assert!(color.is_some());
}

#[test]
fn interpolation_visits_every_crossed_chart_pixel() {
    assert_eq!(
        interpolated_pixels(PixelCoord::new(0, 0), PixelCoord::new(5, 2)),
        [
            PixelCoord::new(0, 0),
            PixelCoord::new(1, 0),
            PixelCoord::new(2, 1),
            PixelCoord::new(3, 1),
            PixelCoord::new(4, 2),
            PixelCoord::new(5, 2),
        ]
    );
}

#[test]
fn one_pointer_drag_paints_all_crossed_pixels_as_one_document_command() {
    let mut document = document(6, 1);
    document.set_tool(Tool::Pencil);
    document.set_current_rgb([70, 80, 90]);
    let mut stroke = StrokeController::default();

    stroke
        .pointer_down(
            &mut document,
            VIEW,
            CanvasKind::Color,
            PixelCoord::new(0, 0),
        )
        .unwrap();
    stroke
        .pointer_dragged(&mut document, PixelCoord::new(5, 0))
        .unwrap();
    assert!(stroke.pointer_released(&mut document).unwrap());

    assert_eq!(document.active_layer(), ActiveLayer::Color);
    assert_eq!(pixels(&document), [[70, 80, 90, 0]; 6]);
    assert!(document.undo());
    assert_eq!(pixels(&document), [[0, 0, 0, 0]; 6]);
    assert!(
        !document.undo(),
        "the drag must create exactly one undo entry"
    );
}
