use depthsprite_format::DepthSpriteModel;
use editor_core::{ActiveLayer, DepthValue, EditorDocument, EditorError};
use relief_core::{Bounds, CanonicalView, Chart};

const VIEW: CanonicalView = CanonicalView::Front;

fn document(width: u32, height: u32, pixels: Vec<[u8; 4]>) -> EditorDocument {
    let bounds = Bounds::new(width, height, 1).unwrap();
    let chart = Chart::from_rgba(VIEW, width, height, pixels).unwrap();
    let model = DepthSpriteModel::new(bounds, vec![chart]).unwrap();
    EditorDocument::from_model(model, None).unwrap()
}

fn pixels(document: &EditorDocument) -> Vec<[u8; 4]> {
    document.source(VIEW).unwrap().rgba().to_vec()
}

#[test]
fn color_pencil_changes_rgb_and_preserves_alpha() {
    let mut document = document(1, 1, vec![[1, 2, 3, 77]]);
    document.set_active_layer(ActiveLayer::Color);
    document.set_current_rgb([9, 8, 7]);

    document.begin_stroke().unwrap();
    assert!(document.pencil_pixel(VIEW, 0, 0).unwrap());
    assert!(document.finish_stroke().unwrap());

    assert_eq!(pixels(&document), [[9, 8, 7, 77]]);
}

#[test]
fn depth_pencil_encodes_relief_and_preserves_rgb() {
    let mut document = document(1, 1, vec![[11, 22, 33, 19]]);
    document.set_active_layer(ActiveLayer::Depth);
    document.set_current_depth(DepthValue::Relief(42)).unwrap();

    document.begin_stroke().unwrap();
    assert!(document.pencil_pixel(VIEW, 0, 0).unwrap());
    document.finish_stroke().unwrap();

    assert_eq!(pixels(&document), [[11, 22, 33, 213]]);
}

#[test]
fn depth_pencil_adds_geometry_to_an_empty_pixel_without_discarding_rgb() {
    let mut document = document(1, 1, vec![[31, 32, 33, 0]]);
    document.set_active_layer(ActiveLayer::Depth);
    document.set_current_depth(DepthValue::Relief(0)).unwrap();

    document.begin_stroke().unwrap();
    assert!(document.pencil_pixel(VIEW, 0, 0).unwrap());
    document.finish_stroke().unwrap();

    assert_eq!(pixels(&document), [[31, 32, 33, 255]]);
}

#[test]
fn only_depth_eraser_is_available_and_it_preserves_rgb() {
    let original = [40, 41, 42, 180];
    let mut color = document(1, 1, vec![original]);
    color.set_active_layer(ActiveLayer::Color);
    color.begin_stroke().unwrap();
    assert!(!color.erase_pixel(VIEW, 0, 0).unwrap());
    assert!(!color.finish_stroke().unwrap());
    assert_eq!(pixels(&color), [original]);

    let mut depth = document(1, 1, vec![original]);
    depth.set_active_layer(ActiveLayer::Depth);
    depth.begin_stroke().unwrap();
    assert!(depth.erase_pixel(VIEW, 0, 0).unwrap());
    assert!(depth.finish_stroke().unwrap());
    assert_eq!(pixels(&depth), [[40, 41, 42, 0]]);
}

#[test]
fn empty_or_out_of_range_depth_cannot_be_painted() {
    let original = [50, 51, 52, 90];
    let mut document = document(1, 1, vec![original]);
    document.set_active_layer(ActiveLayer::Depth);
    document.set_current_depth(DepthValue::Empty).unwrap();
    document.begin_stroke().unwrap();
    assert!(!document.pencil_pixel(VIEW, 0, 0).unwrap());
    assert!(!document.finish_stroke().unwrap());
    assert_eq!(pixels(&document), [original]);

    assert!(matches!(
        document.set_current_depth(DepthValue::Relief(255)),
        Err(EditorError::InvalidRelief(255))
    ));
    assert_eq!(document.current_depth(), DepthValue::Empty);
}

