use std::path::PathBuf;

use editor_core::{ActiveLayer, DepthValue, EditorDocument, ReliefValue};
use relief_core::{CanonicalView, EMPTY_RGBA};
use relief_render::{FrameBuffer, PreparedModel, RenderRequest, TargetView, render_model};

const FRONT: CanonicalView = CanonicalView::Front;
const TOP: CanonicalView = CanonicalView::Top;
const HIDDEN_PIXEL: (u32, u32) = (0, 0);
const BASIN_PIXEL: (u32, u32) = (15, 15);
const HIDDEN_RGB: [u8; 3] = [23, 197, 241];

fn bowl_asset() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/examples/bowl.depthsprite")
}

fn source_pixel(document: &EditorDocument, view: CanonicalView, (x, y): (u32, u32)) -> [u8; 4] {
    let source = document.source(view).expect("authored bowl source");
    source.rgba_at(x, y).expect("pixel inside bowl source")
}

fn paint_color(
    document: &mut EditorDocument,
    view: CanonicalView,
    pixel: (u32, u32),
    rgb: [u8; 3],
) {
    document.set_active_layer(ActiveLayer::Color);
    document.set_current_rgb(rgb);
    document.begin_stroke().unwrap();
    assert!(document.pencil_pixel(view, pixel.0, pixel.1).unwrap());
    assert!(document.finish_stroke().unwrap());
}

fn paint_relief(document: &mut EditorDocument, view: CanonicalView, pixel: (u32, u32), relief: u8) {
    document.set_active_layer(ActiveLayer::Depth);
    document.set_current_depth(DepthValue::Relief(ReliefValue::new(relief).unwrap()));
    document.begin_stroke().unwrap();
    assert!(document.pencil_pixel(view, pixel.0, pixel.1).unwrap());
    assert!(document.finish_stroke().unwrap());
}

fn render(document: &EditorDocument) -> FrameBuffer {
    let resolved = document.model().resolve();
    let prepared = PreparedModel::new(&resolved);
    render_model(
        &prepared,
        &RenderRequest::new(96, 96, TargetView::bowl_acceptance()),
    )
    .unwrap()
}

fn rendered_source_pixel(
    frame: &FrameBuffer,
    view: CanonicalView,
    source: (u32, u32),
) -> Option<(u32, u32)> {
    (0..frame.height()).find_map(|y| {
        (0..frame.width()).find_map(|x| {
            let owner = frame.owner_at(x, y)?;
            (owner.view == view && (owner.source_x, owner.source_y) == source).then_some((x, y))
        })
    })
}

