use editor_core::{ActiveLayer, DepthValue, EditorDocument, ReliefValue};
use relief_core::{
    AuthoredModel, Bounds, CanonicalView, Chart, DiscardPolicy, ImageEdge, ReassignMode,
    ResizeDelta, ResizeRequest,
};

const FRONT: CanonicalView = CanonicalView::Front;

fn document(width: u32, height: u32, pixels: Vec<[u8; 4]>) -> EditorDocument {
    let bounds = Bounds::new(width, height, 63).unwrap();
    let chart = Chart::from_rgba(FRONT, width, height, pixels).unwrap();
    let model = AuthoredModel::new(bounds, vec![chart]).unwrap();
    EditorDocument::from_model(model, None)
}

fn two_source_document() -> EditorDocument {
    let bounds = Bounds::new(2, 1, 63).unwrap();
    let front = Chart::from_rgba(FRONT, 2, 1, vec![[1, 2, 3, 4]; 2]).unwrap();
    let back = Chart::from_rgba(CanonicalView::Back, 2, 1, vec![[5, 6, 7, 8]; 2]).unwrap();
    let model = AuthoredModel::new(bounds, vec![front, back]).unwrap();
    EditorDocument::from_model(model, None)
}

fn pixels(document: &EditorDocument, view: CanonicalView) -> Vec<[u8; 4]> {
    document.source(view).unwrap().rgba().to_vec()
}

fn relief(value: u8) -> DepthValue {
    DepthValue::Relief(ReliefValue::new(value).unwrap())
}

#[test]
fn several_pixel_writes_in_one_drag_are_one_undo_step() {
    let original = vec![[1, 2, 3, 4]; 3];
    let mut document = document(3, 1, original.clone());
    document.set_active_layer(ActiveLayer::Color);
    document.set_current_rgb([9, 8, 7]);

    document.begin_stroke().unwrap();
    for x in 0..3 {
        assert!(document.pencil_pixel(FRONT, x, 0).unwrap());
    }
    assert!(document.finish_stroke().unwrap());
    assert_eq!(pixels(&document, FRONT), vec![[9, 8, 7, 4]; 3]);

    assert!(document.undo());
    assert_eq!(pixels(&document, FRONT), original);
    assert!(!document.can_undo());
    assert!(document.redo());
    assert_eq!(pixels(&document, FRONT), vec![[9, 8, 7, 4]; 3]);
}

#[test]
fn selection_changes_without_pixel_writes_do_not_create_a_stroke_command() {
    let original = vec![[7, 8, 9, 200], [1, 2, 3, 0]];
    let mut document = document(2, 1, original.clone());

    document.begin_stroke().unwrap();
    document.set_current_rgb([90, 91, 92]);
    document.eyedrop(FRONT, 0, 0).unwrap();
    document.set_active_layer(ActiveLayer::Depth);
    document.set_current_depth(relief(42));
    document.eyedrop(FRONT, 1, 0).unwrap();

    assert!(!document.finish_stroke().unwrap());
    assert_eq!(pixels(&document, FRONT), original);
    assert!(!document.can_undo());
    assert!(!document.undo());
}

#[test]
fn fill_is_one_undo_step() {
    let original = vec![[1, 2, 3, 4]; 3];
    let mut document = document(3, 1, original.clone());
    document.set_current_rgb([8, 7, 6]);

    assert!(document.fill(FRONT, 0, 0).unwrap());
    assert!(document.undo());
    assert_eq!(pixels(&document, FRONT), original);
    assert!(!document.can_undo());
}

#[test]
fn add_replace_and_remove_are_each_one_undo_step() {
    let mut added = document(2, 1, vec![[1, 2, 3, 4]; 2]);
    added.add_source(CanonicalView::Back).unwrap();
    assert!(added.undo());
    assert!(added.source(CanonicalView::Back).is_none());
    assert!(!added.can_undo());

    let original = vec![[1, 2, 3, 4]; 2];
    let mut replaced = document(2, 1, original.clone());
    replaced
        .replace_source(Chart::from_rgba(FRONT, 2, 1, vec![[9, 8, 7, 6]; 2]).unwrap())
        .unwrap();
    assert!(replaced.undo());
    assert_eq!(pixels(&replaced, FRONT), original);
    assert!(!replaced.can_undo());

    let mut removed = two_source_document();
    removed.remove_source(CanonicalView::Back).unwrap();
    assert!(removed.undo());
    assert_eq!(pixels(&removed, CanonicalView::Back), vec![[5, 6, 7, 8]; 2]);
    assert!(!removed.can_undo());
}

