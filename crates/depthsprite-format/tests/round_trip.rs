use std::io::Cursor;

use depthsprite_format::{load_reader, save_writer};
use relief_core::{AuthoredModel, Bounds, CanonicalView, Chart};

fn save(model: &AuthoredModel) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    save_writer(model, &mut output).unwrap();
    output.into_inner()
}

#[test]
fn hidden_rgb_survives_alpha_zero_round_trip() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let chart = Chart::from_rgba(CanonicalView::Front, 1, 1, vec![[17, 31, 47, 0]]).unwrap();
    let model = AuthoredModel::new(bounds, vec![chart]).unwrap();

    let reopened = load_reader(Cursor::new(save(&model))).unwrap();

    assert_eq!(reopened.charts()[0].rgba_at(0, 0), Some([17, 31, 47, 0]));
}
