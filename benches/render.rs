use criterion::{Criterion, criterion_group, criterion_main};
use editor_core::{EditorDocument, OrbitCamera, PreviewCache};
use fixture_gen::{globe_model, gyroscope_model};
use relief_core::AuthoredModel;
use relief_render::{PreparedModel, RenderRequest, TargetView, render_model};

/// The preview pipeline sizes its square framebuffer from the model alone
/// (not the orientation), so one cached frame reveals the native side length.
fn native_side(model: &AuthoredModel) -> u32 {
    let document = EditorDocument::from_model(model.clone(), None);
    let mut cache = PreviewCache::default();
    let frame = cache
        .frame(&document, OrbitCamera::default())
        .expect("fixture models must render");
    frame.framebuffer().width()
}

fn oblique_camera() -> OrbitCamera {
    let mut camera = OrbitCamera::default();
    camera.drag(37.0, -23.0);
    camera
}

fn orientations() -> Vec<(&'static str, TargetView)> {
    vec![
        ("front", TargetView::front()),
        ("default_orbit", OrbitCamera::default().target_view()),
        ("oblique", oblique_camera().target_view()),
    ]
}

fn bench_render(criterion: &mut Criterion) {
    let models: Vec<(&'static str, AuthoredModel)> = vec![
        ("globe", globe_model().expect("globe fixture must build")),
        (
            "gyroscope",
            gyroscope_model().expect("gyroscope fixture must build"),
        ),
    ];
    for (model_name, model) in &models {
        let charts = model.resolve();
        let prepared = PreparedModel::new(&charts);
        let side = native_side(model);
        for (view_name, target) in orientations() {
            criterion.bench_function(&format!("render/{model_name}/{view_name}"), |bencher| {
                bencher.iter(|| {
                    let request = RenderRequest::new(side, side, target.clone());
                    render_model(&prepared, &request).expect("render must succeed")
                })
            });
        }
    }
}

/// Preparation is camera-independent and hoisted out of every `render/*`
/// benchmark above; this isolates its own cost.
fn bench_prepare(criterion: &mut Criterion) {
    let charts = globe_model().expect("globe fixture must build").resolve();
    criterion.bench_function("prepare/globe", |bencher| {
        bencher.iter(|| PreparedModel::new(&charts))
    });
}

/// End-to-end orbit interaction through the editor's public preview path:
/// eight distinct orientations per iteration, fresh cache so every frame is a
/// real render (the cache only memoizes the most recent orientation anyway).
/// A fresh cache also means `PreparedModel` construction re-runs every
/// iteration; that's deliberate, since this benchmark measures the full
/// end-to-end orbit interaction rather than isolating render cost the way
/// `bench_render` and `bench_prepare` do.
fn bench_orbit_sweep(criterion: &mut Criterion) {
    let document =
        EditorDocument::from_model(globe_model().expect("globe fixture must build"), None);
    criterion.bench_function("orbit_sweep/globe", |bencher| {
        bencher.iter(|| {
            let mut cache = PreviewCache::default();
            let mut camera = OrbitCamera::default();
            let mut generations = 0_u64;
            for _ in 0..8 {
                camera.drag(12.0, 5.0);
                let frame = cache
                    .frame(&document, camera)
                    .expect("orbit frames must render");
                generations ^= frame.generation();
            }
            generations
        })
    });
}

fn configured() -> Criterion {
    // Twenty samples resolve the multi-x deltas this plan targets; the
    // default hundred only lengthens runs without changing decisions.
    Criterion::default().sample_size(20)
}

criterion_group! {
    name = benches;
    config = configured();
    targets = bench_render, bench_prepare, bench_orbit_sweep
}
criterion_main!(benches);
