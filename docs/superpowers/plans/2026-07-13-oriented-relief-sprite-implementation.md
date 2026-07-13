# Oriented Relief Sprite Renderer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a native Rust application that opens one `.depthsprite` archive, directly warps its oriented color-plus-relief PNG charts into stable pseudo-3D views, and exports deterministic directional PNG sprite sheets without reconstructing an authoritative 3D object.

**Architecture:** A pure `relief-core` crate owns alpha semantics, canonical charts, normalized tent interpolation, and direct image-warp math. `depthsprite-format` owns safe deterministic ZIP/PNG persistence, `relief-render` owns the one authoritative CPU compositor and sheet exporter, and `desktop-app` presents that framebuffer through `eframe`; no crate persists or exposes meshes, volumes, occupancy, or inferred hidden surfaces.

**Tech Stack:** Rust 1.92.0, edition 2024, `num-rational 0.4.2`, `serde 1.0.228`, `serde_json 1.0.150`, `thiserror 2.0.18`, `png 0.18.1`, `zip 8.6.0` with Deflate only, `tempfile 3.27.0`, `eframe 0.35.0` with `wgpu`, and `rfd 0.17.2` with XDG Portal support.

## Global Constraints

- Source alpha `0` is background and contributes no chart sample.
- Source alpha `1..=255` is relief data with exact eighth-pixel value `255 - alpha`; no package field may override this conversion.
- Source RGB is immutable, opaque chart color; target alpha is `255` for a winning fragment and `0` for uncovered output.
- Adjacent samples interpolate only within the same four-connected foreground component using the normalized tent equation in the approved specification.
- Charts may overlap for compositing but are never joined, fused, closed, extruded, or completed.
- Camera movement may alter true visibility but never source color ownership or use angle-weighted texture selection.
- Unsupported disocclusion remains transparent.
- Authoritative directional export uses fixed-point/rational math and the CPU reference renderer; GPU code may only present its framebuffer.
- A model opens and saves as one `.depthsprite` file; runtime model loading has no loose multi-file alternative.
- Target platforms are Linux, Windows, and macOS; web, painting, animation, and mesh export are outside scope.
- Every task uses test-driven development and ends with a focused commit.

---

## File Structure

```text
Cargo.toml                              Workspace members and pinned shared dependencies
Cargo.lock                              Generated dependency lockfile
.gitignore                              Rust/editor/runtime ignores
README.md                               Build, run, format, and model-format overview
crates/relief-core/
  Cargo.toml                            Pure mathematical-core dependencies
  src/lib.rs                            Public exports only
  src/alpha.rs                          Alpha/background/relief conversion
  src/chart.rs                          Bounds, canonical views, validated source charts
  src/component.rs                      Four-connected foreground labeling
  src/rational.rs                       Canonical rational helpers and rounding
  src/relief.rs                         Normalized tent interpolation
  src/warp.rs                           Direct chart-to-target warp and transient depth
  tests/alpha_and_chart.rs              Format-semantic unit tests
  tests/relief_interpolation.rs         Continuity and boundary tests
  tests/warp_properties.rs              Flat, constant-relief, ownership properties
crates/depthsprite-format/
  Cargo.toml                            ZIP, PNG, JSON, and error dependencies
  src/lib.rs                            Public package API
  src/error.rs                          Actionable package errors
  src/manifest.rs                       Version-1 manifest schema and validation
  src/load.rs                           Bounded canonical package reader
  src/save.rs                           Deterministic atomic package writer
  tests/package_roundtrip.rs            Canonical round-trip tests
  tests/package_rejection.rs            Malicious and malformed archive tests
crates/relief-render/
  Cargo.toml                            Core/image dependencies
  src/lib.rs                            Public rendering/export API
  src/framebuffer.rs                    RGBA framebuffer and ownership/depth buffers
  src/diagnostic.rs                     Stable overlap, fold, conflict, and coverage warnings
  src/raster.rs                         Fixed-subdivision direct-warp rasterizer
  src/presets.rs                        Versioned rational camera presets
  src/sheet.rs                          Directional frame layout and packing
  src/png.rs                            Canonical transparent PNG encoder
  tests/compositing.rs                  Occlusion, tie, holes, and color stability
  tests/export_determinism.rs           Repeated frame/sheet byte equality
  tests/bowl_acceptance.rs              Two-chart bowl behavioral proof
crates/fixture-gen/
  Cargo.toml                            Fixture generator dependencies
  src/main.rs                           Deterministic bowl and block package generator
assets/examples/
  bowl.depthsprite                      Generated two-chart concavity proof
  block.depthsprite                     Generated ordinary-sprite proof
crates/desktop-app/
  Cargo.toml                            eframe/rfd/application dependencies
  src/lib.rs                            Testable document and application service exports
  src/main.rs                           Native entry point
  src/app.rs                            Top-level egui application and commands
  src/document.rs                       Validated document lifecycle and dirty state
  src/jobs.rs                           Generation-tagged background rendering
  src/viewport.rs                       Orbit controls and framebuffer texture upload
  src/export_ui.rs                      Directional export request UI
  tests/document_workflow.rs            Open/save/export service tests
tests/e2e_cli.rs                        Headless open-render-save-reopen-export flow
.github/workflows/ci.yml                Linux checks plus Windows/macOS compile checks
```

---

### Task 1: Workspace, Alpha Standard, and Validated Charts

**Files:**
- Create: `Cargo.toml`
- Create: `.gitignore`
- Create: `crates/relief-core/Cargo.toml`
- Create: `crates/relief-core/src/lib.rs`
- Create: `crates/relief-core/src/alpha.rs`
- Create: `crates/relief-core/src/chart.rs`
- Test: `crates/relief-core/tests/alpha_and_chart.rs`

