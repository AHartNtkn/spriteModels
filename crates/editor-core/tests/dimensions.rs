use editor_core::{
    ActiveLayer, DepthValue, EditorDocument, EditorError, OrbitCamera, PreviewCache, ReliefValue,
    Tool,
};
use relief_core::{
    AuthoredModel, Bounds, CanonicalView, Chart, DiscardPolicy, ImageEdge, ModelError,
    ReassignMode, ResizeDelta, ResizeRequest,
};

fn empty_chart(bounds: Bounds, view: CanonicalView) -> Chart {
    let (width, height) = view.dimensions(bounds);
    Chart::from_rgba(
        view,
        width,
        height,
        vec![relief_core::EMPTY_RGBA; (width * height) as usize],
    )
    .unwrap()
}

fn document(bounds: Bounds, views: &[CanonicalView]) -> EditorDocument {
    let charts = views
        .iter()
        .copied()
        .map(|view| empty_chart(bounds, view))
        .collect();
    EditorDocument::from_model(AuthoredModel::new(bounds, charts).unwrap(), None)
}

#[test]
fn selecting_a_lower_maximum_source_clamps_only_transient_tool_depth() {
    let bounds = Bounds::new(2, 3, 4).unwrap();
    let mut document = document(bounds, &[CanonicalView::Front, CanonicalView::Left]);
    document.set_active_layer(ActiveLayer::Depth);
    document.set_tool(Tool::Fill);
    document.set_current_rgb([9, 8, 7]);
    document.set_current_depth(DepthValue::Relief(ReliefValue::new(12).unwrap()));
    let before_model = document.to_model();
    let before_revision = document.revision();

    document.select_source(CanonicalView::Left).unwrap();

    assert_eq!(document.selected_view(), CanonicalView::Left);
    assert_eq!(document.maximum_inward_depth(), 8);
    assert_eq!(
        document.current_depth(),
        DepthValue::Relief(ReliefValue::new(8).unwrap())
    );
    assert_eq!(document.active_layer(), ActiveLayer::Depth);
    assert_eq!(document.tool(), Tool::Fill);
    assert_eq!(document.current_rgb(), [9, 8, 7]);
    assert_eq!(document.to_model(), before_model);
    assert_eq!(document.revision(), before_revision);
    assert!(!document.is_dirty());
    assert!(!document.can_undo());
    assert!(!document.can_redo());
}

#[test]
fn selecting_a_missing_source_is_transactional() {
    let bounds = Bounds::new(2, 3, 4).unwrap();
    let mut document = document(bounds, &[CanonicalView::Front]);
    document.set_current_depth(DepthValue::Relief(ReliefValue::new(12).unwrap()));
    let before_model = document.to_model();
    let before_selection = document.selected_view();
    let before_depth = document.current_depth();

    assert!(matches!(
        document.select_source(CanonicalView::Left),
        Err(EditorError::Model(ModelError::MissingView(
            CanonicalView::Left
        )))
    ));
    assert_eq!(document.to_model(), before_model);
    assert_eq!(document.selected_view(), before_selection);
    assert_eq!(document.current_depth(), before_depth);
    assert_eq!(document.revision(), 0);
    assert!(!document.is_dirty());
    assert!(!document.can_undo());
}

#[test]
fn successful_resize_advances_document_and_preview_once() {
    let bounds = Bounds::new(2, 2, 2).unwrap();
    let mut document = document(bounds, &[CanonicalView::Front]);
    let mut preview = PreviewCache::default();
    let camera = OrbitCamera::default();
    let initial_generation = preview.frame(&document, camera).unwrap().generation();
    let initial_revision = document.revision();

    document
        .resize_source(
            ResizeRequest {
                view: CanonicalView::Front,
                edge: ImageEdge::Left,
                delta: ResizeDelta::Add,
            },
            DiscardPolicy::Reject,
        )
        .unwrap();

    assert_eq!(document.revision(), initial_revision + 1);
    assert_eq!(
        preview.frame(&document, camera).unwrap().generation(),
        initial_generation + 1
    );
    assert_eq!(
        preview.frame(&document, camera).unwrap().generation(),
        initial_generation + 1
    );
}

#[test]
fn failed_resize_changes_no_document_history_or_preview_identity() {
    let bounds = Bounds::new(2, 2, 2).unwrap();
    let occupied = Chart::from_rgba(CanonicalView::Front, 2, 2, vec![[1, 2, 3, 255]; 4]).unwrap();
    let mut document =
        EditorDocument::from_model(AuthoredModel::new(bounds, vec![occupied]).unwrap(), None);
    let before_model = document.to_model();
    let before_revision = document.revision();
    let before_dirty = document.is_dirty();
    let mut preview = PreviewCache::default();
    let camera = OrbitCamera::default();
    let before_generation = preview.frame(&document, camera).unwrap().generation();

    assert!(matches!(
        document.resize_source(
            ResizeRequest {
                view: CanonicalView::Front,
                edge: ImageEdge::Left,
                delta: ResizeDelta::Remove,
            },
            DiscardPolicy::Reject,
        ),
        Err(EditorError::Model(ModelError::ResizeWouldDiscard { .. }))
    ));

    assert_eq!(document.to_model(), before_model);
    assert_eq!(document.selected_view(), CanonicalView::Front);
    assert_eq!(document.revision(), before_revision);
    assert_eq!(document.is_dirty(), before_dirty);
    assert!(!document.can_undo());
    assert!(!document.can_redo());
    assert_eq!(
        preview.frame(&document, camera).unwrap().generation(),
        before_generation
    );
}

#[test]
fn failed_reassignment_changes_no_document_history_or_preview_identity() {
    let bounds = Bounds::new(2, 3, 4).unwrap();
    let mut document = document(bounds, &[CanonicalView::Front, CanonicalView::Back]);
    let before_model = document.to_model();
    let before_revision = document.revision();
    let mut preview = PreviewCache::default();
    let camera = OrbitCamera::default();
    let before_generation = preview.frame(&document, camera).unwrap().generation();

    assert!(matches!(
        document.reassign_source(
            CanonicalView::Front,
            CanonicalView::Back,
            ReassignMode::Preserve,
        ),
        Err(EditorError::Model(ModelError::DuplicateView(
            CanonicalView::Back
        )))
    ));

    assert_eq!(document.to_model(), before_model);
    assert_eq!(document.selected_view(), CanonicalView::Front);
    assert_eq!(document.revision(), before_revision);
    assert!(!document.is_dirty());
    assert!(!document.can_undo());
    assert!(!document.can_redo());
    assert_eq!(
        preview.frame(&document, camera).unwrap().generation(),
        before_generation
    );
}
