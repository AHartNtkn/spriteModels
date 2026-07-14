use desktop_app::model_view::ModelView;
use editor_core::{ActiveLayer, DepthValue, EditorDocument, OrbitCamera, ReliefValue};
use eframe::egui::{
    self, Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, RawInput, Rect, TextureId,
    TextureOptions, TouchPhase, pos2, vec2,
};
use relief_core::{Bounds, CanonicalView};

const SCREEN: Rect = Rect::from_min_max(Pos2::ZERO, pos2(300.0, 200.0));
const MODEL: Rect = Rect::from_min_max(pos2(10.0, 10.0), pos2(290.0, 190.0));

fn document_with_one_undo() -> EditorDocument {
    let mut document = EditorDocument::new(Bounds::new(2, 2, 2).unwrap(), CanonicalView::Front);
    document.set_active_layer(ActiveLayer::Depth);
    document.set_current_depth(DepthValue::Relief(ReliefValue::new(8).unwrap()));
    document.begin_stroke().unwrap();
    document.pencil_pixel(CanonicalView::Front, 0, 0).unwrap();
    document.finish_stroke().unwrap();
    document
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(SCREEN),
        events,
        ..Default::default()
    }
}

fn run_frame(
    context: &egui::Context,
    view: &mut ModelView,
    document: &EditorDocument,
    events: Vec<Event>,
) -> egui::FullOutput {
    context.run_ui(input(events), |ui| {
        view.show(ui, document, MODEL).unwrap();
    })
}

fn pointer_button(position: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos: position,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

fn wheel(delta_y: f32) -> Event {
    Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, delta_y),
        phase: TouchPhase::Move,
        modifiers: Modifiers::NONE,
    }
}

fn model_texture_delta(output: &egui::FullOutput) -> Option<(TextureId, TextureOptions)> {
    output
        .textures_delta
        .set
        .iter()
        .find(|(_, delta)| delta.image.size() == [10, 10])
        .map(|(id, delta)| (*id, delta.options))
}

fn source_bytes(document: &EditorDocument) -> Vec<(CanonicalView, Vec<[u8; 4]>)> {
    document
        .sources()
        .map(|source| (source.view(), source.rgba().to_vec()))
        .collect()
}

#[test]
fn revision_refreshes_one_reused_nearest_neighbor_texture() {
    let context = egui::Context::default();
    let mut document = document_with_one_undo();
    let mut view = ModelView::default();

    let initial = run_frame(&context, &mut view, &document, Vec::new());
    let (texture_id, options) =
        model_texture_delta(&initial).expect("initial model texture upload");
    assert_eq!(options, TextureOptions::NEAREST);

    let unchanged = run_frame(&context, &mut view, &document, Vec::new());
    assert!(model_texture_delta(&unchanged).is_none());

    document.begin_stroke().unwrap();
    document.pencil_pixel(CanonicalView::Front, 1, 0).unwrap();
    document.finish_stroke().unwrap();
    let refreshed = run_frame(&context, &mut view, &document, Vec::new());
    assert_eq!(
        model_texture_delta(&refreshed),
        Some((texture_id, TextureOptions::NEAREST))
    );
}

#[test]
fn successive_drag_frames_apply_incremental_deltas_and_leave_document_untouched() {
    let context = egui::Context::default();
    let mut document = document_with_one_undo();
    let bytes = source_bytes(&document);
    let dirty = document.is_dirty();
    let revision = document.revision();
    let mut view = ModelView::default();
    run_frame(&context, &mut view, &document, Vec::new());

    let start = MODEL.center();
    let zoomed = run_frame(
        &context,
        &mut view,
        &document,
        vec![Event::PointerMoved(start), pointer_button(start, true)],
    );
    run_frame(
        &context,
        &mut view,
        &document,
        vec![Event::PointerMoved(start + vec2(10.0, 4.0))],
    );
    run_frame(
        &context,
        &mut view,
        &document,
        vec![Event::PointerMoved(start + vec2(20.0, 8.0))],
    );

    let mut expected = OrbitCamera::default();
    expected.drag(10.0, 4.0);
    expected.drag(10.0, 4.0);
    assert_eq!(view.camera(), expected);
    assert_eq!(source_bytes(&document), bytes);
    assert_eq!(document.is_dirty(), dirty);
    assert_eq!(document.revision(), revision);
    assert!(document.can_undo());
    assert!(
        model_texture_delta(&zoomed).is_none(),
        "presentation zoom must not regenerate or upload model geometry"
    );
    assert!(document.undo());
    assert!(!document.undo(), "model drag added no undo entry");
}

#[test]
fn wheel_over_model_changes_only_preview_zoom_and_reset_restores_default_camera() {
    let context = egui::Context::default();
    let document = document_with_one_undo();
    let bytes = source_bytes(&document);
    let dirty = document.is_dirty();
    let revision = document.revision();
    let mut view = ModelView::default();
    run_frame(&context, &mut view, &document, Vec::new());

    run_frame(
        &context,
        &mut view,
        &document,
        vec![Event::PointerMoved(MODEL.center()), wheel(3.0)],
    );
    let mut expected = OrbitCamera::default();
    expected.zoom(3.0);
    assert_eq!(view.camera(), expected);
    assert_eq!(source_bytes(&document), bytes);
    assert_eq!(document.is_dirty(), dirty);
    assert_eq!(document.revision(), revision);
    assert!(document.can_undo());

    view.reset();
    assert_eq!(view.camera(), OrbitCamera::default());
    assert_eq!(source_bytes(&document), bytes);
    assert_eq!(document.revision(), revision);
}
