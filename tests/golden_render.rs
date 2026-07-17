//! Golden-image lock on the renderer's exact output.
//!
//! Every optimization to the render pipeline must keep these hashes
//! bit-identical: the hash covers the RGBA image and every fragment owner,
//! including its exact rational depth. Regenerate deliberately with
//! `GOLDEN_REGEN=1 cargo test --test golden_render` and inspect the diff.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use editor_core::{EditorDocument, OrbitCamera, PreviewCache};
use fixture_gen::{
    block_model, bowl_model, dome_model, globe_model, gyroscope_model, tent_model,
};
use relief_core::AuthoredModel;
use relief_render::{FrameBuffer, PreparedModel, RenderRequest, TargetView, render_model};

const GOLDEN_PATH: &str = "tests/golden/render_hashes.txt";
const FAILURE_DIR: &str = "target/golden-failures";

/// FNV-1a, 64-bit: deterministic, dependency-free, and stable across
/// platforms, which is all a golden lock needs.
struct Fnv1a(u64);

impl Fnv1a {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

fn frame_hash(frame: &FrameBuffer) -> u64 {
    let mut hasher = Fnv1a::new();
    hasher.write(&frame.width().to_le_bytes());
    hasher.write(&frame.height().to_le_bytes());
    for pixel in frame.pixels() {
        hasher.write(pixel);
    }
    for y in 0..frame.height() {
        for x in 0..frame.width() {
            match frame.owner_at(x, y) {
                None => hasher.write(&[0]),
                Some(owner) => {
                    hasher.write(&[1]);
                    hasher.write(format!("{:?}", owner.view).as_bytes());
                    hasher.write(&owner.source_x.to_le_bytes());
                    hasher.write(&owner.source_y.to_le_bytes());
                    hasher.write(&owner.depth.numer().to_le_bytes());
                    hasher.write(&owner.depth.denom().to_le_bytes());
                }
            }
        }
    }
    hasher.0
}

fn fixture_models() -> Vec<(&'static str, AuthoredModel)> {
    vec![
        ("block", block_model().expect("block fixture must build")),
        ("bowl", bowl_model().expect("bowl fixture must build")),
        ("dome", dome_model().expect("dome fixture must build")),
        ("globe", globe_model().expect("globe fixture must build")),
        (
            "gyroscope",
            gyroscope_model().expect("gyroscope fixture must build"),
        ),
        ("tent", tent_model().expect("tent fixture must build")),
    ]
}

fn views() -> Vec<(&'static str, TargetView)> {
    let mut oblique = OrbitCamera::default();
    oblique.drag(37.0, -23.0);
    vec![
        ("front", TargetView::front()),
        ("isometric", TargetView::isometric()),
        ("default_orbit", OrbitCamera::default().target_view()),
        ("oblique", oblique.target_view()),
    ]
}

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

fn render_scenarios() -> BTreeMap<String, FrameBuffer> {
    let mut frames = BTreeMap::new();
    for (model_name, model) in fixture_models() {
        let charts = model.resolve();
        let prepared = PreparedModel::new(&charts);
        let side = native_side(&model);
        for (view_name, target) in views() {
            let request = RenderRequest::new(side, side, target);
            let frame = render_model(&prepared, &request).expect("render must succeed");
            frames.insert(format!("{model_name}/{view_name}"), frame);
        }
    }
    frames
}

fn write_failure_png(name: &str, frame: &FrameBuffer) -> PathBuf {
    let path = Path::new(FAILURE_DIR).join(format!("{}.png", name.replace('/', "-")));
    fs::create_dir_all(FAILURE_DIR).expect("failure directory must be creatable");
    let file = fs::File::create(&path).expect("failure PNG must be writable");
    let mut encoder = png::Encoder::new(file, frame.width(), frame.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("PNG header must encode");
    let flat: Vec<u8> = frame.pixels().iter().flatten().copied().collect();
    writer
        .write_image_data(&flat)
        .expect("PNG data must encode");
    path
}

fn parse_goldens(text: &str) -> BTreeMap<String, u64> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (name, hash) = line
                .rsplit_once(' ')
                .unwrap_or_else(|| panic!("malformed golden line: {line:?}"));
            let hash = u64::from_str_radix(hash, 16)
                .unwrap_or_else(|error| panic!("malformed golden hash {hash:?}: {error}"));
            (name.to_owned(), hash)
        })
        .collect()
}

#[test]
fn rendered_output_matches_goldens() {
    let frames = render_scenarios();
    let actual: BTreeMap<String, u64> = frames
        .iter()
        .map(|(name, frame)| (name.clone(), frame_hash(frame)))
        .collect();

    if std::env::var_os("GOLDEN_REGEN").is_some() {
        let mut text = String::new();
        for (name, hash) in &actual {
            text.push_str(&format!("{name} {hash:016x}\n"));
        }
        fs::create_dir_all("tests/golden").expect("golden directory must be creatable");
        fs::write(GOLDEN_PATH, &text).expect("golden file must be writable");
        for (name, frame) in &frames {
            write_failure_png(&format!("regen-{name}"), frame);
        }
        println!("regenerated {GOLDEN_PATH} with {} entries", actual.len());
    }

    let text = fs::read_to_string(GOLDEN_PATH).unwrap_or_else(|error| {
        panic!(
            "missing golden file {GOLDEN_PATH} ({error}); generate it with \
             GOLDEN_REGEN=1 cargo test --test golden_render"
        )
    });
    let expected = parse_goldens(&text);

    let mut failures = Vec::new();
    for (name, frame) in &frames {
        let actual_hash = actual[name];
        match expected.get(name) {
            Some(&expected_hash) if expected_hash == actual_hash => {}
            Some(&expected_hash) => {
                let png_path = write_failure_png(name, frame);
                failures.push(format!(
                    "{name}: expected {expected_hash:016x}, got {actual_hash:016x} \
                     (actual frame written to {})",
                    png_path.display()
                ));
            }
            None => failures.push(format!("{name}: no golden entry recorded")),
        }
    }
    for name in expected.keys() {
        if !frames.contains_key(name) {
            failures.push(format!("{name}: golden entry has no matching scenario"));
        }
    }
    assert!(
        failures.is_empty(),
        "golden mismatches:\n{}",
        failures.join("\n")
    );
}
