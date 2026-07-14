use desktop_app::canvas::{
    CanvasKind, CanvasTransform, PixelCoord, StrokeController, depth_display, display_pixels,
    interpolated_pixels,
};
use editor_core::{ActiveLayer, EditorDocument, Tool};
use eframe::egui::{self, Color32, Pos2, Rect, pos2};
use relief_core::{AuthoredModel, Bounds, CanonicalView, Chart, EMPTY_RGBA};

const VIEW: CanonicalView = CanonicalView::Front;

fn document(width: u32, height: u32) -> EditorDocument {
    EditorDocument::new(Bounds::new(width, height, 1).unwrap(), VIEW)
}

fn pixels(document: &EditorDocument) -> Vec<[u8; 4]> {
    document.source(VIEW).unwrap().rgba().to_vec()
}

const COLOR_RECT: Rect = Rect::from_min_max(pos2(20.0, 20.0), pos2(120.0, 70.0));
const DEPTH_RECT: Rect = Rect::from_min_max(pos2(20.0, 90.0), pos2(120.0, 140.0));

fn raw_input(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(Rect::from_min_max(pos2(0.0, 0.0), pos2(180.0, 180.0))),
        events,
        ..Default::default()
    }
}

fn run_pair_frame(
    context: &egui::Context,
    state: &mut desktop_app::canvas::CanvasPairState,
    document: &mut EditorDocument,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    run_view_pair_frame(context, state, document, VIEW, events)
}

fn run_view_pair_frame(
    context: &egui::Context,
    state: &mut desktop_app::canvas::CanvasPairState,
    document: &mut EditorDocument,
    view: CanonicalView,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    context.run_ui(raw_input(events), |ui| {
        state.show_pair(ui, document, view, COLOR_RECT, DEPTH_RECT);
    })
}

fn moved(position: Pos2) -> egui::Event {
    egui::Event::PointerMoved(position)
}

fn button(position: Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: position,
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn click_pair(
    context: &egui::Context,
    state: &mut desktop_app::canvas::CanvasPairState,
    document: &mut EditorDocument,
    position: Pos2,
) {
    run_pair_frame(context, state, document, Vec::new());
    run_pair_frame(
        context,
        state,
        document,
        vec![
            moved(position),
            button(position, egui::PointerButton::Primary, true),
        ],
    );
    run_pair_frame(
        context,
        state,
        document,
        vec![button(position, egui::PointerButton::Primary, false)],
    );
}

fn filled_rect(output: &egui::FullOutput, color: Color32) -> Rect {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape) if shape.fill == color => Some(shape.rect),
            _ => None,
        })
        .unwrap_or_else(|| panic!("frame did not paint {color:?}"))
}

fn highlight_rects(output: &egui::FullOutput) -> Vec<Rect> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape) if shape.stroke.color == Color32::YELLOW => Some(shape.rect),
            _ => None,
        })
        .collect()
}

fn assert_same_local_rect(color: Rect, depth: Rect) {
    let color_local = color.min - COLOR_RECT.min;
    let depth_local = depth.min - DEPTH_RECT.min;
    assert!((color.width() - depth.width()).abs() < 0.001);
    assert!((color.height() - depth.height()).abs() < 0.001);
    assert!((color_local.x - depth_local.x).abs() < 0.001);
    assert!((color_local.y - depth_local.y).abs() < 0.001);
}

#[test]
fn depth_display_maps_empty_near_and_far_relief_exactly() {
    assert_eq!(depth_display([19, 29, 39, 0]), Color32::MAGENTA);
    assert_eq!(depth_display([19, 29, 39, 255]), Color32::BLACK);
    assert_eq!(depth_display([19, 29, 39, 1]), Color32::WHITE);
}

