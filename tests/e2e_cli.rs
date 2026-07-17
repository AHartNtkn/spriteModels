use std::path::PathBuf;

use depthsprite_format::{load_path, save_path_atomic};
use relief_core::{Bounds, CanonicalView, DecodedTexel};
use relief_render::{PreparedModel, RenderRequest, TargetView, render_model};

fn bowl_asset() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/examples/bowl.depthsprite")
}

#[test]
fn bowl_open_render_save_reopen_preserves_model_and_relief() {
    let model = load_path(bowl_asset()).unwrap();
    assert_eq!(model.bounds(), Bounds::new(32, 12, 32).unwrap());
    assert_eq!(
        model
            .charts()
            .iter()
            .map(|chart| chart.view())
            .collect::<Vec<_>>(),
        vec![CanonicalView::Front, CanonicalView::Top]
    );
    assert!(
        model
            .chart(CanonicalView::Front)
            .unwrap()
            .supplies_opposite()
    );
    assert!(
        model
            .chart(CanonicalView::Front)
            .unwrap()
            .mirrors_opposite()
    );
    assert!(!model.chart(CanonicalView::Top).unwrap().supplies_opposite());
    assert!(!model.chart(CanonicalView::Top).unwrap().mirrors_opposite());

    let request = RenderRequest::new(96, 96, TargetView::bowl_acceptance());
    let resolved = model.resolve();
    assert!(resolved.chart(CanonicalView::Back).is_some());
    assert!(resolved.chart(CanonicalView::Bottom).is_none());
    let frame = render_model(&PreparedModel::new(&resolved), &request).unwrap();
    for view in [CanonicalView::Front, CanonicalView::Top] {
        let (x, y, owner, rgb, relief) = (0..frame.height())
            .flat_map(|y| (0..frame.width()).map(move |x| (x, y)))
            .find_map(|(x, y)| {
                let owner = frame.owner_at(x, y)?;
                if owner.view != view {
                    return None;
                }
                match model
                    .chart(view)
                    .unwrap()
                    .texel_at(owner.source_x, owner.source_y)
                {
                    Some(DecodedTexel::Relief { rgb, eighths }) if eighths > 0 => {
                        Some((x, y, owner, rgb, eighths))
                    }
                    _ => None,
                }
            })
            .unwrap_or_else(|| panic!("render must retain relieved {view:?} ownership"));
        assert!(relief > 0);
        assert_eq!(
            frame.rgba_at(x, y),
            [rgb[0], rgb[1], rgb[2], 255],
            "rendered {view:?} RGB must come from its authored PNG at ({}, {})",
            owner.source_x,
            owner.source_y
        );
    }
    assert_eq!(frame.rgba_at(0, 0), [0, 0, 0, 0]);

    let directory = tempfile::tempdir().unwrap();
    let copy = directory.path().join("bowl-copy.depthsprite");
    save_path_atomic(&model, &copy).unwrap();
    let reopened = load_path(copy).unwrap();
    assert_eq!(reopened, model);
    assert_eq!(
        render_model(&PreparedModel::new(&reopened.resolve()), &request).unwrap(),
        frame
    );
}
