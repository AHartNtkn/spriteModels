use std::{io::Cursor, path::PathBuf};

use depthsprite_format::{DepthSpriteModel, load_path, load_reader, save_writer};
use editor_core::{
    ActiveLayer, DepthValue, EditorDocument, OrbitCamera, PreviewCache, ReliefValue, SourceSprite,
};
use relief_core::{Bounds, CanonicalView, Chart};

fn one_pixel_document() -> EditorDocument {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let chart = Chart::from_rgba(CanonicalView::Front, 1, 1, vec![[11, 22, 33, 255]]).unwrap();
    let model = DepthSpriteModel::new(bounds, vec![chart]).unwrap();
    EditorDocument::from_model(model, None).unwrap()
}

fn bowl_document() -> EditorDocument {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("assets/examples/bowl.depthsprite");
    EditorDocument::from_model(load_path(path).unwrap(), None).unwrap()
}

fn recolor(document: &mut EditorDocument, view: CanonicalView, rgb: [u8; 3]) {
    let source = document.source(view).unwrap();
    let (width, height) = source.dimensions();
    let rgba = source
        .rgba()
        .iter()
        .map(|pixel| {
            if pixel[3] == 0 {
                *pixel
            } else {
                [rgb[0], rgb[1], rgb[2], pixel[3]]
            }
        })
        .collect();
    document
        .replace_source(SourceSprite::from_rgba(view, width, height, rgba).unwrap())
        .unwrap();
}

#[test]
fn dragging_changes_only_camera_state() {
    let mut document = one_pixel_document();
    document.set_active_layer(ActiveLayer::Depth);
    document.set_current_depth(DepthValue::Relief(ReliefValue::new(8).unwrap()));
    document.begin_stroke().unwrap();
    document.pencil_pixel(CanonicalView::Front, 0, 0).unwrap();
    document.finish_stroke().unwrap();
    let content = document.to_model().unwrap();
    let dirty = document.is_dirty();
    let revision = document.revision();
    let mut camera = OrbitCamera::default();
    let initial_camera = camera;

    camera.drag(12.25, -7.75);

    assert_ne!(camera, initial_camera);
    assert_eq!(document.to_model().unwrap(), content);
    assert_eq!(document.is_dirty(), dirty);
    assert_eq!(document.revision(), revision);
    assert!(document.undo(), "the one authored undo entry remains");
    assert!(!document.undo(), "camera drag added no undo entry");
}

#[test]
fn reset_restores_the_default_view() {
    let expected = OrbitCamera::default();
    let mut camera = expected;
    camera.drag(-81.5, 39.25);
    camera.zoom(6.0);
    assert_ne!(camera, expected);

    camera.reset();

    assert_eq!(camera, expected);
    assert_eq!(camera.target_view(), expected.target_view());
}

#[test]
fn non_finite_orbit_input_is_ignored() {
    let expected = OrbitCamera::default();
    let mut camera = expected;

    camera.drag(f32::NAN, f32::INFINITY);
    camera.zoom(f32::NEG_INFINITY);

    assert_eq!(camera, expected);
}

#[test]
fn extreme_finite_orbit_input_remains_bounded_and_deterministic() {
    let mut first = OrbitCamera::default();
    first.drag(f32::MAX, f32::MAX);
    first.zoom(f32::MAX);
    let mut second = OrbitCamera::default();
    second.drag(f32::MAX, f32::MAX);
    second.zoom(f32::MAX);
    assert_eq!(first, second);
    assert_eq!(first.target_view(), second.target_view());

    let pitch_maximum = first;
    first.drag(0.0, 1.0);
    assert_eq!(first, pitch_maximum);
    let zoom_maximum = first;
    first.zoom(1.0);
    assert_eq!(first, zoom_maximum);

    first.drag(0.0, f32::MIN);
    let pitch_minimum = first;
    first.drag(0.0, -1.0);
    assert_eq!(first, pitch_minimum);
    first.zoom(f32::MIN);
    let zoom_minimum = first;
    first.zoom(-1.0);
    assert_eq!(first, zoom_minimum);
}