**Interfaces:**
- Consumes: No earlier product API.
- Produces: `DecodedTexel`, `decode_rgba`, `Bounds`, `CanonicalView`, `Chart`, `ChartError`.

- [ ] **Step 1: Write the failing alpha and chart tests**

```rust
use relief_core::{
    Bounds, CanonicalView, Chart, ChartError, DecodedTexel, decode_rgba,
};

#[test]
fn alpha_is_background_or_exact_eighth_pixel_relief() {
    assert_eq!(decode_rgba([9, 8, 7, 0]), DecodedTexel::Background);
    assert_eq!(
        decode_rgba([9, 8, 7, 255]),
        DecodedTexel::Relief { rgb: [9, 8, 7], eighths: 0 }
    );
    assert_eq!(
        decode_rgba([9, 8, 7, 1]),
        DecodedTexel::Relief { rgb: [9, 8, 7], eighths: 254 }
    );
}

#[test]
fn canonical_dimensions_come_only_from_integer_bounds() {
    let bounds = Bounds::new(32, 16, 24).unwrap();
    assert_eq!(CanonicalView::Front.dimensions(bounds), (32, 16));
    assert_eq!(CanonicalView::Left.dimensions(bounds), (24, 16));
    assert_eq!(CanonicalView::Top.dimensions(bounds), (32, 24));
}

#[test]
fn chart_rejects_dimensions_that_disagree_with_bounds() {
    let bounds = Bounds::new(2, 1, 3).unwrap();
    let error = Chart::from_rgba(bounds, CanonicalView::Top, 2, 2, vec![[0, 0, 0, 0]; 4])
        .unwrap_err();
    assert_eq!(error, ChartError::DimensionMismatch { expected: (2, 3), actual: (2, 2) });
}
```

- [ ] **Step 2: Run the tests and verify the missing-crate failure**

Run: `cargo test -p relief-core --test alpha_and_chart`

Expected: FAIL because the workspace and `relief-core` package do not exist.

- [ ] **Step 3: Create the workspace and minimal exact semantic model**

```toml
# Cargo.toml
[workspace]
resolver = "3"
members = [
    "crates/relief-core",
]

[workspace.package]
edition = "2024"
rust-version = "1.92"
license = "MIT OR Apache-2.0"

[workspace.dependencies]
eframe = { version = "=0.35.0", default-features = false, features = ["default_fonts", "wgpu", "wayland", "x11"] }
num-rational = "=0.4.2"
png = "=0.18.1"
rfd = { version = "=0.17.2", default-features = false, features = ["xdg-portal"] }
serde = { version = "=1.0.228", features = ["derive"] }
serde_json = "=1.0.150"
tempfile = "=3.27.0"
thiserror = "=2.0.18"
zip = { version = "=8.6.0", default-features = false, features = ["deflate"] }
```

```gitignore
/target/
*.depthsprite.tmp
*.png.tmp
.idea/
.vscode/
```

```toml
# crates/relief-core/Cargo.toml
[package]
name = "relief-core"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
num-rational.workspace = true
thiserror.workspace = true
```

```rust
// crates/relief-core/src/alpha.rs
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodedTexel {
    Background,
    Relief { rgb: [u8; 3], eighths: u8 },
}

pub fn decode_rgba([r, g, b, a]: [u8; 4]) -> DecodedTexel {
    if a == 0 {
        DecodedTexel::Background
    } else {
        DecodedTexel::Relief { rgb: [r, g, b], eighths: 255 - a }
    }
}
```

```rust
// crates/relief-core/src/chart.rs
use thiserror::Error;
use crate::{DecodedTexel, decode_rgba};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Bounds { pub width: u32, pub height: u32, pub depth: u32 }

impl Bounds {
    pub fn new(width: u32, height: u32, depth: u32) -> Result<Self, ChartError> {
        if width == 0 || height == 0 || depth == 0 {
            return Err(ChartError::ZeroBounds);
        }
        Ok(Self { width, height, depth })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CanonicalView { Front, Back, Left, Right, Top, Bottom }

impl CanonicalView {
    pub fn dimensions(self, b: Bounds) -> (u32, u32) {
        match self {
            Self::Front | Self::Back => (b.width, b.height),
            Self::Left | Self::Right => (b.depth, b.height),
            Self::Top | Self::Bottom => (b.width, b.depth),
        }
    }

    pub fn rank(self) -> u8 {
        match self { Self::Front => 0, Self::Right => 1, Self::Back => 2,
            Self::Left => 3, Self::Top => 4, Self::Bottom => 5 }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Chart { bounds: Bounds, view: CanonicalView, width: u32, height: u32, texels: Vec<DecodedTexel> }

impl Chart {
    pub fn from_rgba(bounds: Bounds, view: CanonicalView, width: u32, height: u32, rgba: Vec<[u8; 4]>) -> Result<Self, ChartError> {
        let expected = view.dimensions(bounds);
        if expected != (width, height) {
            return Err(ChartError::DimensionMismatch { expected, actual: (width, height) });
        }
        if rgba.len() != (width as usize) * (height as usize) {
            return Err(ChartError::PixelCount);
        }
        Ok(Self { bounds, view, width, height, texels: rgba.into_iter().map(decode_rgba).collect() })
    }
    pub fn bounds(&self) -> Bounds { self.bounds }
    pub fn view(&self) -> CanonicalView { self.view }
    pub fn dimensions(&self) -> (u32, u32) { (self.width, self.height) }
    pub fn texels(&self) -> &[DecodedTexel] { &self.texels }
    pub fn texel(&self, x: u32, y: u32) -> Option<DecodedTexel> {
        (x < self.width && y < self.height).then(|| self.texels[(y * self.width + x) as usize])
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ChartError {
    #[error("model bounds must be nonzero")] ZeroBounds,
    #[error("expected image dimensions {expected:?}, got {actual:?}")] DimensionMismatch { expected: (u32, u32), actual: (u32, u32) },
    #[error("RGBA pixel count does not match image dimensions")] PixelCount,
}
```

