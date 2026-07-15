use fixture_gen::{block_model, bowl_model, dome_model, globe_model, gyroscope_model, tent_model};
use relief_core::{AuthoredModel, Chart, DecodedTexel};

fn value(rgb: [u8; 3]) -> u32 {
    rgb.into_iter().map(u32::from).sum()
}

fn assert_blunt_cartoon_values(model_name: &str, chart: &Chart) {
    let values = chart
        .texels()
        .filter_map(|texel| match texel {
            DecodedTexel::Background => None,
            DecodedTexel::Relief { rgb, .. } => Some(value(rgb)),
        })
        .collect::<Vec<_>>();
    let darkest = *values.iter().min().unwrap();
    let lightest = *values.iter().max().unwrap();
    let range = lightest - darkest;
    assert!(
        range >= 150,
        "{model_name} {:?} needs an unmistakable light-to-shadow range, got {darkest}..={lightest}",
        chart.view()
    );
    let low_ceiling = darkest + range / 4;
    let high_floor = lightest - range / 4;
    let minimum_region = (values.len() / 100).max(1);
    let low = values.iter().filter(|&&value| value <= low_ceiling).count();
    let high = values.iter().filter(|&&value| value >= high_floor).count();
    assert!(
        low >= minimum_region,
        "{model_name} {:?} needs a broad shadow region, got {low}/{} pixels",
        chart.view(),
        values.len()
    );
    assert!(
        high >= minimum_region,
        "{model_name} {:?} needs a broad highlight region, got {high}/{} pixels",
        chart.view(),
        values.len()
    );
}

fn assert_model(model_name: &str, model: AuthoredModel) {
    for chart in model.charts() {
        assert_blunt_cartoon_values(model_name, chart);
    }
}

#[test]
fn every_demo_has_broad_cartoon_highlights_and_shadows_on_every_source() {
    assert_model("block", block_model().unwrap());
    assert_model("bowl", bowl_model().unwrap());
    assert_model("globe", globe_model().unwrap());
    assert_model("gyroscope", gyroscope_model().unwrap());
    assert_model("tent", tent_model().unwrap());
    assert_model("dome", dome_model().unwrap());
}
