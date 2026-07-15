use std::{io::Cursor, path::PathBuf};

use depthsprite_format::{load_path, load_reader, save_writer};
use editor_core::{
    ActiveLayer, DepthValue, EditorDocument, OrbitCamera, PreviewCache, ReliefValue,
};
use relief_core::{AuthoredModel, Bounds, CanonicalView, Chart};
use relief_render::FrameBuffer;

fn one_pixel_document() -> EditorDocument {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let chart = Chart::from_rgba(CanonicalView::Front, 1, 1, vec![[11, 22, 33, 255]]).unwrap();
    let model = AuthoredModel::new(bounds, vec![chart]).unwrap();
    EditorDocument::from_model(model, None)
}

fn maximum_relief_shallow_document() -> EditorDocument {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let chart = Chart::from_rgba(CanonicalView::Front, 1, 1, vec![[11, 22, 33, 251]]).unwrap();
    let model = AuthoredModel::new(bounds, vec![chart]).unwrap();
    EditorDocument::from_model(model, None)
}

fn assert_occupied_with_breathing_room(frame: &FrameBuffer) {
    let occupied = (0..frame.height())
        .flat_map(|y| (0..frame.width()).map(move |x| (x, y)))
        .filter(|&(x, y)| frame.owner_at(x, y).is_some())
        .collect::<Vec<_>>();
    assert!(!occupied.is_empty(), "relief geometry remains rendered");
    assert!(
        occupied
            .iter()
            .all(|&(x, y)| x >= 2 && y >= 2 && x + 2 < frame.width() && y + 2 < frame.height()),
        "relief geometry remains inside the two-pixel raster breathing room"
    );
}

fn bowl_document() -> EditorDocument {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("assets/examples/bowl.depthsprite");
    EditorDocument::from_model(load_path(path).unwrap(), None)
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
        .replace_source(Chart::from_rgba(view, width, height, rgba).unwrap())
        .unwrap();
}

fn document_with_one_undo() -> EditorDocument {
    let mut document = one_pixel_document();
    document.set_active_layer(ActiveLayer::Depth);
    document.set_current_depth(DepthValue::Relief(ReliefValue::new(4).unwrap()));
    document.begin_stroke().unwrap();
    document.pencil_pixel(CanonicalView::Front, 0, 0).unwrap();
    document.finish_stroke().unwrap();
    document
}

fn assert_document_unchanged_by_drag(
    document: &mut EditorDocument,
    content: AuthoredModel,
    dirty: bool,
    revision: u64,
) {
    assert_eq!(document.to_model(), content);
    assert_eq!(document.is_dirty(), dirty);
    assert_eq!(document.revision(), revision);
    assert!(document.undo(), "the one authored undo entry remains");
    assert!(!document.undo(), "camera drag added no undo entry");
}

#[test]
fn horizontal_drag_changes_yaw_without_document_mutation() {
    let mut document = document_with_one_undo();
    let content = document.to_model();
    let dirty = document.is_dirty();
    let revision = document.revision();
    let mut camera = OrbitCamera::default();
    let initial_camera = camera;
    let initial_target = camera.target_view();
    camera.drag(400.0, 0.0);

    assert_ne!(camera, initial_camera);
    assert_ne!(camera.target_view(), initial_target);
    assert_document_unchanged_by_drag(&mut document, content, dirty, revision);
}

#[test]
fn vertical_drag_changes_pitch_without_document_mutation() {
    let mut document = document_with_one_undo();
    let content = document.to_model();
    let dirty = document.is_dirty();
    let revision = document.revision();
    let mut camera = OrbitCamera::default();
    let initial_camera = camera;
    let initial_target = camera.target_view();
    camera.drag(0.0, -400.0);

    assert_ne!(camera, initial_camera);
    assert_ne!(camera.target_view(), initial_target);
    assert_document_unchanged_by_drag(&mut document, content, dirty, revision);
}

#[test]
fn reset_restores_the_default_view() {
    let expected = OrbitCamera::default();
    let mut camera = expected;
    camera.drag(-81.5, 39.25);
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

    assert_eq!(camera, expected);
}

#[test]
fn extreme_finite_orbit_input_remains_bounded_and_deterministic() {
    let mut first = OrbitCamera::default();
    first.drag(f32::MAX, f32::MAX);
    let mut second = OrbitCamera::default();
    second.drag(f32::MAX, f32::MAX);
    assert_eq!(first, second);
    assert_eq!(first.target_view(), second.target_view());
    let document = one_pixel_document();
    let mut preview = PreviewCache::default();
    let frame = preview.frame(&document, first).unwrap();
    let frame = frame.framebuffer();
    assert_eq!((frame.width(), frame.height()), (6, 6));

    let pitch_maximum = first;
    first.drag(0.0, 1.0);
    assert_eq!(first, pitch_maximum);
    first.drag(0.0, f32::MIN);
    let pitch_minimum = first;
    first.drag(0.0, -1.0);
    assert_eq!(first, pitch_minimum);
}

#[test]
fn an_unchanged_preview_key_returns_the_same_framebuffer() {
    let document = one_pixel_document();
    let camera = OrbitCamera::default();
    let mut preview = PreviewCache::default();

    let first = preview
        .frame(&document, camera)
        .unwrap()
        .framebuffer()
        .clone();
    let second = preview
        .frame(&document, camera)
        .unwrap()
        .framebuffer()
        .clone();

    assert_eq!(first, second);
}