#[test]
fn depth_display_rounds_midpoint_relief_to_nearest_gray() {
    assert_eq!(depth_display([19, 29, 39, 128]), Color32::from_gray(128));
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
fn alpha_223_displays_the_same_gray_for_views_with_different_depth_limits() {
    let bounds = Bounds::new(1, 16, 8).unwrap();
    let mut front = vec![EMPTY_RGBA; 16];
    front[0] = [1, 2, 3, 223];
    let mut top = vec![EMPTY_RGBA; 8];
    top[0] = [4, 5, 6, 223];
    let document = EditorDocument::from_model(
        AuthoredModel::new(
            bounds,
            vec![
                Chart::from_rgba(CanonicalView::Front, 1, 16, front).unwrap(),
                Chart::from_rgba(CanonicalView::Top, 1, 8, top).unwrap(),
            ],
        )
        .unwrap(),
        None,
    );

    assert_ne!(
        CanonicalView::Front.maximum_inward_depth(bounds),
        CanonicalView::Top.maximum_inward_depth(bounds)
    );
    assert_eq!(
        display_pixels(&document, CanonicalView::Front, CanvasKind::Depth)[0],
        display_pixels(&document, CanonicalView::Top, CanvasKind::Depth)[0]
    );
    assert_eq!(
        display_pixels(&document, CanonicalView::Front, CanvasKind::Depth)[0],
        Color32::from_gray(32)
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
    assert_eq!(pixels(&document), [EMPTY_RGBA; 6]);
    assert!(
        !document.undo(),
        "the drag must create exactly one undo entry"
    );
}

#[test]
fn pan_and_zoom_from_either_canvas_paint_one_shared_transform_that_frame() {
    for origin in [COLOR_RECT, DEPTH_RECT] {
        let context = egui::Context::default();
        let mut state = desktop_app::canvas::CanvasPairState::default();
        let mut document = document(1, 1);
        document.set_current_rgb([12, 34, 56]);
        document.begin_stroke().unwrap();
        document.pencil_pixel(VIEW, 0, 0).unwrap();
        document.finish_stroke().unwrap();
        let start = origin.center();

        run_pair_frame(&context, &mut state, &mut document, vec![moved(start)]);
        run_pair_frame(
            &context,
            &mut state,
            &mut document,
            vec![button(start, egui::PointerButton::Middle, true)],
        );
        let drag = run_pair_frame(
            &context,
            &mut state,
            &mut document,
            vec![moved(start + egui::vec2(7.0, -4.0))],
        );
        assert_same_local_rect(
            filled_rect(&drag, Color32::from_rgb(12, 34, 56)),
            filled_rect(&drag, Color32::MAGENTA),
        );
        assert_eq!(state.transform.pan(), egui::vec2(7.0, -4.0));
        run_pair_frame(
            &context,
            &mut state,
            &mut document,
            vec![button(
                start + egui::vec2(7.0, -4.0),
                egui::PointerButton::Middle,
                false,
            )],
        );

        let zoom = run_pair_frame(
            &context,
            &mut state,
            &mut document,
            vec![
                moved(start),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, 120.0),
                    modifiers: egui::Modifiers::NONE,
                    phase: egui::TouchPhase::Move,
                },
            ],
        );
        assert_same_local_rect(
            filled_rect(&zoom, Color32::from_rgb(12, 34, 56)),
            filled_rect(&zoom, Color32::MAGENTA),
        );
        assert!(state.transform.zoom() > 1.0);
    }
}

#[test]
fn pair_hover_highlights_the_same_pixel_in_both_views_and_clears_outside() {
    let context = egui::Context::default();
    let mut state = desktop_app::canvas::CanvasPairState::default();
    let mut document = document(2, 1);
    let pointer = COLOR_RECT.left_center() + egui::vec2(75.0, 0.0);

    run_pair_frame(&context, &mut state, &mut document, Vec::new());
    let hovered = run_pair_frame(&context, &mut state, &mut document, vec![moved(pointer)]);
    let highlights = highlight_rects(&hovered);
    assert_eq!(highlights.len(), 2);
    assert_same_local_rect(highlights[0], highlights[1]);
    assert_eq!(state.hover, Some(PixelCoord::new(1, 0)));

    let cleared = run_pair_frame(
        &context,
        &mut state,
        &mut document,
        vec![moved(pos2(160.0, 160.0))],
    );
    assert!(highlight_rects(&cleared).is_empty());
    assert_eq!(state.hover, None);
}

