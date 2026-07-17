use desktop_app::model_view::ModelView;
use editor_core::{ActiveLayer, DepthValue, EditorDocument, OrbitCamera, ReliefValue};
use eframe::egui::{
    self, Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, RawInput, Rect, TextureId,
    TextureOptions, TouchPhase, pos2, vec2,
};
use relief_core::{Bounds, CanonicalView};

const SCREEN: Rect = Rect::from_min_max(Pos2::ZERO, pos2(300.0, 200.0));
const MODEL: Rect = Rect::from_min_max(pos2(10.0, 10.0), pos2(290.0, 190.0));
const RESIZED_MODEL: Rect = Rect::from_min_max(pos2(60.0, 30.0), pos2(240.0, 170.0));

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
    model_rect: Rect,
    events: Vec<Event>,
) -> egui::FullOutput {
    context.run_ui(input(events), |ui| {
        view.show(ui, document, model_rect).unwrap();
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
        .find(|(_, delta)| delta.image.size() == [8, 8])
        .map(|(id, delta)| (*id, delta.options))
}

fn model_mesh(output: &egui::FullOutput, texture_id: TextureId) -> (Rect, Rect) {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Mesh(mesh) if mesh.texture_id == texture_id => {
                Some((clipped.clip_rect, mesh.calc_bounds()))
            }
            _ => None,
        })
        .expect("model texture is painted in the real egui frame")
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

    let initial = run_frame(&context, &mut view, &document, MODEL, Vec::new());
    let (texture_id, options) =
        model_texture_delta(&initial).expect("initial model texture upload");
    assert_eq!(options, TextureOptions::NEAREST);

    let unchanged = run_frame(&context, &mut view, &document, MODEL, Vec::new());
    assert!(model_texture_delta(&unchanged).is_none());

    document.begin_stroke().unwrap();
    document.pencil_pixel(CanonicalView::Front, 1, 0).unwrap();
    document.finish_stroke().unwrap();
    let refreshed = run_frame(&context, &mut view, &document, MODEL, Vec::new());
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
    run_frame(&context, &mut view, &document, MODEL, Vec::new());

    let start = MODEL.center();
    run_frame(
        &context,
        &mut view,
        &document,
        MODEL,
        vec![Event::PointerMoved(start), pointer_button(start, true)],
    );
    run_frame(
        &context,
        &mut view,
        &document,
        MODEL,
        vec![Event::PointerMoved(start + vec2(10.0, 4.0))],
    );
    run_frame(
        &context,
        &mut view,
        &document,
        MODEL,
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
    assert!(document.undo());
    assert!(!document.undo(), "model drag added no undo entry");
}

#[test]
fn real_frames_fit_zoom_resize_reset_and_clip_without_texture_uploads() {
    let context = egui::Context::default();
    let document = document_with_one_undo();
    let bytes = source_bytes(&document);
    let dirty = document.is_dirty();
    let revision = document.revision();
    let mut view = ModelView::default();
    let initial = run_frame(&context, &mut view, &document, MODEL, Vec::new());
    let (texture_id, _) = model_texture_delta(&initial).expect("initial model texture upload");
    let (initial_clip, initial_image) = model_mesh(&initial, texture_id);
    assert_eq!(initial_clip, MODEL);
    assert!(MODEL.contains_rect(initial_image));

    let resized = run_frame(&context, &mut view, &document, RESIZED_MODEL, Vec::new());
    assert!(model_texture_delta(&resized).is_none());
    let (resized_clip, resized_image) = model_mesh(&resized, texture_id);
    assert_eq!(resized_clip, RESIZED_MODEL);
    assert_ne!(resized_image, initial_image, "resize recomputes fit");
    assert!(resized_image.width() < initial_image.width());

    let restored_stage = run_frame(&context, &mut view, &document, MODEL, Vec::new());
    assert!(model_texture_delta(&restored_stage).is_none());
    assert_eq!(model_mesh(&restored_stage, texture_id).1, initial_image);

    let zoomed = run_frame(
        &context,
        &mut view,
        &document,
        MODEL,
        vec![Event::PointerMoved(MODEL.center()), wheel(200.0)],
    );
    assert!(
        model_texture_delta(&zoomed).is_none(),
        "presentation zoom must not regenerate or upload model geometry"
    );
    let (zoom_clip, zoomed_image) = model_mesh(&zoomed, texture_id);
    assert_eq!(zoom_clip, MODEL, "painting remains clipped to the stage");
    assert!(zoomed_image.width() > initial_image.width());
    assert!(
        !MODEL.contains_rect(zoomed_image),
        "the oversized image proves the clip is active"
    );
    assert_eq!(
        view.camera(),
        OrbitCamera::default(),
        "presentation zoom is not editor-core camera state"
    );
    assert_eq!(source_bytes(&document), bytes);
    assert_eq!(document.is_dirty(), dirty);
    assert_eq!(document.revision(), revision);
    assert!(document.can_undo());

    view.reset();
    let reset = run_frame(
        &context,
        &mut view,
        &document,
        MODEL,
        vec![Event::PointerMoved(Pos2::ZERO)],
    );
    assert!(model_texture_delta(&reset).is_none());
    assert_eq!(model_mesh(&reset, texture_id).1, initial_image);
    assert_eq!(view.camera(), OrbitCamera::default());
    assert_eq!(source_bytes(&document), bytes);
    assert_eq!(document.revision(), revision);
}