#[test]
fn preview_generation_advances_once_per_successful_render() {
    let mut document = one_pixel_document();
    let camera = OrbitCamera::default();
    let mut preview = PreviewCache::default();

    let initial_generation = preview.frame(&document, camera).unwrap().generation();
    assert_eq!(initial_generation, 1);
    assert_eq!(preview.generation(), initial_generation);
    assert_eq!(
        preview.frame(&document, camera).unwrap().generation(),
        initial_generation
    );

    for rgb in [[91, 82, 73], [64, 55, 46], [37, 28, 19]] {
        recolor(&mut document, CanonicalView::Front, rgb);
    }
    let edited_generation = preview.frame(&document, camera).unwrap().generation();
    assert_eq!(edited_generation, initial_generation + 1);
    assert_eq!(preview.generation(), edited_generation);
    assert_eq!(
        preview.frame(&document, camera).unwrap().generation(),
        edited_generation
    );
}

#[test]
fn several_document_mutations_update_then_stabilize_the_framebuffer() {
    let mut document = one_pixel_document();
    let camera = OrbitCamera::default();
    let mut preview = PreviewCache::default();
    let initial_generation = preview.frame(&document, camera).unwrap().generation();

    for rgb in [[40, 50, 60], [70, 80, 90], [100, 110, 120]] {
        recolor(&mut document, CanonicalView::Front, rgb);
    }
    let changed_generation = preview.frame(&document, camera).unwrap().generation();
    let repeated_generation = preview.frame(&document, camera).unwrap().generation();

    assert_eq!(document.revision(), 3);
    assert_eq!(changed_generation, initial_generation + 1);
    assert_eq!(repeated_generation, changed_generation);
}

#[test]
fn both_bowl_sources_update_preview_and_reopen_with_a_visible_recessed_basin() {
    let mut document = bowl_document();
    let camera = OrbitCamera::default();
    let mut preview = PreviewCache::default();
    let original = preview
        .frame(&document, camera)
        .unwrap()
        .framebuffer()
        .clone();

    recolor(&mut document, CanonicalView::Front, [19, 211, 83]);
    let front_edited = preview
        .frame(&document, camera)
        .unwrap()
        .framebuffer()
        .clone();
    assert_ne!(front_edited, original);

    recolor(&mut document, CanonicalView::Top, [67, 101, 239]);
    let both_edited = preview
        .frame(&document, camera)
        .unwrap()
        .framebuffer()
        .clone();
    assert_ne!(both_edited, front_edited);

    let saved_model = document.to_model();
    let mut package = Cursor::new(Vec::new());
    save_writer(&saved_model, &mut package).unwrap();
    package.set_position(0);
    let reopened_model = load_reader(package).unwrap();
    assert_eq!(reopened_model, saved_model);
    let reopened = EditorDocument::from_model(reopened_model, None);
    let mut reopened_preview = PreviewCache::default();
    let reopened_frame = reopened_preview.frame(&reopened, camera).unwrap();
    let reopened_frame = reopened_frame.framebuffer();
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

#[test]
fn native_preview_cell_depends_only_on_registered_bounds_and_legal_relief() {
    let document = bowl_document();
    let mut preview = PreviewCache::default();
    let mut camera = OrbitCamera::default();

    let first = preview.frame(&document, camera).unwrap();
    assert_eq!(
        (first.framebuffer().width(), first.framebuffer().height()),
        (51, 51)
    );
    assert_occupied_with_breathing_room(first.framebuffer());
    let generation = first.generation();

    camera.drag(24.0, -12.0);
    let orbited = preview.frame(&document, camera).unwrap();
    assert_eq!(
        (
            orbited.framebuffer().width(),
            orbited.framebuffer().height()
        ),
        (51, 51)
    );
    assert_occupied_with_breathing_room(orbited.framebuffer());
    assert_eq!(orbited.generation(), generation + 1);
}

#[test]
fn shallow_bounds_contain_maximum_legal_relief_with_frame_breathing_room() {
    let document = maximum_relief_shallow_document();
    let mut preview = PreviewCache::default();
    let mut camera = OrbitCamera::default();
    camera.drag(-180.0, -141.056);
    let preview_frame = preview.frame(&document, camera).unwrap();
    let frame = preview_frame.framebuffer();

    assert_eq!((frame.width(), frame.height()), (6, 6));
    assert_occupied_with_breathing_room(frame);
}

#[test]
fn replacing_a_document_with_equal_revision_cannot_reuse_its_preview() {
    let first = one_pixel_document();
    let mut edited = one_pixel_document();
    recolor(&mut edited, CanonicalView::Front, [201, 17, 83]);
    let second = EditorDocument::from_model(edited.to_model(), None);
    assert_eq!(first.revision(), second.revision());

    let camera = OrbitCamera::default();
    let mut preview = PreviewCache::default();
    preview.frame(&first, camera).unwrap();
    let generation = preview.generation();
    preview.frame(&second, camera).unwrap();

    assert_eq!(preview.generation(), generation + 1);
}