#[test]
fn a_new_edit_clears_redo() {
    let mut document = document(2, 1, vec![[1, 2, 3, 4]; 2]);
    document.set_current_rgb([9, 9, 9]);
    document.begin_stroke().unwrap();
    document.pencil_pixel(FRONT, 0, 0).unwrap();
    document.finish_stroke().unwrap();
    assert!(document.undo());
    assert!(document.can_redo());

    document.set_current_rgb([8, 8, 8]);
    assert!(document.fill(FRONT, 0, 0).unwrap());

    assert!(!document.can_redo());
    assert!(!document.redo());
}

#[test]
fn successful_mutations_and_restorations_advance_revision_monotonically() {
    let original = vec![[1, 2, 3, 4]; 2];
    let mut document = document(2, 1, original.clone());
    document.set_active_layer(ActiveLayer::Depth);
    document.set_current_depth(relief(10));
    let initial = document.revision();

    document.begin_stroke().unwrap();
    assert!(document.pencil_pixel(FRONT, 0, 0).unwrap());
    let after_first_write = document.revision();
    assert!(after_first_write > initial);
    assert!(!document.pencil_pixel(FRONT, 0, 0).unwrap());
    assert_eq!(document.revision(), after_first_write);
    assert!(document.pencil_pixel(FRONT, 1, 0).unwrap());
    let after_second_write = document.revision();
    assert!(after_second_write > after_first_write);
    document.finish_stroke().unwrap();
    assert_eq!(document.revision(), after_second_write);

    assert!(document.undo());
    let after_undo = document.revision();
    assert!(after_undo > after_second_write);
    assert!(document.redo());
    let after_redo = document.revision();
    assert!(after_redo > after_undo);

    document.begin_stroke().unwrap();
    document.erase_pixel(FRONT, 0, 0).unwrap();
    let after_cancelled_write = document.revision();
    assert!(after_cancelled_write > after_redo);
    document.cancel_stroke();
    assert!(document.revision() > after_cancelled_write);
    assert_eq!(pixels(&document, FRONT), vec![[1, 2, 3, 245]; 2]);
}

#[test]
fn source_commands_and_fill_each_advance_revision_once() {
    let mut document = document(2, 1, vec![[1, 2, 3, 4]; 2]);
    let before_fill = document.revision();
    document.set_current_rgb([9, 8, 7]);
    document.fill(FRONT, 0, 0).unwrap();
    assert_eq!(document.revision(), before_fill + 1);

    let before_add = document.revision();
    document.add_source(CanonicalView::Back).unwrap();
    assert_eq!(document.revision(), before_add + 1);

    let before_replace = document.revision();
    document
        .replace_source(Chart::from_rgba(FRONT, 2, 1, vec![[2, 3, 4, 5]; 2]).unwrap())
        .unwrap();
    assert_eq!(document.revision(), before_replace + 1);

    let before_remove = document.revision();
    document.remove_source(CanonicalView::Back).unwrap();
    assert_eq!(document.revision(), before_remove + 1);
}

#[test]
fn resize_is_exactly_one_undo_step() {
    let mut document = document(2, 1, vec![[255, 0, 255, 0]; 2]);
    let before = document.to_model();
    let before_revision = document.revision();

    document
        .resize_source(
            ResizeRequest {
                view: FRONT,
                edge: ImageEdge::Left,
                delta: ResizeDelta::Add,
            },
            DiscardPolicy::Reject,
        )
        .unwrap();

    assert_eq!(document.revision(), before_revision + 1);
    assert_eq!(document.bounds().width(), 3);
    assert!(document.undo());
    assert_eq!(document.to_model(), before);
    assert!(!document.can_undo());
    assert!(document.redo());
    assert_eq!(document.bounds().width(), 3);
    assert!(!document.can_redo());
}

#[test]
fn reassignment_is_exactly_one_undo_step_and_tracks_the_selected_source() {
    let mut document = document(2, 1, vec![[1, 2, 3, 255]; 2]);
    let before = document.to_model();
    let before_revision = document.revision();

    document
        .reassign_source(FRONT, CanonicalView::Back, ReassignMode::Preserve)
        .unwrap();

    assert_eq!(document.revision(), before_revision + 1);
    assert_eq!(document.selected_view(), CanonicalView::Back);
    assert!(document.source(FRONT).is_none());
    assert!(document.undo());
    assert_eq!(document.to_model(), before);
    assert_eq!(document.selected_view(), FRONT);
    assert!(!document.can_undo());
    assert!(document.redo());
    assert_eq!(document.selected_view(), CanonicalView::Back);
    assert!(!document.can_redo());
}
