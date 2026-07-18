//! Full-conversion benchmark on the committed GLB fixtures. The import
//! dialog re-captures during Ctrl+drag throttled to the frame rate; the
//! budget of record is ~21 ms for a six-side 63-pixel capture (spec:
//! "Rasterizer" in 2026-07-17-model-import-design.md).

use criterion::{Criterion, criterion_group, criterion_main};
use mesh_import::{ImportSettings, convert, load_scene};
use std::path::PathBuf;

fn bench_capture(c: &mut Criterion) {
    for name in ["stanford-bunny.glb", "xyzrgb_dragon.glb"] {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        assert!(path.exists(), "Missing fixture {}", path.display());
        let scene = load_scene(&path).expect("fixture loads");
        let settings = ImportSettings::default();
        c.bench_function(&format!("convert {name}"), |b| {
            b.iter(|| convert(&scene, &settings).expect("convert"));
        });
    }
}

criterion_group!(benches, bench_capture);
criterion_main!(benches);