#[test]
fn color_fill_uses_only_contiguous_seed_rgb_and_preserves_each_alpha() {
    let a = [1, 2, 3];
    let b = [4, 5, 6];
    let mut document = document(
        3,
        2,
        vec![
            [a[0], a[1], a[2], 10],
            [a[0], a[1], a[2], 20],
            [b[0], b[1], b[2], 30],
            [a[0], a[1], a[2], 40],
            [b[0], b[1], b[2], 50],
            [a[0], a[1], a[2], 60],
        ],
    );
    document.set_active_layer(ActiveLayer::Color);
    document.set_current_rgb([9, 8, 7]);

    assert!(document.fill(VIEW, 0, 0).unwrap());

    assert_eq!(
        pixels(&document),
        [
            [9, 8, 7, 10],
            [9, 8, 7, 20],
            [4, 5, 6, 30],
            [9, 8, 7, 40],
            [4, 5, 6, 50],
            [1, 2, 3, 60],
        ]
    );
}

#[test]
fn depth_fill_uses_only_contiguous_seed_alpha_and_preserves_each_rgb() {
    let mut document = document(
        3,
        2,
        vec![
            [1, 2, 3, 10],
            [4, 5, 6, 10],
            [7, 8, 9, 20],
            [10, 11, 12, 10],
            [13, 14, 15, 20],
            [16, 17, 18, 10],
        ],
    );
    document.set_active_layer(ActiveLayer::Depth);
    document.set_current_depth(DepthValue::Relief(200)).unwrap();

    assert!(document.fill(VIEW, 0, 0).unwrap());

    assert_eq!(
        pixels(&document),
        [
            [1, 2, 3, 55],
            [4, 5, 6, 55],
            [7, 8, 9, 20],
            [10, 11, 12, 55],
            [13, 14, 15, 20],
            [16, 17, 18, 10],
        ]
    );
}

#[test]
fn depth_fill_cannot_paint_the_explicit_empty_selection() {
    let original = vec![[1, 2, 3, 20], [4, 5, 6, 20]];
    let mut document = document(2, 1, original.clone());
    document.set_active_layer(ActiveLayer::Depth);
    document.set_current_depth(DepthValue::Empty).unwrap();

    assert!(!document.fill(VIEW, 0, 0).unwrap());
    assert_eq!(pixels(&document), original);
}

#[test]
fn eyedropper_selects_color_relief_and_explicit_empty_depth() {
    let mut document = document(3, 1, vec![[7, 8, 9, 200], [1, 2, 3, 0], [4, 5, 6, 1]]);

    document.set_active_layer(ActiveLayer::Color);
    document.eyedrop(VIEW, 0, 0).unwrap();
    assert_eq!(document.current_rgb(), [7, 8, 9]);

    document.set_active_layer(ActiveLayer::Depth);
    document.eyedrop(VIEW, 0, 0).unwrap();
    assert_eq!(document.current_depth(), DepthValue::Relief(55));
    document.eyedrop(VIEW, 1, 0).unwrap();
    assert_eq!(document.current_depth(), DepthValue::Empty);
    document.eyedrop(VIEW, 2, 0).unwrap();
    assert_eq!(document.current_depth(), DepthValue::Relief(254));
}

#[test]
fn pixel_commands_reject_missing_sources_out_of_bounds_and_invalid_stroke_lifecycle() {
    let mut document = document(1, 1, vec![[1, 2, 3, 4]]);
    assert!(matches!(
        document.pencil_pixel(VIEW, 0, 0),
        Err(EditorError::NoActiveStroke)
    ));
    document.begin_stroke().unwrap();
    assert!(matches!(
        document.begin_stroke(),
        Err(EditorError::StrokeAlreadyActive)
    ));
    assert!(matches!(
        document.pencil_pixel(CanonicalView::Back, 0, 0),
        Err(EditorError::SourceNotFound(CanonicalView::Back))
    ));
    assert!(matches!(
        document.pencil_pixel(VIEW, 1, 0),
        Err(EditorError::PixelOutOfBounds {
            view: VIEW,
            x: 1,
            y: 0,
        })
    ));
    document.cancel_stroke();
    assert!(matches!(
        document.finish_stroke(),
        Err(EditorError::NoActiveStroke)
    ));
}