#[test]
fn fill_and_eyedropper_dispatch_through_pair_widget_frames() {
    let context = egui::Context::default();
    let mut state = desktop_app::canvas::CanvasPairState::default();
    let mut fill = document(2, 1);
    fill.set_current_rgb([9, 8, 7]);
    fill.set_tool(Tool::Fill);
    click_pair(
        &context,
        &mut state,
        &mut fill,
        COLOR_RECT.left_center() + egui::vec2(25.0, 0.0),
    );
    assert_eq!(pixels(&fill), [[9, 8, 7, 0], [9, 8, 7, 0]]);

    let mut eyedrop = document(1, 1);
    eyedrop.set_current_rgb([71, 81, 91]);
    eyedrop.begin_stroke().unwrap();
    eyedrop.pencil_pixel(VIEW, 0, 0).unwrap();
    eyedrop.finish_stroke().unwrap();
    eyedrop.set_current_rgb([0, 0, 0]);
    eyedrop.set_tool(Tool::Eyedropper);
    click_pair(&context, &mut state, &mut eyedrop, COLOR_RECT.center());
    assert_eq!(eyedrop.current_rgb(), [71, 81, 91]);
}

#[test]
fn primary_press_selects_its_source_before_selecting_the_layer() {
    let context = egui::Context::default();
    let mut state = desktop_app::canvas::CanvasPairState::default();
    let mut document = EditorDocument::new(Bounds::new(2, 3, 4).unwrap(), CanonicalView::Front);
    document.add_source(CanonicalView::Top).unwrap();
    document.select_source(CanonicalView::Front).unwrap();
    document.set_active_layer(ActiveLayer::Depth);
    let position = COLOR_RECT.center();

    run_view_pair_frame(
        &context,
        &mut state,
        &mut document,
        CanonicalView::Top,
        Vec::new(),
    );
    run_view_pair_frame(
        &context,
        &mut state,
        &mut document,
        CanonicalView::Top,
        vec![
            moved(position),
            button(position, egui::PointerButton::Primary, true),
        ],
    );

    assert_eq!(document.selected_view(), CanonicalView::Top);
    assert_eq!(document.active_layer(), ActiveLayer::Color);
}

#[test]
fn color_eraser_stays_out_of_stroke_history_while_depth_eraser_works() {
    let context = egui::Context::default();
    let mut state = desktop_app::canvas::CanvasPairState::default();
    let mut color = document(1, 1);
    color.set_active_layer(ActiveLayer::Depth);
    color.set_tool(Tool::Eraser);
    click_pair(&context, &mut state, &mut color, COLOR_RECT.center());
    assert_eq!(color.active_layer(), ActiveLayer::Color);
    assert!(!color.stroke_active());
    assert!(!color.can_undo());

    let mut depth = document(1, 1);
    depth.set_active_layer(ActiveLayer::Depth);
    depth.set_current_depth(editor_core::DepthValue::Relief(
        editor_core::ReliefValue::new(0).unwrap(),
    ));
    depth.begin_stroke().unwrap();
    depth.pencil_pixel(VIEW, 0, 0).unwrap();
    depth.finish_stroke().unwrap();
    depth.set_tool(Tool::Eraser);
    click_pair(&context, &mut state, &mut depth, DEPTH_RECT.center());
    assert_eq!(pixels(&depth), [EMPTY_RGBA]);
    assert!(depth.undo());
    assert_eq!(pixels(&depth), [[255, 0, 255, 255]]);
}

#[test]
fn primary_release_outside_pair_finishes_one_stroke_command() {
    let context = egui::Context::default();
    let mut state = desktop_app::canvas::CanvasPairState::default();
    let mut document = document(3, 1);
    document.set_current_rgb([40, 50, 60]);
    let start = COLOR_RECT.left_center() + egui::vec2(15.0, 0.0);
    let end = COLOR_RECT.right_center() - egui::vec2(15.0, 0.0);

    run_pair_frame(&context, &mut state, &mut document, Vec::new());
    run_pair_frame(
        &context,
        &mut state,
        &mut document,
        vec![
            moved(start),
            button(start, egui::PointerButton::Primary, true),
        ],
    );
    run_pair_frame(&context, &mut state, &mut document, vec![moved(end)]);
    run_pair_frame(
        &context,
        &mut state,
        &mut document,
        vec![button(
            pos2(170.0, 170.0),
            egui::PointerButton::Primary,
            false,
        )],
    );

    assert!(!document.stroke_active());
    assert_eq!(pixels(&document), [[40, 50, 60, 0]; 3]);
    assert!(document.undo());
    assert_eq!(pixels(&document), [EMPTY_RGBA; 3]);
    assert!(!document.undo());
}