```rust
// crates/relief-core/src/lib.rs
mod alpha;
mod chart;
pub use alpha::{DecodedTexel, decode_rgba};
pub use chart::{Bounds, CanonicalView, Chart, ChartError};
```

- [ ] **Step 4: Run format, lint, and tests**

Run: `cargo fmt --all --check`

Expected: PASS.

Run: `cargo clippy -p relief-core --all-targets -- -D warnings`

Expected: PASS.

Run: `cargo test -p relief-core --test alpha_and_chart`

Expected: 3 passed, 0 failed.

- [ ] **Step 5: Commit the semantic foundation**

```bash
git add Cargo.toml Cargo.lock .gitignore crates/relief-core
git commit -m "feat: define depth sprite alpha and chart semantics"
```

---

### Task 2: Foreground Components and Exact Tent Relief

**Files:**
- Create: `crates/relief-core/src/component.rs`
- Create: `crates/relief-core/src/rational.rs`
- Create: `crates/relief-core/src/relief.rs`
- Modify: `crates/relief-core/src/lib.rs`
- Test: `crates/relief-core/tests/relief_interpolation.rs`

**Interfaces:**
- Consumes: `Chart::texel`, `DecodedTexel`.
- Produces: `ComponentId`, `ComponentMap::label`, `ReliefField::new`, `ReliefField::sample` returning `Ratio<i64>`.

- [ ] **Step 1: Write failing interpolation tests**

```rust
use num_rational::Ratio;
use relief_core::{Bounds, CanonicalView, Chart, ReliefField};

fn alpha(depth_eighths: u8) -> u8 { 255 - depth_eighths }

#[test]
fn tent_field_is_exact_at_texel_centers_and_interpolates_between_them() {
    let chart = Chart::from_rgba(
        Bounds::new(2, 1, 1).unwrap(), CanonicalView::Front, 2, 1,
        vec![[10, 0, 0, alpha(0)], [20, 0, 0, alpha(8)]],
    ).unwrap();
    let field = ReliefField::new(&chart);
    assert_eq!(field.sample(Ratio::new(1, 2), Ratio::new(1, 2)), Some(Ratio::from_integer(0)));
    assert_eq!(field.sample(Ratio::new(3, 2), Ratio::new(1, 2)), Some(Ratio::from_integer(8)));
    assert_eq!(field.sample(Ratio::from_integer(1), Ratio::new(1, 2)), Some(Ratio::from_integer(4)));
}

#[test]
fn alpha_zero_terminates_the_domain_and_components_do_not_mix() {
    let chart = Chart::from_rgba(
        Bounds::new(3, 1, 1).unwrap(), CanonicalView::Front, 3, 1,
        vec![[1, 0, 0, alpha(0)], [0, 0, 0, 0], [2, 0, 0, alpha(24)]],
    ).unwrap();
    let field = ReliefField::new(&chart);
    assert_eq!(field.sample(Ratio::new(1, 2), Ratio::new(1, 2)), Some(Ratio::from_integer(0)));
    assert_eq!(field.sample(Ratio::new(3, 2), Ratio::new(1, 2)), None);
    assert_eq!(field.sample(Ratio::new(5, 2), Ratio::new(1, 2)), Some(Ratio::from_integer(24)));
}
```

- [ ] **Step 2: Run the tests and verify missing `ReliefField`**

Run: `cargo test -p relief-core --test relief_interpolation`

Expected: FAIL with unresolved import `relief_core::ReliefField`.

- [ ] **Step 3: Implement deterministic component labeling and the normalized tent equation**

Implement `ComponentMap::label` with a row-major flood fill over four-neighbors. Represent absent samples with `None`; assign monotonically increasing `u32` IDs in first-seen row-major order. Implement `ReliefField::sample(x, y)` exactly as:

```rust
pub fn sample(&self, x: Ratio<i64>, y: Ratio<i64>) -> Option<Ratio<i64>> {
    let cell_x = x.to_integer();
    let cell_y = y.to_integer();
    if x < Ratio::from_integer(0) || y < Ratio::from_integer(0)
        || cell_x < 0 || cell_y < 0
        || cell_x >= i64::from(self.width) || cell_y >= i64::from(self.height)
    { return None; }
    let component = self.components.at(cell_x as u32, cell_y as u32)?;
    let mut weighted = Ratio::from_integer(0);
    let mut total = Ratio::from_integer(0);
    for sy in (cell_y - 1).max(0)..=(cell_y + 1).min(i64::from(self.height) - 1) {
        for sx in (cell_x - 1).max(0)..=(cell_x + 1).min(i64::from(self.width) - 1) {
            if self.components.at(sx as u32, sy as u32) != Some(component) { continue; }
            let center_x = Ratio::new(2 * sx + 1, 2);
            let center_y = Ratio::new(2 * sy + 1, 2);
            let wx = (Ratio::from_integer(1) - abs_ratio(x.clone() - center_x)).max(Ratio::from_integer(0));
            let wy = (Ratio::from_integer(1) - abs_ratio(y.clone() - center_y)).max(Ratio::from_integer(0));
            let weight = wx * wy;
            let relief = Ratio::from_integer(i64::from(self.relief[(sy as u32 * self.width + sx as u32) as usize]?));
            weighted += relief * weight.clone();
            total += weight;
        }
    }
    (!total.is_zero()).then(|| weighted / total)
}
```

