use std::path::PathBuf;

use mesh_import::{ALL_VIEWS, ImportSettings, TriangleScene, convert, load_scene};
use relief_core::{AuthoredModel, CanonicalView};

fn fixture(name: &str) -> TriangleScene {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    assert!(
        path.exists(),
        "Missing fixture {}. Provision it per tests/fixtures/README.md.",
        path.display()
    );
    load_scene(&path).unwrap_or_else(|error| panic!("{name} must load: {error}"))
}

/// Format invariants every conversion must satisfy: chart dims match the
/// bounds, and every texel is either empty (alpha 0) or has relief
/// h = 255 - alpha within 0..=h_max.
fn assert_format_invariants(model: &AuthoredModel) {
    for chart in model.charts() {
        let view = chart.view();
        assert_eq!(chart.dimensions(), view.dimensions(model.bounds()));
        let h_max = view.maximum_inward_depth(model.bounds());
        for &texel in chart.rgba() {
            if texel[3] == 0 {
                continue;
            }
            let relief = 255 - texel[3];
            assert!(
                relief <= h_max,
                "{view:?}: relief {relief} exceeds h_max {h_max}"
            );
        }
    }
}

fn assert_full_conversion(name: &str, minimum_triangles: usize) {
    let scene = fixture(name);
    assert!(
        scene.triangles.len() >= minimum_triangles,
        "{name}: {} triangles, expected at least {minimum_triangles}",
        scene.triangles.len()
    );
    for longest in [63, 32, 7] {
        let settings = ImportSettings {
            longest_axis_pixels: longest,
            ..Default::default()
        };
        let model = convert(&scene, &settings)
            .unwrap_or_else(|error| panic!("{name} at {longest}px must convert: {error}"));
        assert_eq!(model.charts().len(), 6);
        assert_format_invariants(&model);
        // Closed meshes must be visible from every axis.
        for view in ALL_VIEWS {
            let chart = model.chart(view).expect("all six captured");
            let covered = chart.rgba().iter().filter(|texel| texel[3] != 0).count();
            assert!(
                covered > 0,
                "{name} {view:?} at {longest}px has no coverage"
            );
        }
    }
}

#[test]
fn teapot_converts_with_invariants() {
    assert_full_conversion("teapot.glb", 1_000);
}

#[test]
fn bunny_converts_with_invariants() {
    assert_full_conversion("stanford-bunny.glb", 10_000);
}

#[test]
fn dragon_converts_with_invariants() {
    assert_full_conversion("xyzrgb_dragon.glb", 10_000);
}

#[test]
fn earth_sphere_front_capture_is_a_textured_disc() {
    let scene = fixture("earth.glb");
    let model = convert(&scene, &ImportSettings::default()).expect("earth converts");
    assert_format_invariants(&model);
    let front = model.chart(CanonicalView::Front).expect("front chart");
    let (width, height) = front.dimensions();
    assert_eq!((width, height), (63, 63));

    // Center texel: the sphere touches the front face; with 0.5-texel
    // parallax on a radius-31.5px sphere the sag is r - sqrt(r^2 - 0.5^2)
    // < 0.004 px, so quantized relief at the center is at most one unit.
    let center = front.rgba()[(31 * 63 + 31) as usize];
    let center_relief = 255 - center[3];
    assert!(
        center_relief <= 1,
        "center relief {center_relief} must be ~0"
    );

    // Ownership-region bounds (same derivation as
    // capture.rs's sphere dedup test): the sphere sits inscribed in the box
    // (radius R = 31.5), so a point at polar angle theta from the Front
    // axis projects at screen radius R*sin(theta) with analytic normal
    // n = (sin(theta)cos(phi), ..., cos(theta)) at azimuth phi.
    //
    // - contains: Front strictly-or-tie-best requires cos(theta) >=
    //   max(|n_x|, |n_y|), which holds for every azimuth iff theta <= 45
    //   degrees (worst azimuth aligns one axis with the full sin(theta));
    //   ties resolve to Front by rank at exactly 45 degrees. That admits
    //   every point inside screen radius R*sin(45deg) = R/sqrt(2); shrink
    //   by 1 texel for center-vs-region discretization.
    // - contained: ownership only requires cos(theta) >= |n_x| AND
    //   cos(theta) >= |n_y| (Front need not be the unique best, just not
    //   beaten by another Capture side observing the same point), so the
    //   admitted region extends to the weakest azimuth (45 degrees, where
    //   |n_x| = |n_y|): tan(theta) <= sqrt(2), i.e. sin(theta) <=
    //   sqrt(2/3). Add 1 texel for the closure ring plus 0.5 texel for the
    //   texel-center-vs-continuous-boundary offset.
    let radius = 31.5_f64;
    let sin_45 = std::f64::consts::FRAC_1_SQRT_2; // sin(45 deg) = sqrt(2)/2
    let sin_weakest_azimuth = (2.0_f64 / 3.0).sqrt(); // tan(theta) <= sqrt(2)
    let contains_radius = (sin_45 * radius - 1.0).max(0.0);
    let contained_radius = sin_weakest_azimuth * radius + 1.5;
    let lower = std::f64::consts::PI * contains_radius * contains_radius;
    let upper = std::f64::consts::PI * contained_radius * contained_radius;
    let covered = front.rgba().iter().filter(|texel| texel[3] != 0).count() as f64;
    assert!(
        (lower..=upper).contains(&covered),
        "covered {covered} outside ownership-region bounds [{lower:.1}, {upper:.1}]"
    );

    // Texture liveness: a constant-color capture means texture sampling is
    // dead. Earth's oceans and land must differ somewhere.
    let mut colors: Vec<[u8; 3]> = front
        .rgba()
        .iter()
        .filter(|texel| texel[3] != 0)
        .map(|texel| [texel[0], texel[1], texel[2]])
        .collect();
    colors.sort();
    colors.dedup();
    assert!(
        colors.len() > 1,
        "captured earth color must vary across the surface"
    );
}