#[test]
fn complete_bowl_authoring_workflow_preserves_exact_sources_and_recessed_render() {
    let asset = bowl_asset();
    let mut document = EditorDocument::open(&asset).unwrap();
    assert_eq!(
        document.sources().len(),
        2,
        "only authored sources are stored"
    );
    assert_eq!(source_pixel(&document, TOP, HIDDEN_PIXEL), EMPTY_RGBA);
    assert!(document.source(FRONT).unwrap().supplies_opposite());
    assert!(document.source(FRONT).unwrap().mirrors_opposite());
    assert!(!document.source(TOP).unwrap().supplies_opposite());
    assert!(!document.source(TOP).unwrap().mirrors_opposite());
    let initial_basin_pixel = source_pixel(&document, TOP, BASIN_PIXEL);
    assert!(initial_basin_pixel[3] > 0);

    let before = render(&document);
    assert!((0..before.height()).any(|y| (0..before.width()).any(|x| {
        before
            .owner_at(x, y)
            .is_some_and(|owner| owner.view == FRONT)
    })));
    assert!(rendered_source_pixel(&before, TOP, BASIN_PIXEL).is_some());

    paint_color(&mut document, TOP, HIDDEN_PIXEL, HIDDEN_RGB);
    assert_eq!(
        source_pixel(&document, TOP, HIDDEN_PIXEL),
        [HIDDEN_RGB[0], HIDDEN_RGB[1], HIDDEN_RGB[2], 0]
    );
    assert_eq!(
        render(&document),
        before,
        "stored RGB remains hidden while depth is empty"
    );

    paint_relief(&mut document, TOP, HIDDEN_PIXEL, 24);
    assert_eq!(
        source_pixel(&document, TOP, HIDDEN_PIXEL),
        [HIDDEN_RGB[0], HIDDEN_RGB[1], HIDDEN_RGB[2], 231]
    );
    let with_geometry = render(&document);
    assert_ne!(with_geometry, before);
    let added_geometry = rendered_source_pixel(&with_geometry, TOP, HIDDEN_PIXEL)
        .expect("painted depth adds visible geometry");
    assert_eq!(
        with_geometry.rgba_at(added_geometry.0, added_geometry.1),
        [HIDDEN_RGB[0], HIDDEN_RGB[1], HIDDEN_RGB[2], 255]
    );

    paint_relief(&mut document, TOP, BASIN_PIXEL, 40);
    assert_eq!(
        source_pixel(&document, TOP, BASIN_PIXEL),
        [
            initial_basin_pixel[0],
            initial_basin_pixel[1],
            initial_basin_pixel[2],
            215,
        ]
    );
    let after = render(&document);
    assert_ne!(after, with_geometry, "basin relief changes the render");

    assert!(document.undo(), "undo basin relief");
    assert_eq!(
        source_pixel(&document, TOP, BASIN_PIXEL),
        initial_basin_pixel
    );
    assert!(document.undo(), "undo added geometry");
    assert_eq!(
        source_pixel(&document, TOP, HIDDEN_PIXEL),
        [HIDDEN_RGB[0], HIDDEN_RGB[1], HIDDEN_RGB[2], 0]
    );
    assert!(document.undo(), "undo hidden color edit");
    assert_eq!(source_pixel(&document, TOP, HIDDEN_PIXEL), EMPTY_RGBA);
    assert_eq!(render(&document), before);

    assert!(document.redo(), "redo hidden color edit");
    assert_eq!(
        source_pixel(&document, TOP, HIDDEN_PIXEL),
        [HIDDEN_RGB[0], HIDDEN_RGB[1], HIDDEN_RGB[2], 0]
    );
    assert!(document.redo(), "redo added geometry");
    assert_eq!(
        source_pixel(&document, TOP, HIDDEN_PIXEL),
        [HIDDEN_RGB[0], HIDDEN_RGB[1], HIDDEN_RGB[2], 231]
    );
    assert!(document.redo(), "redo basin relief");
    assert_eq!(
        source_pixel(&document, TOP, BASIN_PIXEL),
        [
            initial_basin_pixel[0],
            initial_basin_pixel[1],
            initial_basin_pixel[2],
            215,
        ]
    );
    assert_eq!(render(&document), after);

    let authored_sources = document
        .sources()
        .map(|source| {
            (
                source.view(),
                source.supplies_opposite(),
                source.mirrors_opposite(),
                source.rgba().to_vec(),
            )
        })
        .collect::<Vec<_>>();
    let directory = tempfile::tempdir().unwrap();
    let saved_path = directory.path().join("edited-bowl.depthsprite");
    document.save_as(&saved_path).unwrap();
    assert!(!document.is_dirty());

    let reopened = EditorDocument::open(&saved_path).unwrap();
    assert_eq!(reopened.path(), Some(saved_path.as_path()));
    assert!(!reopened.is_dirty());
    assert_eq!(
        reopened
            .sources()
            .map(|source| {
                (
                    source.view(),
                    source.supplies_opposite(),
                    source.mirrors_opposite(),
                    source.rgba().to_vec(),
                )
            })
            .collect::<Vec<_>>(),
        authored_sources
    );
    assert_eq!(
        source_pixel(&reopened, TOP, HIDDEN_PIXEL),
        [23, 197, 241, 231]
    );
    assert_eq!(
        source_pixel(&reopened, TOP, BASIN_PIXEL),
        [
            initial_basin_pixel[0],
            initial_basin_pixel[1],
            initial_basin_pixel[2],
            215,
        ]
    );

    let reopened_frame = render(&reopened);
    assert_eq!(reopened_frame, after, "save and reopen preserve exact RGBA");
    let basin = rendered_source_pixel(&reopened_frame, TOP, BASIN_PIXEL)
        .expect("edited recessed basin remains visible");
    assert_eq!(
        reopened_frame.rgba_at(basin.0, basin.1),
        [
            initial_basin_pixel[0],
            initial_basin_pixel[1],
            initial_basin_pixel[2],
            255,
        ]
    );
    assert!(
        (0..reopened_frame.height()).any(|y| (0..reopened_frame.width()).any(|x| {
            reopened_frame
                .owner_at(x, y)
                .is_some_and(|owner| owner.view == FRONT)
        })),
        "rounded Front exterior remains visible"
    );
    assert!(
        reopened
            .model()
            .resolve()
            .chart(CanonicalView::Bottom)
            .is_none()
    );
}