Define `abs_ratio`, `ComponentMap::at`, and `ReliefField::new` in focused modules; `ReliefField::new` must retain RGB separately from relief and never manufacture a sample for `DecodedTexel::Background`.

- [ ] **Step 4: Run the interpolation and core suites**

Run: `cargo test -p relief-core`

Expected: 5 passed, 0 failed.

Run: `cargo clippy -p relief-core --all-targets -- -D warnings`

Expected: PASS.

- [ ] **Step 5: Commit continuous per-chart relief**

```bash
git add crates/relief-core
git commit -m "feat: interpolate continuous relief within chart masks"
```

---

### Task 3: Direct Warp, Transient Depth, and Stable Compositing

**Files:**
- Create: `crates/relief-core/src/warp.rs`
- Modify: `crates/relief-core/src/lib.rs`
- Create: `crates/relief-render/Cargo.toml`
- Modify: `Cargo.toml` to add `crates/relief-render` to `workspace.members`
- Create: `crates/relief-render/src/lib.rs`
- Create: `crates/relief-render/src/framebuffer.rs`
- Create: `crates/relief-render/src/diagnostic.rs`
- Create: `crates/relief-render/src/raster.rs`
- Create: `crates/relief-render/src/presets.rs`
- Test: `crates/relief-core/tests/warp_properties.rs`
- Test: `crates/relief-render/tests/compositing.rs`

**Interfaces:**
- Consumes: `Chart`, `ReliefField`, `CanonicalView`, rational relief samples.
- Produces: `WarpCoefficients`, `TargetView`, `RenderRequest`, `FrameBuffer`, `RenderDiagnostic`, `render_model`.

- [ ] **Step 1: Write failing warp and ownership tests**

```rust
use num_rational::Ratio;
use relief_core::{SourcePoint, WarpCoefficients};

#[test]
fn direct_warp_is_flat_transform_plus_relief_parallax() {
    let warp = WarpCoefficients::new(
        [[1, 0, 10], [0, 1, 20]], [2, -1], [0, 0, 1], 3,
    );
    let sample = warp.apply(SourcePoint::new(Ratio::from_integer(4), Ratio::from_integer(5)), Ratio::from_integer(8));
    assert_eq!(sample.screen_x, Ratio::from_integer(30));
    assert_eq!(sample.screen_y, Ratio::from_integer(17));
    assert_eq!(sample.depth, Ratio::from_integer(25));
}
```

```rust
use relief_core::{Bounds, CanonicalView, Chart};
use relief_render::{RenderRequest, TargetView, render_model};

#[test]
fn exact_overlap_uses_permanent_chart_rank_and_keeps_source_color() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let front = Chart::from_rgba(bounds, CanonicalView::Front, 1, 1, vec![[255, 0, 0, 255]]).unwrap();
    let right = Chart::from_rgba(bounds, CanonicalView::Right, 1, 1, vec![[0, 255, 0, 255]]).unwrap();
    let request = RenderRequest::new(3, 3, TargetView::front_for_test());
    let frame = render_model(&[right, front], &request).unwrap();
    assert_eq!(frame.rgba_at(1, 1), [255, 0, 0, 255]);
}

#[test]
fn uncovered_output_remains_transparent() {
    let frame = render_model(&[], &RenderRequest::new(2, 2, TargetView::front_for_test())).unwrap();
    assert_eq!(frame.rgba_at(0, 0), [0, 0, 0, 0]);
}

#[test]
fn a_chart_is_not_rendered_from_its_unsupported_back_side() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let front = Chart::from_rgba(bounds, CanonicalView::Front, 1, 1, vec![[5, 6, 7, 255]]).unwrap();
    let frame = render_model(&[front], &RenderRequest::new(3, 3, TargetView::back_of_front_for_test())).unwrap();
    assert!(frame.pixels().iter().all(|pixel| *pixel == [0, 0, 0, 0]));
}

#[test]
fn equal_depth_color_disagreement_is_stable_and_diagnostic() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let front = Chart::from_rgba(bounds, CanonicalView::Front, 1, 1, vec![[255, 0, 0, 255]]).unwrap();
    let right = Chart::from_rgba(bounds, CanonicalView::Right, 1, 1, vec![[0, 255, 0, 255]]).unwrap();
    let frame = render_model(&[right, front], &RenderRequest::new(3, 3, TargetView::front_for_test())).unwrap();
    assert!(frame.diagnostics().iter().any(|item| matches!(
        item, relief_render::RenderDiagnostic::EqualDepthColorConflict { .. }
    )));
}
```

- [ ] **Step 2: Verify both test targets fail**

Run: `cargo test -p relief-core --test warp_properties`

Expected: FAIL with unresolved `WarpCoefficients`.

Run: `cargo test -p relief-render --test compositing`

Expected: FAIL because `relief-render` does not exist.

- [ ] **Step 3: Implement pure direct-warp coefficients**

Add `crates/relief-render` to `workspace.members` and create its manifest:

```toml
[package]
name = "relief-render"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
num-rational.workspace = true
png.workspace = true
relief-core = { path = "../relief-core" }
thiserror.workspace = true
```

`WarpCoefficients::apply` must evaluate these equations and nothing else:

```rust
pub fn apply(&self, p: SourcePoint, relief: Ratio<i64>) -> WarpedSample {
    let one = Ratio::from_integer(1);
    let source = [p.x, p.y, one];
    let dot = |row: &[i64; 3]| {
        source.iter().zip(row).fold(Ratio::from_integer(0), |sum, (value, coefficient)| {
            sum + value.clone() * Ratio::from_integer(*coefficient)
        })
    };
    WarpedSample {
        screen_x: dot(&self.screen[0]) + relief.clone() * Ratio::from_integer(self.parallax[0]),
        screen_y: dot(&self.screen[1]) + relief.clone() * Ratio::from_integer(self.parallax[1]),
        depth: dot(&self.depth_plane) + relief * Ratio::from_integer(self.depth_relief),
    }
}
```

`TargetView` must provide canonical signed-axis transforms and versioned rational export presets. `front_for_test` is exactly the identity chart view centered in the requested framebuffer.

- [ ] **Step 4: Implement the deterministic transient rasterizer**

For each chart texel cell, subdivide its source domain into an 8×8 grid. At every microcell corner, evaluate the normalized tent relief and direct warp. Rasterize the two diagonally split warped microtriangles with a fixed top-left rule. Derive fragment RGB from the nearest source texel at the microcell center. The depth buffer entry is:

```rust
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct FragmentKey {
    pub depth: Ratio<i64>,
    pub chart_rank: u8,
    pub source_y: u32,
    pub source_x: u32,
}

pub fn commit_fragment(frame: &mut FrameBuffer, x: u32, y: u32, key: FragmentKey, rgb: [u8; 3]) {
    let index = (y * frame.width() + x) as usize;
    if frame.keys[index].as_ref().is_none_or(|current| key < *current) {
        frame.keys[index] = Some(key);
        frame.rgba[index] = [rgb[0], rgb[1], rgb[2], 255];
    }
}
```

Skip a chart when `TargetView::is_front_facing(chart.view())` is false. Do not add dilation, skirts, backfaces, color blending, inferred connectors, or persistent triangle storage. Microtriangles are transient integration primitives for the approved image warp.

Record deterministic diagnostics in the returned framebuffer:

```rust
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum RenderDiagnostic {
    ReliefBeyondOpposingPlane { view: CanonicalView, source_x: u32, source_y: u32 },
    EqualDepthColorConflict { x: u32, y: u32, first: CanonicalView, second: CanonicalView },
    WarpFold { view: CanonicalView, source_x: u32, source_y: u32 },
    HeavyChartOverlap { covered_pixels: u32, conflicting_pixels: u32 },
    InsufficientCoverage { covered_pixels: u32, total_pixels: u32 },
}
```

Emit `ReliefBeyondOpposingPlane` when a source relief exceeds the registered bound along its inward axis. Emit an equal-depth conflict before applying the stable rank tie. Emit `WarpFold` when a microtriangle reverses signed orientation relative to its source cell. Emit heavy overlap when more than 20% of covered output pixels received candidates from multiple charts. Emit insufficient coverage when fewer than 70% of the tight projected model cell is covered. Sort and deduplicate diagnostics before returning them; diagnostics never change fragment selection.

- [ ] **Step 5: Run renderer properties and commit**

Run: `cargo test -p relief-core -p relief-render`

Expected: all tests pass.

Run: `cargo clippy -p relief-core -p relief-render --all-targets -- -D warnings`

Expected: PASS.

```bash
git add crates/relief-core crates/relief-render
git commit -m "feat: render direct relief warps with stable ownership"
```

---

### Task 4: Safe Deterministic `.depthsprite` Archives

**Files:**
- Create: `crates/depthsprite-format/Cargo.toml`
- Modify: `Cargo.toml` to add `crates/depthsprite-format` to `workspace.members`
- Create: `crates/depthsprite-format/src/lib.rs`
- Create: `crates/depthsprite-format/src/error.rs`
- Create: `crates/depthsprite-format/src/manifest.rs`
- Create: `crates/depthsprite-format/src/load.rs`
- Create: `crates/depthsprite-format/src/save.rs`
- Test: `crates/depthsprite-format/tests/package_roundtrip.rs`
- Test: `crates/depthsprite-format/tests/package_rejection.rs`

**Interfaces:**
- Consumes: `Bounds`, `CanonicalView`, `Chart`.
- Produces: `DepthSpriteModel`, `ManifestV1`, `load_reader`, `load_path`, `save_writer`, `save_path_atomic`, `PackageError`.

- [ ] **Step 1: Write failing canonical round-trip and rejection tests**

```rust
use depthsprite_format::{DepthSpriteModel, load_reader, save_writer};
use relief_core::{Bounds, CanonicalView, Chart};
use std::io::Cursor;

#[test]
fn canonical_save_is_byte_identical_after_round_trip() {
    let bounds = Bounds::new(1, 1, 1).unwrap();
    let model = DepthSpriteModel::new(bounds, vec![
        Chart::from_rgba(bounds, CanonicalView::Front, 1, 1, vec![[7, 8, 9, 255]]).unwrap(),
    ]).unwrap();
    let mut first = Cursor::new(Vec::new());
    save_writer(&model, &mut first).unwrap();
    let loaded = load_reader(Cursor::new(first.get_ref())).unwrap();
    let mut second = Cursor::new(Vec::new());
    save_writer(&loaded, &mut second).unwrap();
    assert_eq!(first.into_inner(), second.into_inner());
}
```

```rust
use depthsprite_format::{PackageError, load_reader};
use std::io::{Cursor, Write};
use zip::{ZipWriter, write::SimpleFileOptions};

#[test]
fn parent_traversal_entry_is_rejected_before_png_decode() {
    let mut bytes = Cursor::new(Vec::new());
    let mut zip = ZipWriter::new(&mut bytes);
    zip.start_file("../front.png", SimpleFileOptions::default()).unwrap();
    zip.write_all(b"not a png").unwrap();
    zip.finish().unwrap();
    assert!(matches!(load_reader(Cursor::new(bytes.into_inner())), Err(PackageError::UnsafeEntry(_))));
}
```

