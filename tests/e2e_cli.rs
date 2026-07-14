use std::path::PathBuf;

use depthsprite_format::{load_path, save_path_atomic};
use relief_core::{Bounds, CanonicalView, DecodedTexel};
use relief_render::{RenderRequest, TargetView, render_model};

const FRONT_RGB: [u8; 3] = [144, 76, 52];
const TOP_RGB: [u8; 3] = [216, 156, 85];

fn bowl_asset() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/examples/bowl.depthsprite")
}

#[test]
fn bowl_open_render_save_reopen_preserves_model_and_relief() {
    let model = load_path(bowl_asset()).unwrap();
    assert_eq!(model.bounds(), Bounds::new(32, 16, 32).unwrap());
    assert_eq!(
        model
            .charts()
            .iter()
            .map(|chart| chart.view())
            .collect::<Vec<_>>(),
        vec![CanonicalView::Front, CanonicalView::Top]
    );

    let request = RenderRequest::new(96, 96, TargetView::bowl_acceptance());
    let frame = render_model(model.bounds(), model.charts(), &request).unwrap();
    let rim = frame.owner_at(48, 67).expect("rounded front rim");
    let basin = frame.owner_at(48, 48).expect("recessed top basin");

    assert_eq!(
        (rim.view, rim.source_x, rim.source_y),
        (CanonicalView::Front, 27, 2)
    );
    assert_eq!(
        (basin.view, basin.source_x, basin.source_y),
        (CanonicalView::Top, 16, 16)
    );
    assert_eq!(
        model.charts()[0].texel_at(rim.source_x, rim.source_y),
        Some(DecodedTexel::Relief {
            rgb: FRONT_RGB,
            eighths: 40,
        })
    );
    assert_eq!(
        model.charts()[1].texel_at(basin.source_x, basin.source_y),
        Some(DecodedTexel::Relief {
            rgb: TOP_RGB,
            eighths: 64,
        })
    );
    assert_eq!(frame.rgba_at(48, 67), [144, 76, 52, 255]);
    assert_eq!(frame.rgba_at(48, 48), [216, 156, 85, 255]);
    assert_eq!(frame.rgba_at(0, 0), [0, 0, 0, 0]);

    let directory = tempfile::tempdir().unwrap();
    let copy = directory.path().join("bowl-copy.depthsprite");
    save_path_atomic(&model, &copy).unwrap();
    let reopened = load_path(copy).unwrap();
    assert_eq!(reopened, model);
    assert_eq!(
        render_model(reopened.bounds(), reopened.charts(), &request).unwrap(),
        frame
    );
}
