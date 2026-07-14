use std::path::PathBuf;

use editor_core::{ActiveLayer, DepthValue, EditorDocument, ReliefValue};
use relief_core::CanonicalView;
use relief_render::{FrameBuffer, RenderRequest, TargetView, render_model};

const FRONT: CanonicalView = CanonicalView::Front;
const TOP: CanonicalView = CanonicalView::Top;
const HIDDEN_PIXEL: (u32, u32) = (15, 1);
const BASIN_PIXEL: (u32, u32) = (15, 15);
const HIDDEN_RGB: [u8; 3] = [23, 197, 241];
const TOP_RGB: [u8; 3] = [216, 156, 85];
const FRONT_RGB: [u8; 3] = [144, 76, 52];

fn bowl_asset() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/examples/bowl.depthsprite")
}

fn source_pixel(document: &EditorDocument, view: CanonicalView, (x, y): (u32, u32)) -> [u8; 4] {
    let source = document.source(view).expect("authored bowl source");
    let pixel = source.pixel(x, y).expect("pixel inside bowl source");
    let rgb = pixel.rgb();
    [rgb[0], rgb[1], rgb[2], pixel.alpha()]
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
    render_model(
        document.bounds(),
        &document.resolved_charts().unwrap(),
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
    assert_eq!(source_pixel(&document, TOP, HIDDEN_PIXEL), [0, 0, 0, 0]);
    assert_eq!(
        source_pixel(&document, TOP, BASIN_PIXEL),
        [TOP_RGB[0], TOP_RGB[1], TOP_RGB[2], 191]
    );

    let before = render(&document);
    let initial_rim = before.owner_at(48, 67).expect("rounded front rim");
    let initial_basin = before.owner_at(48, 48).expect("recessed top basin");
    assert_eq!(
        (initial_rim.view, initial_rim.source_x, initial_rim.source_y),
        (FRONT, 27, 2)
    );
    assert_eq!(
        (
            initial_basin.view,
            initial_basin.source_x,
            initial_basin.source_y,
        ),
        (TOP, 16, 16)
    );
    assert_eq!(before.rgba_at(48, 67), [144, 76, 52, 255]);
    assert_eq!(before.rgba_at(48, 48), [216, 156, 85, 255]);

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

    paint_relief(&mut document, TOP, BASIN_PIXEL, 96);
    assert_eq!(
        source_pixel(&document, TOP, BASIN_PIXEL),
        [TOP_RGB[0], TOP_RGB[1], TOP_RGB[2], 159]
    );
    let after = render(&document);
    assert_ne!(after, with_geometry, "basin relief changes the render");

    assert!(document.undo(), "undo basin relief");
    assert_eq!(
        source_pixel(&document, TOP, BASIN_PIXEL),
        [TOP_RGB[0], TOP_RGB[1], TOP_RGB[2], 191]
    );
    assert!(document.undo(), "undo added geometry");
    assert_eq!(
        source_pixel(&document, TOP, HIDDEN_PIXEL),
        [HIDDEN_RGB[0], HIDDEN_RGB[1], HIDDEN_RGB[2], 0]
    );
    assert!(document.undo(), "undo hidden color edit");
    assert_eq!(source_pixel(&document, TOP, HIDDEN_PIXEL), [0, 0, 0, 0]);
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
        [TOP_RGB[0], TOP_RGB[1], TOP_RGB[2], 159]
    );
    assert_eq!(render(&document), after);

    let authored_sources = document
        .sources()
        .map(|source| (source.view(), source.rgba().to_vec()))
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
            .map(|source| (source.view(), source.rgba().to_vec()))
            .collect::<Vec<_>>(),
        authored_sources
    );
    assert_eq!(
        source_pixel(&reopened, TOP, HIDDEN_PIXEL),
        [23, 197, 241, 231]
    );
    assert_eq!(
        source_pixel(&reopened, TOP, BASIN_PIXEL),
        [216, 156, 85, 159]
    );

    let reopened_frame = render(&reopened);
    assert_eq!(reopened_frame, after, "save and reopen preserve exact RGBA");
    let rim = rendered_source_pixel(&reopened_frame, FRONT, (27, 2))
        .expect("rounded front wall remains visible");
    let basin = rendered_source_pixel(&reopened_frame, TOP, BASIN_PIXEL)
        .expect("edited recessed basin remains visible");
    assert_eq!(
        reopened_frame.rgba_at(rim.0, rim.1),
        [FRONT_RGB[0], FRONT_RGB[1], FRONT_RGB[2], 255]
    );
    assert_eq!(
        reopened_frame.rgba_at(basin.0, basin.1),
        [TOP_RGB[0], TOP_RGB[1], TOP_RGB[2], 255]
    );
    assert!(
        basin.1 < rim.1,
        "the recessed basin renders behind the near rim"
    );
    assert_eq!(source_pixel(&reopened, FRONT, (27, 2)), [144, 76, 52, 215]);
    assert_eq!(source_pixel(&reopened, FRONT, (4, 2)), [144, 76, 52, 215]);
}