- [ ] **Step 2: Run tests and verify the package is absent**

Run: `cargo test -p depthsprite-format`

Expected: FAIL because `depthsprite-format` does not exist.

- [ ] **Step 3: Implement schema and bounded loading**

Add `crates/depthsprite-format` to `workspace.members` and use this manifest:

```toml
[package]
name = "depthsprite-format"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
png.workspace = true
relief-core = { path = "../relief-core" }
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
zip.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

Use this exact serialized schema:

```rust
#[derive(Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestV1 {
    pub format: String,
    pub version: u32,
    pub bounds_pixels: [u32; 3],
    pub views: Vec<CanonicalViewName>,
}
```

Require `format == "depthsprite"`, `version == 1`, nonzero bounds no larger than 512 per axis, one to six unique views, no undeclared entries, at most seven ZIP entries, and at most 64 MiB total expanded data. Accept only `manifest.json` and `views/{front,back,left,right,top,bottom}.png`. Normalize decoded pixels to nonpremultiplied RGBA8 without altering alpha; reject formats that do not decode as 8-bit RGBA. Validate dimensions through `Chart::from_rgba`.

`DepthSpriteModel::new(bounds, charts)` sorts charts by canonical rank and rejects empty lists, duplicate views, and mixed bounds. It exposes `bounds() -> Bounds` and `charts() -> &[Chart]`. `CanonicalViewName` serializes exactly to lowercase `front`, `back`, `left`, `right`, `top`, or `bottom` and converts losslessly to `CanonicalView`.

- [ ] **Step 4: Implement canonical writing and atomic path replacement**

`save_writer` writes `manifest.json` first and view PNGs in canonical rank order. Use Deflate, a fixed ZIP timestamp, Unix mode `0o644`, compact JSON plus one trailing newline, fixed PNG compression/filter settings, and black RGB for alpha-zero pixels. `save_path_atomic` writes `<name>.depthsprite.tmp` beside the destination, flushes and syncs it, then replaces the destination without modifying it on any earlier error.

- [ ] **Step 5: Run archive tests and commit**

Run: `cargo test -p depthsprite-format`

Expected: canonical round trip and all malformed-archive cases pass.

Run: `cargo clippy -p depthsprite-format --all-targets -- -D warnings`

Expected: PASS.

```bash
git add crates/depthsprite-format Cargo.lock
git commit -m "feat: add safe deterministic depth sprite packages"
```

---

### Task 5: Directional Sheets and Reproducible Proof Models

**Files:**
- Create: `crates/relief-render/src/sheet.rs`
- Create: `crates/relief-render/src/png.rs`
- Modify: `crates/relief-render/src/lib.rs`
- Create: `crates/fixture-gen/Cargo.toml`
- Modify: `Cargo.toml` to add `crates/fixture-gen` to `workspace.members`
- Create: `crates/fixture-gen/src/main.rs`
- Generate: `assets/examples/bowl.depthsprite`
- Generate: `assets/examples/block.depthsprite`
- Test: `crates/relief-render/tests/export_determinism.rs`
- Test: `crates/relief-render/tests/bowl_acceptance.rs`

**Interfaces:**
- Consumes: `Bounds`, `&[Chart]`, `render_model`, rational `TargetView` presets.
- Produces: `SheetRequest`, `DirectionCount`, `render_sheet`, `encode_png`, generated fixture files.

- [ ] **Step 1: Write failing sheet and bowl tests**

```rust
use depthsprite_format::load_path;
use relief_render::{DirectionCount, SheetRequest, encode_png, render_sheet};

#[test]
fn repeated_sheet_exports_are_byte_identical() {
    let model = load_path("assets/examples/block.depthsprite").unwrap();
    let request = SheetRequest::new(DirectionCount::Eight, 2, 1, 1);
    let first = encode_png(&render_sheet(model.charts(), model.bounds(), &request).unwrap()).unwrap();
    let second = encode_png(&render_sheet(model.charts(), model.bounds(), &request).unwrap()).unwrap();
    assert_eq!(first, second);
}
```

```rust
use depthsprite_format::load_path;
use relief_render::{RenderRequest, TargetView, render_model};