#[test]
fn an_unchanged_preview_key_renders_once() {
    let document = one_pixel_document();
    let camera = OrbitCamera::default();
    let mut preview = PreviewCache::default();

    let first = preview.frame(&document, camera, 48, 32).unwrap().clone();
    let second = preview.frame(&document, camera, 48, 32).unwrap().clone();

    assert_eq!(first, second);
    assert_eq!(preview.render_count_for_test(), 1);
}

#[test]
fn several_document_mutations_before_a_frame_render_once() {
    let mut document = one_pixel_document();
    let camera = OrbitCamera::default();
    let mut preview = PreviewCache::default();
    preview.frame(&document, camera, 48, 32).unwrap();

    for rgb in [[40, 50, 60], [70, 80, 90], [100, 110, 120]] {
        recolor(&mut document, CanonicalView::Front, rgb);
    }
    preview.frame(&document, camera, 48, 32).unwrap();
    preview.frame(&document, camera, 48, 32).unwrap();

    assert_eq!(document.revision(), 3);
    assert_eq!(preview.render_count_for_test(), 2);
}

#[test]
fn orbit_change_renders_once_without_document_mutation() {
    let document = one_pixel_document();
    let content = document.to_model().unwrap();
    let dirty = document.is_dirty();
    let revision = document.revision();
    let can_undo = document.can_undo();
    let mut camera = OrbitCamera::default();
    let mut preview = PreviewCache::default();
    preview.frame(&document, camera, 48, 32).unwrap();

    camera.drag(9.0, 4.0);
    preview.frame(&document, camera, 48, 32).unwrap();
    preview.frame(&document, camera, 48, 32).unwrap();

    assert_eq!(preview.render_count_for_test(), 2);
    assert_eq!(document.to_model().unwrap(), content);
    assert_eq!(document.is_dirty(), dirty);
    assert_eq!(document.revision(), revision);
    assert_eq!(document.can_undo(), can_undo);
}

#[test]
fn both_bowl_sources_update_preview_and_reopen_with_a_visible_recessed_basin() {
    let mut document = bowl_document();
    let camera = OrbitCamera::default();
    let mut preview = PreviewCache::default();
    let original = preview.frame(&document, camera, 96, 96).unwrap().clone();

    recolor(&mut document, CanonicalView::Front, [19, 211, 83]);
    let front_edited = preview.frame(&document, camera, 96, 96).unwrap().clone();
    assert_ne!(front_edited, original);

    recolor(&mut document, CanonicalView::Top, [67, 101, 239]);
    let both_edited = preview.frame(&document, camera, 96, 96).unwrap().clone();
    assert_ne!(both_edited, front_edited);
    assert_eq!(preview.render_count_for_test(), 3);

    let saved_model = document.to_model().unwrap();
    let mut package = Cursor::new(Vec::new());
    save_writer(&saved_model, &mut package).unwrap();
    package.set_position(0);
    let reopened_model = load_reader(package).unwrap();
    assert_eq!(reopened_model, saved_model);
    let reopened = EditorDocument::from_model(reopened_model, None).unwrap();
    let mut reopened_preview = PreviewCache::default();
    let reopened_frame = reopened_preview.frame(&reopened, camera, 96, 96).unwrap();
    assert_eq!(reopened_frame, &both_edited);

    let mut basin = None;
    let mut near_rim = None;
    for y in 0..reopened_frame.height() {
        for x in 0..reopened_frame.width() {
            let Some(owner) = reopened_frame.owner_at(x, y) else {
                continue;
            };
            if owner.view == CanonicalView::Top
                && (15..=16).contains(&owner.source_x)
                && (15..=16).contains(&owner.source_y)
            {
                basin = Some((x, y));
            }
            if owner.view == CanonicalView::Front && owner.source_y == 2 {
                near_rim = Some((x, y));
            }
        }
    }
    let basin = basin.expect("the deeply recessed top-center basin remains visible");
    let near_rim = near_rim.expect("the front near rim remains visible");
    assert!(
        basin.1 < near_rim.1,
        "the basin renders behind the near rim"
    );
}