#[test]
fn bowl_has_recessed_center_visible_behind_near_rim() {
    let model = load_path("assets/examples/bowl.depthsprite").unwrap();
    let frame = render_model(model.charts(), &RenderRequest::new(96, 96, TargetView::bowl_acceptance())).unwrap();
    let rim = frame.owner_at(48, 38).expect("near rim");
    let basin = frame.owner_at(48, 48).expect("recessed basin");
    assert_eq!(rim.view, relief_core::CanonicalView::Front);
    assert_eq!(basin.view, relief_core::CanonicalView::Top);
    assert_ne!(frame.rgba_at(48, 48), [0, 0, 0, 0]);
    assert_eq!(frame.rgba_at(0, 0), [0, 0, 0, 0]);
}
```

- [ ] **Step 2: Run tests and verify missing assets/APIs**

Run: `cargo test -p relief-render --test export_determinism --test bowl_acceptance`

Expected: FAIL because `SheetRequest`, encoders, and fixtures do not exist.

- [ ] **Step 3: Implement fixed directional sheet layout**

Add this development dependency to `crates/relief-render/Cargo.toml` so renderer acceptance tests can open canonical packages:

```toml
[dev-dependencies]
depthsprite-format = { path = "../depthsprite-format" }
```

`DirectionCount` has only `Eight` and `Sixteen`. `SheetRequest::new(count, integer_scale, padding, elevation_index)` rejects zero scale and unsupported elevation. Render directions clockwise from south/front using version-1 rational camera tables. Pack frames in one horizontal row, preserving a fixed anchor and cell size derived from the model bounds plus maximum relief. Padding is transparent.

`encode_png` must emit RGBA8 with fixed compression, adaptive filtering disabled, no timestamps, and no ancillary text chunks.

- [ ] **Step 4: Implement and run the deterministic fixture generator**

Add `crates/fixture-gen` to `workspace.members` and create:

```toml
[package]
name = "fixture-gen"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
depthsprite-format = { path = "../depthsprite-format" }
relief-core = { path = "../relief-core" }
```

The bowl generator uses a 32×16×32 registration. Its top view is a radius-14 circle with zero-relief rim and a radial basin reaching 64 eighth-pixels at the center. Its front view is a 32×16 bowl profile whose visible exterior color differs from the top interior and whose relief is symmetric about the center. The block fixture uses flat alpha 255 on front, right, and top charts. Both write through `save_path_atomic`.

Run: `cargo run -p fixture-gen -- assets/examples`

Expected: creates `assets/examples/bowl.depthsprite` and `assets/examples/block.depthsprite`.

- [ ] **Step 5: Run acceptance tests, regenerate twice, and commit**

Run: `cargo test -p relief-render`

Expected: all compositing, determinism, and bowl tests pass.

Run the generator twice and compare hashes:

```bash
sha256sum assets/examples/bowl.depthsprite assets/examples/block.depthsprite
```

Expected: the second generation reports the same two hashes as the first.

```bash
git add crates/relief-render crates/fixture-gen assets/examples Cargo.lock
git commit -m "feat: export deterministic sheets and prove bowl relief"
```

---

### Task 6: Native Document, Viewport, and Export Workflow

**Files:**
- Create: `crates/desktop-app/Cargo.toml`
- Modify: `Cargo.toml` to add `crates/desktop-app` to `workspace.members`
- Create: `crates/desktop-app/src/lib.rs`
- Create: `crates/desktop-app/src/main.rs`
- Create: `crates/desktop-app/src/app.rs`
- Create: `crates/desktop-app/src/document.rs`
- Create: `crates/desktop-app/src/jobs.rs`
- Create: `crates/desktop-app/src/viewport.rs`
- Create: `crates/desktop-app/src/export_ui.rs`
- Test: `crates/desktop-app/tests/document_workflow.rs`

**Interfaces:**
- Consumes: `load_path`, `save_path_atomic`, `render_model`, `render_sheet`, `encode_png`.
- Produces: native `DepthSpriteApp`, `Document`, `RenderWorker`, file-dialog commands, viewport controls.

- [ ] **Step 1: Write failing headless document workflow tests**

```rust
use desktop_app::document::Document;
use relief_render::{DirectionCount, SheetRequest};
use tempfile::tempdir;

#[test]
fn failed_open_keeps_the_current_document() {
    let mut document = Document::open("assets/examples/block.depthsprite").unwrap();
    let original = document.model_hash();
    assert!(document.replace_from_path("assets/examples/missing.depthsprite").is_err());
    assert_eq!(document.model_hash(), original);
}

#[test]
fn save_reopen_and_export_preserve_authority() {
    let temp = tempdir().unwrap();
    let saved = temp.path().join("copy.depthsprite");
    let sheet = temp.path().join("sheet.png");
    let document = Document::open("assets/examples/bowl.depthsprite").unwrap();
    document.save_as(&saved).unwrap();
    let reopened = Document::open(&saved).unwrap();
    assert_eq!(document.model_hash(), reopened.model_hash());
    reopened.export_sheet(&sheet, &SheetRequest::new(DirectionCount::Eight, 2, 1, 1)).unwrap();
    assert!(sheet.metadata().unwrap().len() > 0);
}
```

- [ ] **Step 2: Run tests and verify the desktop package is absent**

Run: `cargo test -p desktop-app --test document_workflow`

Expected: FAIL because `desktop-app` does not exist.

- [ ] **Step 3: Implement document services and generation-tagged jobs**

Add `crates/desktop-app` to `workspace.members` and create:

```toml
[package]
name = "desktop-app"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
depthsprite-format = { path = "../depthsprite-format" }
eframe.workspace = true
relief-core = { path = "../relief-core" }
relief-render = { path = "../relief-render" }
rfd.workspace = true
thiserror.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

`src/lib.rs` publicly exposes `document` and keeps UI modules private to the binary.

`Document::open` fully validates before construction. `replace_from_path` loads into a temporary value and swaps only on success. `model_hash` hashes canonical `save_writer` bytes. `save_as` calls atomic package writing. `export_sheet` renders then atomically writes canonical PNG bytes.

Use this stale-result guard:

```rust
pub struct RenderResult { pub generation: u64, pub frame: relief_render::FrameBuffer }

pub fn install_if_current(current: u64, result: RenderResult, slot: &mut Option<relief_render::FrameBuffer>) -> bool {
    if result.generation != current { return false; }
    *slot = Some(result.frame);
    true
}
```

- [ ] **Step 4: Implement the native eframe shell**

`main.rs` calls `eframe::run_native` with the wgpu renderer. The app initially opens `assets/examples/bowl.depthsprite` when available, otherwise shows an empty-state message. File commands use a single `.depthsprite` filter through `rfd::FileDialog`; export uses a single PNG save dialog.

The viewport uploads the CPU framebuffer through `egui::Context::load_texture` with nearest filtering. Primary drag adjusts yaw and pitch, wheel adjusts integer zoom, and buttons select front/top/side/isometric presets. Camera input increments a generation ID and queues a new CPU render; stale results are discarded. The UI displays included chart names, bounds, warnings, current view, and transparent unsupported regions over a checkerboard.

The export panel exposes exactly 8/16 directions, integer scale, padding, and version-1 elevation preset. It never exposes depth scale, chart transforms, image slots, or mesh controls.

- [ ] **Step 5: Run document tests and a native smoke launch**

Run: `cargo test -p desktop-app`

Expected: document workflow tests pass.

Run: `cargo run -p desktop-app -- assets/examples/bowl.depthsprite`

Expected: native window opens the bowl, orbit changes parallax without color-source flicker, fixed isometric view matches the reference framebuffer, and export produces one PNG sheet.

- [ ] **Step 6: Commit the desktop workflow**

```bash
git add crates/desktop-app Cargo.lock
git commit -m "feat: add native depth sprite inspection workflow"
```

---

### Task 7: End-to-End Validation, Documentation, and Cross-Platform Checks

**Files:**
- Create: `tests/e2e_cli.rs`
- Create: `README.md`
- Create: `.github/workflows/ci.yml`
- Modify: `docs/superpowers/specs/2026-07-13-oriented-relief-sprite-design.md` only if implementation evidence requires a factual clarification; do not weaken approved behavior.

**Interfaces:**
- Consumes: all public package, renderer, fixture, and document APIs.
- Produces: authoritative end-to-end receipts and user-facing build/use documentation.

- [ ] **Step 1: Add the failing end-to-end test**

```rust
#[test]
fn bowl_open_render_save_reopen_export_is_reproducible() {
    let temp = tempfile::tempdir().unwrap();
    let model_path = temp.path().join("bowl-copy.depthsprite");
    let first_sheet = temp.path().join("first.png");
    let second_sheet = temp.path().join("second.png");
    let document = desktop_app::document::Document::open("assets/examples/bowl.depthsprite").unwrap();
    document.save_as(&model_path).unwrap();
    let reopened = desktop_app::document::Document::open(&model_path).unwrap();
    let request = relief_render::SheetRequest::new(relief_render::DirectionCount::Sixteen, 3, 2, 1);
    document.export_sheet(&first_sheet, &request).unwrap();
    reopened.export_sheet(&second_sheet, &request).unwrap();
    assert_eq!(std::fs::read(first_sheet).unwrap(), std::fs::read(second_sheet).unwrap());
    assert_eq!(document.model_hash(), reopened.model_hash());
}
```

- [ ] **Step 2: Run the end-to-end test before adding root dev dependencies**

Run: `cargo test --test e2e_cli`

Expected: FAIL because the root integration-test target and dev dependencies are not configured.

- [ ] **Step 3: Wire the root integration test and write README instructions**

Add this root package to the workspace manifest so `tests/e2e_cli.rs` is an integration-test target:

```toml
[package]
name = "sprite-models"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish = false

[dev-dependencies]
desktop-app = { path = "crates/desktop-app" }
relief-render = { path = "crates/relief-render" }
tempfile.workspace = true
```

Then document:

- `cargo run -p fixture-gen -- assets/examples`
- `cargo run -p desktop-app -- assets/examples/bowl.depthsprite`
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `.depthsprite` archive layout and exact alpha formula
- supported platform prerequisites for eframe/wgpu and XDG Portal
- the honest limitation that unsupported viewing regions remain transparent

- [ ] **Step 4: Add CI with authoritative Linux checks and cross-platform compilation**

Create jobs for Ubuntu, Windows, and macOS. Ubuntu runs formatting, clippy, fixture regeneration with clean-tree verification, all tests, and release build. Windows and macOS run `cargo test --workspace --no-run` and `cargo build --workspace --release`. Pin the stable toolchain to `1.92.0`.

- [ ] **Step 5: Run the complete local verification sequence**

Run: `cargo run -p fixture-gen -- assets/examples`

Expected: fixtures regenerate successfully and `git diff --exit-code -- assets/examples` passes.

Run: `cargo fmt --all --check`

Expected: PASS.

Run: `cargo clippy --workspace --all-targets -- -D warnings`

Expected: PASS with zero warnings.

Run: `cargo test --workspace`

Expected: all unit, integration, archive, rendering, bowl, document, and end-to-end tests pass.

Run: `cargo build --workspace --release`

Expected: PASS.

Run: `git grep -n -E "voxel|occupancy|marching.cubes|signed.distance|mesh" -- ':!docs/**' ':!README.md'`

Expected: no production representation implementing a prohibited geometry authority; investigate any dependency-name or diagnostic-text match rather than accepting the grep alone.

- [ ] **Step 6: Append foundation conformance evidence and commit**

Append `<conformance>` to `/tmp/sprite-models-foundation-20260713-01.md` recording implemented owners, the absence of displaced models, migrated surfaces, commands run, bowl evidence, and proof that packages contain only manifest plus PNG charts. Do not alter earlier foundation sections.

```bash
git add Cargo.toml Cargo.lock README.md .github tests docs assets crates
git commit -m "test: validate relief sprite application end to end"
```

---

## Final Acceptance

Implementation is complete only when all seven task commits exist and the final verification sequence passes from a clean checkout. The delivered application must open `assets/examples/bowl.depthsprite` as one file, show a continuous recessed basin and rounded exterior over its supported orbit sector, keep source colors attached without angle-selected flicker, save and reopen byte-stable model authority, and export byte-identical directional sheets. Unsupported regions must remain transparent, and neither saved data nor runtime authority may contain a reconstructed solid, volume, mesh, or hidden-surface completion.
