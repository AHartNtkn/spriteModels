# DepthSprite Model Authoring and Demonstration Suite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build one core-owned fixed-scale sprite model, explicit side and synchronized dimension editing, resolved-only inverse rendering, and a lit six-asset demonstration suite with repo-local authoring guidance.

**Architecture:** `relief-core::AuthoredModel` becomes the only durable model and owns bounds, charts, canonical frames, validation, mutation, resizing, and resolution. Package I/O converts directly to that model, `EditorDocument` wraps it with history and transient tool state, and `relief-render` accepts only `ResolvedCharts`; the desktop UI delegates all semantic operations to the document. Deterministic fixture modules generate the examples through the same package and renderer boundaries used by the application.

**Tech Stack:** Rust 1.92, edition 2024, `thiserror`, `num-rational`, `png`, `zip`, `serde`, `eframe`/`egui`, Cargo workspace tests.

## Global Constraints

- Do not create or use a Git worktree.
- The source PNGs remain the complete authored model; do not construct a mesh, voxel volume, or persistent world geometry.
- Rendering remains the output-first inverse warp `W(p) = H p + h(p)e` with transient compositor depth only.
- `RELIEF_UNITS_PER_PIXEL` remains exactly 8 for every model.
- Every model bound is in `1..=63`.
- Maximum inward depth is half the opposing model dimension, or four times that dimension in relief units.
- A chart contributes no candidates from behind or when exactly edge-on.
- Empty RGBA is exactly `[255, 0, 255, 0]`.
- Depth-canvas grayscale is absolute: for nonempty alpha `a`, `h = 255 - a` and gray is `round(255h / 254)`.
- Color is above depth; source cards use the two-column by three-row progressive grid; the model viewport remains at least three times each canvas in width and height.
- Preserve unrelated user changes and do not add compatibility aliases, adapters, re-exports, or parallel model authorities.

---

## File and responsibility map

- Create `crates/relief-core/src/model.rs`: sole `AuthoredModel`, `ResolvedCharts`, validation, and model mutations.
- Create `crates/relief-core/src/frame.rs`: canonical signed axes, opposite sides, world/image edge conversion, and bounds-dependent frames.
- Create `crates/relief-core/src/resize.rs`: transactional synchronized edge resizing and reassignment policy.
- Modify `crates/relief-core/src/chart.rs`: bounded `Bounds`, immutable chart reconstruction helpers, and empty-pixel creation.
- Modify `crates/relief-core/src/lib.rs`: export only the selected core types.
- Delete `crates/depthsprite-format` model ownership from `src/lib.rs`; `load.rs` and `save.rs` convert directly to/from `AuthoredModel`.
- Delete `crates/editor-core/src/source.rs` and `crates/editor-core/src/fallback.rs`; `document.rs` owns one `AuthoredModel`.
- Modify `crates/relief-render/src/compositor.rs` and `src/presets.rs`: accept `ResolvedCharts`, use core frames and per-chart maximum inward depth, and remove all-charts test bypasses.
- Modify `crates/desktop-app/src/source_grid.rs`: explicit add/assignment controls, compact resize popover, and confirmations.
- Preserve `crates/desktop-app/src/layout.rs`: it already implements the approved dominant model and two-by-three source layout.
- Preserve and strengthen `crates/desktop-app/src/canvas.rs`: its current absolute depth grayscale formula is already correct.
- Split `crates/fixture-gen/src/lib.rs` into orchestration plus `block.rs`, `bowl.rs`, `globe.rs`, `gyroscope.rs`, `tent.rs`, `dome.rs`, and `pixel.rs`.
- Create `.codex/skills/create-depthsprite-assets/{SKILL.md,agents/openai.yaml,references/asset-principles.md}`.

---

### Task 1: Core authored-model authority and resolution

**Files:**
- Create: `crates/relief-core/src/model.rs`
- Create: `crates/relief-core/src/frame.rs`
- Modify: `crates/relief-core/src/chart.rs`
- Modify: `crates/relief-core/src/lib.rs`
- Create: `crates/relief-core/tests/authored_model.rs`
- Modify: `crates/relief-core/tests/alpha_and_chart.rs`

**Interfaces:**
- Produces: `AuthoredModel::new(Bounds, Vec<Chart>) -> Result<AuthoredModel, ModelError>`
- Produces: `AuthoredModel::{with_empty_chart, bounds, charts, chart, add_chart, add_empty_chart, replace_chart, remove_chart, set_rgba, resolve}`
- Produces: `ResolvedCharts::{bounds, charts, chart}`
- Produces: `CanonicalView::{opposite, maximum_inward_depth, frame}`
- Produces: `Bounds::new` enforcing `1..=63`
- Produces: `EMPTY_RGBA: [u8; 4]`

- [ ] **Step 1: Write core authority tests**

Create `crates/relief-core/tests/authored_model.rs` with table-driven tests equivalent to:

```rust
use relief_core::{
    AuthoredModel, Bounds, CanonicalView, Chart, ModelError, EMPTY_RGBA,
};

fn rgba(relief: u8, rgb: [u8; 3]) -> [u8; 4] {
    [rgb[0], rgb[1], rgb[2], 255 - relief]
}

fn chart(bounds: Bounds, view: CanonicalView, pixel: [u8; 4]) -> Chart {
    let (width, height) = view.dimensions(bounds);
    Chart::from_rgba(view, width, height, vec![pixel; (width * height) as usize]).unwrap()
}

#[test]
fn bounds_are_limited_to_the_fixed_scale_encodable_range() {
    assert!(Bounds::new(1, 63, 1).is_ok());
    for dimensions in [(0, 1, 1), (64, 1, 1), (1, 64, 1), (1, 1, 64)] {
        assert!(Bounds::new(dimensions.0, dimensions.1, dimensions.2).is_err());
    }
}

#[test]
fn maximum_inward_depth_is_half_the_opposing_axis() {
    let bounds = Bounds::new(10, 12, 14).unwrap();
    assert_eq!(CanonicalView::Front.maximum_inward_depth(bounds), 56);
    assert_eq!(CanonicalView::Left.maximum_inward_depth(bounds), 40);
    assert_eq!(CanonicalView::Top.maximum_inward_depth(bounds), 48);
}

#[test]
fn model_rejects_duplicate_dimensions_and_relief_beyond_the_midpoint() {
    let bounds = Bounds::new(2, 3, 4).unwrap();
    let front = chart(bounds, CanonicalView::Front, rgba(16, [1, 2, 3]));
    assert!(AuthoredModel::new(bounds, vec![front.clone()]).is_ok());
    assert!(matches!(
        AuthoredModel::new(bounds, vec![front.clone(), front]),
        Err(ModelError::DuplicateView(CanonicalView::Front))
    ));
    let too_deep = chart(bounds, CanonicalView::Front, rgba(17, [1, 2, 3]));
    assert!(matches!(
        AuthoredModel::new(bounds, vec![too_deep]),
        Err(ModelError::ReliefBeyondMaximum { view: CanonicalView::Front, actual: 17, maximum: 16, .. })
    ));
}

#[test]
fn opposite_maxima_have_the_same_midpoint_coordinate() {
    let bounds = Bounds::new(3, 5, 7).unwrap();
    let axis = i64::from(bounds.depth());
    let inward = i64::from(CanonicalView::Front.maximum_inward_depth(bounds));
    assert_eq!(
        num_rational::Ratio::new(inward, 8),
        num_rational::Ratio::new(axis, 2),
    );
    assert_eq!(
        num_rational::Ratio::from_integer(axis) - num_rational::Ratio::new(inward, 8),
        num_rational::Ratio::new(axis, 2),
    );
}

#[test]
fn one_authored_chart_resolves_to_two_observations_and_explicit_opposite_wins() {
    let bounds = Bounds::new(2, 2, 2).unwrap();
    let front = chart(bounds, CanonicalView::Front, rgba(0, [200, 10, 20]));
    let mut model = AuthoredModel::new(bounds, vec![front]).unwrap();
    let resolved = model.resolve();
    assert_eq!(resolved.charts().len(), 2);
    assert_eq!(resolved.chart(CanonicalView::Back).unwrap().rgba()[0], [200, 10, 20, 255]);

    let back = chart(bounds, CanonicalView::Back, rgba(0, [20, 10, 200]));
    model.add_chart(back).unwrap();
    let resolved = model.resolve();
    assert_eq!(resolved.chart(CanonicalView::Front).unwrap().rgba()[0][0], 200);
    assert_eq!(resolved.chart(CanonicalView::Back).unwrap().rgba()[0][2], 200);
}

#[test]
fn empty_charts_use_the_visible_magenta_authoring_sentinel() {
    let bounds = Bounds::new(2, 1, 1).unwrap();
    let model = AuthoredModel::with_empty_chart(bounds, CanonicalView::Front).unwrap();
    assert_eq!(model.chart(CanonicalView::Front).unwrap().rgba(), &[EMPTY_RGBA; 2]);
}
```

Add table cases showing zero charts and seven charts return `ChartCount`, removal of the sole chart returns `LastChart` without mutation, removal of an explicit Back immediately restores the Front-derived Back observation, and failed `add_chart`, `replace_chart`, and `set_rgba` validations leave the entire model byte-for-byte unchanged. Update existing bounds tests so `64` is rejected and replace any `Bounds::new(1, 1, 1)` foreground relief above `4` with bounds large enough for that relief.

- [ ] **Step 2: Run the new tests and verify the authority is missing**

Run: `cargo test -p relief-core --test authored_model`

Expected: FAIL because `AuthoredModel`, `ModelError`, `EMPTY_RGBA`, and the canonical frame methods do not exist.

- [ ] **Step 3: Implement bounded charts, canonical frames, model validation, mutation, and resolution**

Add these public shapes, keeping all vectors private and sorted by `CanonicalView::rank()`:

```rust
pub const EMPTY_RGBA: [u8; 4] = [255, 0, 255, 0];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CanonicalFrame {
    pub origin: [i64; 3],
    pub source_u: [i64; 3],
    pub source_v: [i64; 3],
    pub inward: [i64; 3],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoredModel {
    bounds: Bounds,
    charts: Vec<Chart>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedCharts {
    bounds: Bounds,
    charts: Vec<Chart>,
}

#[derive(Clone, Debug, thiserror::Error, Eq, PartialEq)]
pub enum ModelError {
    #[error("model must contain between one and six authored charts, got {0}")]
    ChartCount(usize),
    #[error("model already contains {0:?}")]
    DuplicateView(CanonicalView),
    #[error("model has no authored {0:?} chart")]
    MissingView(CanonicalView),
    #[error("the last authored chart cannot be removed")]
    LastChart,
    #[error("{view:?} dimensions {actual:?} do not match {expected:?}")]
    DimensionMismatch { view: CanonicalView, expected: (u32, u32), actual: (u32, u32) },
    #[error("{view:?} pixel ({x}, {y}) has inward depth {actual}, above maximum {maximum}")]
    ReliefBeyondMaximum { view: CanonicalView, x: u32, y: u32, actual: u8, maximum: u8 },
    #[error(transparent)]
    Chart(#[from] ChartError),
}
```

`AuthoredModel::new` must reject count, dimensions, duplicates, and every nonzero-alpha pixel whose `255 - alpha` exceeds `view.maximum_inward_depth(bounds)`. The same validation must run inside `add_chart`, `replace_chart`, and `set_rgba` before mutation. `resolve` must iterate ranks `0..6`, choose explicit then opposite, rebuild the chosen chart with the requested view without flipping bytes, and return a `ResolvedCharts` carrying the same bounds. `Bounds::new` must reject both zero and values above `63`; replace `ChartError::ZeroBounds` with `ChartError::BoundsOutOfRange { width, height, depth }` so every invalid dimension reports the attempted bounds. Move the existing renderer frame table verbatim into `CanonicalView::frame(bounds)` and implement `opposite()` in core.

- [ ] **Step 4: Run core tests**

Run: `cargo test -p relief-core`

Expected: PASS, including existing interpolation and warp tests.

- [ ] **Step 5: Commit the core authority**

```bash
git add crates/relief-core
git commit -m "feat: add authoritative relief sprite model"
```

---

### Task 2: Remove competing model, fallback, and raw-render paths

**Files:**
- Modify: `crates/depthsprite-format/src/{lib.rs,error.rs,load.rs,manifest.rs,save.rs}`
- Modify: `crates/depthsprite-format/tests/{package_roundtrip.rs,round_trip.rs}`
- Modify: `crates/relief-render/src/{lib.rs,compositor.rs,presets.rs}`
- Modify: `crates/relief-render/tests/{compositing.rs,presets.rs,bowl_acceptance.rs}`
- Modify: `crates/editor-core/src/{lib.rs,document.rs,edit.rs,history.rs,io.rs,preview.rs}`
- Delete: `crates/editor-core/src/fallback.rs`
- Delete: `crates/editor-core/src/source.rs`
- Modify: all `crates/editor-core/tests/*.rs`
- Modify: `crates/desktop-app/src/canvas.rs`

**Interfaces:**
- Consumes: `AuthoredModel`, `ResolvedCharts`, `ModelError`, `CanonicalView::frame`
- Produces: `load_path/load_reader -> Result<AuthoredModel, PackageError>`
- Produces: `save_path_atomic/save_writer(&AuthoredModel, ...)`
- Produces: `render_model(&ResolvedCharts, &RenderRequest)`
- Produces: real `TargetView::{front, back, left, right, top, bottom}` camera presets; no all-charts bypass
- Produces: `EditorDocument::from_model(AuthoredModel, Option<PathBuf>)`
- Produces: `EditorDocument::{model, source, sources, to_model}` using core charts directly

- [ ] **Step 1: Rewrite cross-crate tests against the selected types**

Change package tests to import `relief_core::AuthoredModel`. Add a package test with bounds `[1, 1, 1]` and a Front pixel with alpha `250`; loading must return `PackageError::Model(ModelError::ReliefBeyondMaximum { actual: 5, maximum: 4, .. })`. Add the same invalid PNG import case to the editor tests and assert bounds, RGBA, revision, dirty state, and undo/redo history are all unchanged after rejection. Change renderer tests to use:

```rust
fn resolved(bounds: Bounds, charts: Vec<Chart>) -> ResolvedCharts {
    AuthoredModel::new(bounds, charts).unwrap().resolve()
}

let charts = resolved(bounds, vec![front, back]);
let frame = render_model(&charts, &request).unwrap();
```

Replace the old rear-side test with two direct behavioral tests, including their complete helpers:

```rust
fn solid(bounds: Bounds, view: CanonicalView, pixel: [u8; 4]) -> Chart {
    let (width, height) = view.dimensions(bounds);
    Chart::from_rgba(view, width, height, vec![pixel; (width * height) as usize]).unwrap()
}

#[test]
fn explicit_opposites_never_bleed_through_each_other() {
    let bounds = Bounds::new(2, 2, 2).unwrap();
    let front = solid(bounds, CanonicalView::Front, [255, 0, 0, 255]);
    let back = solid(bounds, CanonicalView::Back, [0, 0, 255, 255]);
    let resolved = AuthoredModel::new(bounds, vec![front, back]).unwrap().resolve();
    let front_request = RenderRequest::new(8, 8, TargetView::front());
    let back_request = RenderRequest::new(8, 8, TargetView::back());
    assert!(render_model(&resolved, &front_request).unwrap().pixels().iter().all(|p| p[2] == 0));
    assert!(render_model(&resolved, &back_request).unwrap().pixels().iter().all(|p| p[0] == 0));
}

#[test]
fn one_authored_front_is_visible_as_a_derived_back_observation() {
    let bounds = Bounds::new(2, 2, 2).unwrap();
    let front = solid(bounds, CanonicalView::Front, [7, 11, 13, 255]);
    let resolved = AuthoredModel::new(bounds, vec![front]).unwrap().resolve();
    let request = RenderRequest::new(8, 8, TargetView::back());
    let rear = render_model(&resolved, &request).unwrap();
    let visible = rear.pixels().iter().enumerate().find(|(_, pixel)| **pixel == [7, 11, 13, 255]);
    let (index, _) = visible.expect("derived Back observation must render");
    let x = index as u32 % rear.width();
    let y = index as u32 / rear.width();
    assert_eq!(rear.owner_at(x, y).unwrap().view, CanonicalView::Back);
}

#[test]
fn resolved_charts_are_invisible_when_edge_on() {
    let bounds = Bounds::new(2, 2, 2).unwrap();
    let front = solid(bounds, CanonicalView::Front, [7, 11, 13, 255]);
    let resolved = AuthoredModel::new(bounds, vec![front]).unwrap().resolve();
    let request = RenderRequest::new(8, 8, TargetView::left());
    let edge_on = render_model(&resolved, &request).unwrap();
    assert!(edge_on.pixels().iter().all(|pixel| pixel[3] == 0));
}
```

Update editor tests to compare `AuthoredModel` and `Chart` directly and to expect magenta-empty pixels. Delete tests of `resolve_charts` and replace them with preview tests that exercise `document.model().resolve()`.

- [ ] **Step 2: Run the workspace tests and verify consumers still require displaced types**

Run: `cargo test --workspace --no-run`

Expected: FAIL at imports/usages of `DepthSpriteModel`, `SourceSprite`, `resolve_charts`, and the old `render_model(bounds, &[Chart], ...)` signature.

- [ ] **Step 3: Migrate package I/O directly to the core model**

Delete `DepthSpriteModel` from `depthsprite-format/src/lib.rs`. Make load construct `AuthoredModel::new(bounds, charts)` and make save iterate `model.charts()`. Reduce `PackageError` to archive/manifest/PNG/I/O errors plus:

```rust
#[error(transparent)]
Model(#[from] relief_core::ModelError),
```

Manifest validation may validate format/version and call `Bounds::new`; chart count, uniqueness, dimensions, and depth belong only to `AuthoredModel::new`.

- [ ] **Step 4: Make resolved charts the renderer's only model input**

Change the public signature to:

```rust
pub fn render_model(
    charts: &ResolvedCharts,
    request: &RenderRequest,
) -> Result<FrameBuffer, RenderError>
```

Read bounds from `charts.bounds()`. Iterate `charts.charts()`. Pass `f64::from(chart.view().maximum_inward_depth(bounds))` into `solve_preimages` and `add_grid_crossings` instead of using global `MAX_RELIEF`. Replace the renderer-private `chart_frame` table with `CanonicalView::frame(bounds)`. Add Back, Left, and Bottom `TargetView` constructors symmetric to the existing Front, Right, and Top cameras. Delete `ProjectionSource::IdentityAllCharts` and `TargetView::front_for_test`; tests must use these real camera bases so culling is never bypassed.

- [ ] **Step 5: Make the editor document wrap one core model**

Replace `DocumentState { bounds, sources, ... }` with:

```rust
pub(crate) struct DocumentState {
    pub(crate) model: AuthoredModel,
    pub(crate) selection: CanonicalView,
    pub(crate) active_layer: ActiveLayer,
    pub(crate) tool: Tool,
    pub(crate) current_rgb: [u8; 3],
    pub(crate) current_depth: DepthValue,
}
```

Delete `SourceSprite` and editor fallback. Return `&Chart` from `source`, iterate `model.charts()` from `sources`, clone the core model from `to_model`, and delegate add/replace/remove/pixel mutation to `AuthoredModel`. `EditorError` wraps `ModelError`, `PackageError`, and `RenderError`; remove duplicate source/dimension variants. Preview must call `document.model().resolve()` and `render_model(&resolved, request)`. Update `canvas.rs` to read `[r, g, b, a]` through `Chart::rgba_at` while preserving its existing absolute `depth_display_alpha` formula.

- [ ] **Step 6: Run migration tests and prove displaced owners are gone**

Run: `cargo test --workspace`

Expected: PASS.

Run: `rg -n "DepthSpriteModel|SourceSprite|resolve_charts|IdentityAllCharts|front_for_test" crates`

Expected: no matches.

- [ ] **Step 7: Commit the ownership migration**

```bash
git add crates/depthsprite-format crates/relief-render crates/editor-core crates/desktop-app/src/canvas.rs
git commit -m "refactor: use one resolved sprite model boundary"
```

---

### Task 3: Signed-edge resizing, reassignment, and editor history

**Files:**
- Create: `crates/relief-core/src/resize.rs`
- Modify: `crates/relief-core/src/{chart.rs,frame.rs,lib.rs,model.rs}`
- Create: `crates/relief-core/tests/resize.rs`
- Modify: `crates/editor-core/src/{document.rs,edit.rs,history.rs,lib.rs}`
- Create: `crates/editor-core/tests/dimensions.rs`
- Modify: `crates/editor-core/tests/history.rs`

**Interfaces:**
- Produces: `ImageEdge::{Left,Right,Top,Bottom}`
- Produces: `WorldAxis::{X,Y,Z}`, `AxisSide::{Min,Max}`, `WorldEdge { axis, side }`
- Produces: `ResizeDelta::{Add,Remove}`, `ResizeRequest { view, edge, delta }`
- Produces: `DiscardPolicy::{Reject,Allow}`, `ChartEdge { view, edge }`
- Produces: `ReassignMode::{Preserve,RecreateEmpty}`
- Produces: `AuthoredModel::{resize, reassign_chart}`
- Produces: `EditorDocument::{select_source, maximum_inward_depth, resize_source, reassign_source}`

- [ ] **Step 1: Write all signed-edge and transaction tests**

Create a 24-row expected mapping in `relief-core/tests/resize.rs`:

```rust
let expected = [
    (CanonicalView::Front, ImageEdge::Left, WorldAxis::X, AxisSide::Min),
    (CanonicalView::Front, ImageEdge::Right, WorldAxis::X, AxisSide::Max),
    (CanonicalView::Front, ImageEdge::Top, WorldAxis::Y, AxisSide::Min),
    (CanonicalView::Front, ImageEdge::Bottom, WorldAxis::Y, AxisSide::Max),
    (CanonicalView::Back, ImageEdge::Left, WorldAxis::X, AxisSide::Max),
    (CanonicalView::Back, ImageEdge::Right, WorldAxis::X, AxisSide::Min),
    (CanonicalView::Back, ImageEdge::Top, WorldAxis::Y, AxisSide::Min),
    (CanonicalView::Back, ImageEdge::Bottom, WorldAxis::Y, AxisSide::Max),
    (CanonicalView::Left, ImageEdge::Left, WorldAxis::Z, AxisSide::Min),
    (CanonicalView::Left, ImageEdge::Right, WorldAxis::Z, AxisSide::Max),
    (CanonicalView::Left, ImageEdge::Top, WorldAxis::Y, AxisSide::Min),
    (CanonicalView::Left, ImageEdge::Bottom, WorldAxis::Y, AxisSide::Max),
    (CanonicalView::Right, ImageEdge::Left, WorldAxis::Z, AxisSide::Max),
    (CanonicalView::Right, ImageEdge::Right, WorldAxis::Z, AxisSide::Min),
    (CanonicalView::Right, ImageEdge::Top, WorldAxis::Y, AxisSide::Min),
    (CanonicalView::Right, ImageEdge::Bottom, WorldAxis::Y, AxisSide::Max),
    (CanonicalView::Top, ImageEdge::Left, WorldAxis::X, AxisSide::Min),
    (CanonicalView::Top, ImageEdge::Right, WorldAxis::X, AxisSide::Max),
    (CanonicalView::Top, ImageEdge::Top, WorldAxis::Z, AxisSide::Min),
    (CanonicalView::Top, ImageEdge::Bottom, WorldAxis::Z, AxisSide::Max),
    (CanonicalView::Bottom, ImageEdge::Left, WorldAxis::X, AxisSide::Min),
    (CanonicalView::Bottom, ImageEdge::Right, WorldAxis::X, AxisSide::Max),
    (CanonicalView::Bottom, ImageEdge::Top, WorldAxis::Z, AxisSide::Max),
    (CanonicalView::Bottom, ImageEdge::Bottom, WorldAxis::Z, AxisSide::Min),
];
for (view, image, axis, side) in expected {
    assert_eq!(view.world_edge(image), WorldEdge { axis, side });
    assert_eq!(view.image_edge(WorldEdge { axis, side }), Some(image));
}
```

Add tests proving one Front-left addition changes Front/Back and Top/Bottom on their correctly mirrored local edges, leaves Left/Right raster dimensions unchanged, inserts `EMPTY_RGBA`, and increments width once. Add remove tests proving `DiscardPolicy::Reject` returns `ResizeWouldDiscard` without mutation and `Allow` removes exactly those pixels. For perpendicular relief validation, start with bounds `[2, 2, 2]` and a Front pixel at relief `8`, then request `ResizeRequest { view: Right, edge: Left, delta: Remove }`; the prospective depth becomes `1`, so the operation must return `ReliefBeyondMaximum { actual: 8, maximum: 4, .. }` without mutation. Also prove `1/63` limits. Add reassignment tests proving an occupied target is rejected unchanged, `Preserve` retains exact RGBA when dimensions match, dimension mismatch is rejected unchanged, and `RecreateEmpty` creates exactly one correctly sized magenta-empty target. Add editor tests proving resize/reassign are single undo steps, preview revision advances once, and selecting a lower-maximum side reduces only current tool depth.

- [ ] **Step 2: Run the focused tests and verify resize types are absent**

Run: `cargo test -p relief-core --test resize`

Expected: FAIL because the signed-edge and resize interfaces do not exist.

- [ ] **Step 3: Implement canonical edge conversion and raster reconstruction**

Define:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)] pub enum ImageEdge { Left, Right, Top, Bottom }
#[derive(Clone, Copy, Debug, Eq, PartialEq)] pub enum WorldAxis { X, Y, Z }
#[derive(Clone, Copy, Debug, Eq, PartialEq)] pub enum AxisSide { Min, Max }
#[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct WorldEdge { pub axis: WorldAxis, pub side: AxisSide }
#[derive(Clone, Copy, Debug, Eq, PartialEq)] pub enum ResizeDelta { Add, Remove }
#[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct ResizeRequest { pub view: CanonicalView, pub edge: ImageEdge, pub delta: ResizeDelta }
#[derive(Clone, Copy, Debug, Eq, PartialEq)] pub enum DiscardPolicy { Reject, Allow }
#[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct ChartEdge { pub view: CanonicalView, pub edge: ImageEdge }
#[derive(Clone, Copy, Debug, Eq, PartialEq)] pub enum ReassignMode { Preserve, RecreateEmpty }
```

Implement the exact table from Step 1. Add a crate-private `Chart::resized(edge, delta) -> Chart` that copies rows/columns without interpolation and inserts `EMPTY_RGBA`. Do not expose mutable RGBA slices.

- [ ] **Step 4: Implement transactional model resize and reassignment**

`AuthoredModel::resize(request, policy)` must:

1. Convert the selected local edge to `WorldEdge`.
2. Compute new bounds and reject outside `1..=63`.
3. Find each authored chart whose image plane contains that world axis and map the same `WorldEdge` back to its local edge.
4. Before removal, collect every affected edge containing a pixel other than `EMPTY_RGBA`; return `ModelError::ResizeWouldDiscard { edges }` under `Reject`.
5. Rebuild affected charts on a clone, construct `AuthoredModel::new(new_bounds, charts)`, and assign to `self` only after full validation.

`reassign_chart(from, to, Preserve)` preserves bytes only when canonical dimensions match. `RecreateEmpty` replaces the source with one magenta-empty target chart. Both reject occupied targets and are transactional.

- [ ] **Step 5: Add editor commands and history around core transactions**

`select_source(view)` validates existence, updates selection, and if current depth is relief above `view.maximum_inward_depth(bounds)`, changes only the transient tool value. `resize_source` and `reassign_source` clone `DocumentState`, call the model operation, preserve/select the correct side, and use `finish_command` once. A failed operation must not change state, dirty status, history, revision, or preview identity.

- [ ] **Step 6: Run core and editor tests**

Run: `cargo test -p relief-core --test resize`

Expected: PASS.

Run: `cargo test -p editor-core`

Expected: PASS.

- [ ] **Step 7: Commit dimension editing**

```bash
git add crates/relief-core crates/editor-core
git commit -m "feat: add synchronized sprite dimension editing"
```

---

### Task 4: Explicit side assignment and compact resize UI

**Files:**
- Modify: `crates/desktop-app/src/{source_grid.rs,canvas.rs,palette.rs}`
- Modify: `crates/desktop-app/tests/{source_grid.rs,canvas.rs,palette.rs,application.rs}`
- Preserve: `crates/desktop-app/src/layout.rs`
- Preserve: `crates/desktop-app/tests/layout.rs`

**Interfaces:**
- Consumes: editor selection, reassignment, resize, maximum inward depth, and error types
- Produces: compact Add Sprite chooser, side selector, resize popover, recreate confirmation, destructive-edge confirmation

- [ ] **Step 1: Replace implicit-add and fallback-header tests with interaction tests**

Delete tests for `add_next_source` and `Front → Back`. Add helpers and assertions equivalent to:

```rust
#[test]
fn add_action_accepts_an_explicit_unoccupied_side() {
    let mut document = document();
    add_source(&mut document, CanonicalView::Back).unwrap();
    assert!(document.source(CanonicalView::Back).is_some());
    assert!(document.source(CanonicalView::Right).is_none());
}

#[test]
fn card_headers_only_name_the_assigned_side() {
    let document = document();
    assert_eq!(card_header(&document, CanonicalView::Front).unwrap().label, "Front");
}
```

Extend the real-egui observation to record the add chooser, side selector, resize button, and resize popover. Simulate clicks proving: Back can be added second; an occupied target is disabled; same-size reassignment preserves RGBA; mismatched reassignment opens a recreate confirmation; remove on an authored edge opens a named confirmation; confirmation produces one undoable model change; cancel leaves revision unchanged; controls remain inside the 18-pixel header or their popovers and do not reduce canvas rectangles.

- [ ] **Step 2: Run source-grid tests and verify the old UI fails the new behavior**

Run: `cargo test -p desktop-app --test source_grid`

Expected: FAIL because Add Sprite still calls `add_next_source`, headers contain fallback arrows, and assignment/resize controls do not exist.

- [ ] **Step 3: Implement explicit add and side assignment**

Replace `add_next_source` with:

```rust
pub fn add_source(document: &mut EditorDocument, view: CanonicalView) -> Result<(), EditorError> {
    document.add_source(view)
}
```

Render the fixed Add Sprite rectangle as an `egui::menu_button`; list canonical names in display order and disable occupied sides. In each card header, use the side name as a menu button; disable the current and occupied sides. Call `reassign_source(..., Preserve)` when dimensions match, otherwise store `PendingSourceAction::Recreate { from, to }` and show a modal with **Recreate Empty** and **Cancel**. Card text must never mention derived/fallback sides.

Define the UI display order once as `[Front, Right, Top, Back, Left, Bottom]` and use it for the Add chooser and packed source cards. This is presentation order only; core canonical rank remains `[Front, Right, Back, Left, Top, Bottom]` for deterministic compositing.

- [ ] **Step 4: Implement the resize popover and confirmations**

Add state:

```rust
enum PendingSourceAction {
    Recreate { from: CanonicalView, to: CanonicalView },
    DiscardResize { request: ResizeRequest, edges: Vec<ChartEdge> },
}
```

Place one compact Resize menu in the selected card header. Its popover shows the current `width × height` and eight actions arranged around a central rectangle: add/remove Top, Bottom, Left, Right. First call `resize_source(request, DiscardPolicy::Reject)`; convert only `ResizeWouldDiscard` into `PendingSourceAction::DiscardResize`, and show other errors beside the source grid. The modal must list affected side/edge names and offer **Remove Pixels** and **Cancel**. Confirmation repeats the same request with `DiscardPolicy::Allow`.

- [ ] **Step 5: Bind selection and maximum inward depth to canvases and palette**

When either canvas receives a primary press, call `document.select_source(view)` before selecting its layer. Change the Relief slider range from `0..=254` to `0..=document.maximum_inward_depth()`. Keep `depth_display_alpha` unchanged and add a regression test proving alpha `223` displays the same gray for Front and Top even when their maximum inward depths differ.

- [ ] **Step 6: Run desktop tests**

Run: `cargo test -p desktop-app`

Expected: PASS, including the existing layout ratio tests without changing canvas allocation.

- [ ] **Step 7: Commit the editor UI**

```bash
git add crates/desktop-app
git commit -m "feat: add explicit side and dimension controls"
```

---

### Task 5: Deterministic lit block, corrected bowl, and two-sided globe

**Files:**
- Create: `crates/fixture-gen/src/{pixel.rs,block.rs,bowl.rs,globe.rs}`
- Modify: `crates/fixture-gen/src/lib.rs`
- Modify: `crates/fixture-gen/tests/reproducibility.rs`
- Replace: `assets/examples/{block.depthsprite,bowl.depthsprite}`
- Create: `assets/examples/globe.depthsprite`
- Replace: `crates/relief-render/tests/bowl_acceptance.rs`
- Create: `crates/relief-render/tests/demo_acceptance.rs`

**Interfaces:**
- Produces: deterministic `block_model`, `bowl_model`, `globe_model`
- Produces: shared integer `rgba`, `integer_sqrt`, `shade`, mask-boundary, and package helpers

- [ ] **Step 1: Write source and rendered acceptance tests before changing generators**

Set the reproducibility asset list to all six final names immediately so this test fails until Tasks 5 and 6 finish:

```rust
const ASSETS: [&str; 6] = [
    "block.depthsprite", "bowl.depthsprite", "globe.depthsprite",
    "gyroscope.depthsprite", "tent.depthsprite", "dome.depthsprite",
];
```

Add focused Task 5 tests whose names begin with `foundational_`:

- block has all six explicit charts, every foreground relief is zero, and each chart has at least four RGB values;
- bowl bounds are `[32, 12, 32]`, Front is `32×12`, rows `0` and `11` contain foreground, representative rows and columns contain multiple relief values, and Top/Front each have multiple RGB values;
- globe bounds are `[48, 48, 48]`, exactly Front and Back are authored, their RGB patterns differ, and each silhouette boundary contains pixels at relief `192`;
- front/rear globe renders contain the corresponding explicit colors and oblique silhouette occupancy is connected;
- the bowl acceptance render contains top basin/rim/front exterior owners and top/front ownership regions touch within an eight-neighbor output neighborhood.

- [ ] **Step 2: Run focused tests and verify the current assets fail**

Run: `cargo test -p relief-render --test demo_acceptance foundational_`

Expected: FAIL because globe is absent, block has three flat-color charts, and bowl dimensions/lighting/vertical relief are wrong.

- [ ] **Step 3: Split the generator and implement deterministic pixel helpers**

Use only integer math in `pixel.rs`:

```rust
pub(crate) fn rgba(rgb: [u8; 3], relief: u8) -> [u8; 4] {
    [rgb[0], rgb[1], rgb[2], 255 - relief]
}

pub(crate) fn shade(base: [u8; 3], light: i32, relief: u8) -> [u8; 3] {
    std::array::from_fn(|channel| {
        (i32::from(base[channel]) + light - i32::from(relief) / 8).clamp(0, 255) as u8
    })
}
```

Move integer square root and boundary detection here. `lib.rs` only creates the output directory and saves each module's `AuthoredModel` in the six-name order.

- [ ] **Step 4: Build the six-face lit flat block**

Keep `[16, 16, 16]` bounds. Generate every canonical chart with relief zero. Give each side a base light term derived from the fixed upper-left-front light (`Top +36`, `Front +22`, `Left +12`, `Right -8`, `Back -18`, `Bottom -30`) plus the within-face integer gradient `(width - x + height - y) / 4`. Assert relief remains zero after shading.

- [ ] **Step 5: Rebuild the two-chart bowl**

Use `[32, 12, 32]` bounds. Top uses a circular mask and relief from `0` on the rim to at most `48` on the basin floor. Front row `y` uses a half-width that decreases monotonically from `16` at `y=0` to one or two center texels at `y=11`; for each occupied pixel compute the row-specific elliptical front distance so relief depends on both `x` and `y` and remains at most `128`. Use the same radius/front-distance function for the Top near rim and Front row 0. Apply upper-left highlights, lower-right falloff, rim distinction, and relief darkening only to RGB.

- [ ] **Step 6: Build the explicit two-hemisphere globe**

For each `48×48` chart use doubled center coordinates `dx = 2x + 1 - 48`, `dy = 2y + 1 - 48` and circular radius `48`. Interior inward depth is `4 * (48 - integer_sqrt(48² - dx² - dy²))`, clamped to `192`; force every foreground pixel adjacent to background to `192` so the two explicit hemispheres meet at the midpoint. Generate distinct corresponding continent masks from deterministic longitude/latitude bands, add grid accents and an eight-unit inset away from the boundary, and apply one coherent light gradient.

- [ ] **Step 7: Regenerate and run focused acceptance tests**

Run: `cargo run -p fixture-gen -- assets/examples`

Expected: block and bowl are replaced and globe is created.

Run: `cargo test -p relief-render --test demo_acceptance foundational_`

Expected: PASS for the Task 5 cases; the six-name reproducibility test may still fail because Task 6 assets are not present.

- [ ] **Step 8: Commit foundational demos**

```bash
git add crates/fixture-gen crates/relief-render/tests assets/examples
git commit -m "feat: add lit relief demonstration assets"
```

---

### Task 6: Asymmetric gyroscope, curved tent, and architectural dome

**Files:**
- Create: `crates/fixture-gen/src/{gyroscope.rs,tent.rs,dome.rs}`
- Modify: `crates/fixture-gen/src/lib.rs`
- Modify: `crates/relief-render/tests/demo_acceptance.rs`
- Create: `assets/examples/{gyroscope.depthsprite,tent.depthsprite,dome.depthsprite}`

**Interfaces:**
- Produces: six explicit gyroscope charts and three-chart tent/dome models
- Consumes: shared pixel, package, core validation, and renderer helpers from Task 5

- [ ] **Step 1: Add failing source and rendered claims for all ambitious demos**

Add tests whose names begin with `ambitious_`. They must prove:

```rust
assert_eq!(views(&gyroscope), vec![Front, Right, Back, Left, Top, Bottom]);
for (a, b) in [(Front, Back), (Left, Right), (Top, Bottom)] {
    assert_ne!(gyroscope.chart(a).unwrap().rgba(), gyroscope.chart(b).unwrap().rgba());
}
assert_eq!(views(&tent), vec![Front, Right, Top]);
assert_eq!(views(&dome), vec![Front, Right, Top]);
```

Also assert gyroscope charts contain three ring palette families and alpha-empty gaps; opposite target renders have different owner/color distributions; tent Front has an alpha-empty entrance surrounded by foreground, Right/Top have multiple relief values along both axes, and ridge/eave owners connect in oblique render; dome Front/Right/Top contain repeated rib colors, multiple two-axis relief values, and connected crown/drum ownership with no Back/Left/Bottom owner in the standard Front-Right-Top oblique view.

- [ ] **Step 2: Run the ambitious demo tests and verify assets are absent**

Run: `cargo test -p relief-render --test demo_acceptance ambitious_`

Expected: FAIL because the three packages do not exist.

- [ ] **Step 3: Generate six asymmetric gyroscope observations**

Use `[48, 48, 48]` bounds and all six sides. Define three ring identities (outer red, middle green, inner blue) and start with this exact six-view projection table. Each tuple is `(radius_u, radius_v, direction_u, direction_v, thickness, phase, draw_order)`:

| View | outer red | middle green | inner blue |
|---|---|---|---|
| Front | `(21, 15, 4, 1, 2, 1, 0)` | `(16, 10, 3, -2, 2, 5, 2)` | `(10, 6, 1, 0, 2, 9, 1)` |
| Back | `(20, 14, 3, -2, 2, 7, 2)` | `(16, 11, 4, 1, 2, 2, 0)` | `(9, 6, 1, 1, 2, 11, 1)` |
| Left | `(21, 13, 2, 3, 2, 4, 1)` | `(15, 11, 1, -3, 2, 8, 0)` | `(10, 5, 1, 0, 2, 12, 2)` |
| Right | `(20, 15, 1, -3, 2, 10, 0)` | `(16, 9, 2, 3, 2, 3, 2)` | `(9, 6, 0, 1, 2, 14, 1)` |
| Top | `(19, 16, 3, 1, 2, 6, 2)` | `(16, 10, 1, 3, 2, 12, 1)` | `(10, 6, 2, -1, 2, 1, 0)` |
| Bottom | `(21, 14, 1, 3, 2, 13, 1)` | `(15, 11, 3, -1, 2, 4, 0)` | `(9, 5, 1, 2, 2, 7, 2)` |

For centered pixel coordinates `(x, y)`, rotate into each record with `u_num = direction_u*x + direction_v*y` and `v_num = -direction_v*x + direction_u*y`; carry the shared squared direction norm into the denominator rather than rounding `u` or `v`. Classify annular pixels with fixed-point ellipse metric

```text
metric = u² × 4096 / radius_u² + v² × 4096 / radius_v²
foreground when |metric - 4096| <= band
```

At overlaps, select the ring with the greatest `draw_order` in that view. Use `thickness` as the source-pixel half-width when deriving `band`; vary relief across band width and the 16-step arc `phase`, keep every value `<=192`, leave gaps alpha zero, and apply the fixed light to RGB. Do not derive any gyroscope opposite through fallback. The approved asset bounds are starting values, so adjust only these numeric projection records if rendered evidence shows a disconnected or visually collapsed ring, and update the table in the implementation comment and acceptance fixture together.

- [ ] **Step 4: Generate the three-chart curved tent**

Use `[48, 28, 36]` bounds with Front, Right, Top. Front builds a peaked mask with a central alpha-empty entrance and a separate foreground flap. Right uses a curved wall profile; Top uses two roof planes meeting at the ridge. Add deterministic sag `fold = amplitude * distance_from_ridge * periodic_position / denominator`, clamp each chart to its maximum inward depth, and carry matching ridge/eave landmark indices across charts. Apply fabric stripes, seams, and upper-left-front lighting in RGB.

- [ ] **Step 5: Generate the three-chart dome**

Use `[48, 32, 48]` bounds with Front, Right, Top. Side masks combine a drum and circular dome profile; Top uses a circular radial shell. Compute inward relief from the relevant circular cross-section, overlay ribs at matching angular/index intervals, add windows only on the drum, and share crown/rib endpoints across charts. Keep relief within `128` for Top and `192` for Front/Right.

- [ ] **Step 6: Regenerate all packages and run complete asset validation**

Run: `cargo run -p fixture-gen -- assets/examples`

Run: `cargo test -p fixture-gen --test reproducibility`

Run: `cargo test -p relief-render --test demo_acceptance`

Expected: all PASS and all six committed packages byte-match a fresh generation.

- [ ] **Step 7: Commit ambitious demos**

```bash
git add crates/fixture-gen crates/relief-render/tests/demo_acceptance.rs assets/examples
git commit -m "feat: add ambitious relief sprite demos"
```

---

### Task 7: Repo-local DepthSprite asset-authoring skill

**Files:**
- Create: `.codex/skills/create-depthsprite-assets/SKILL.md`
- Create: `.codex/skills/create-depthsprite-assets/agents/openai.yaml`
- Create: `.codex/skills/create-depthsprite-assets/references/asset-principles.md`

**Interfaces:**
- Consumes: approved specs, fixture generator, package loader, demo acceptance tests
- Produces: repo-local workflow for creating, repairing, and reviewing DepthSprite assets

- [ ] **Step 1: Invoke the required skill-authoring guidance before creating files**

Read the complete `skill-creator` and `superpowers:writing-skills` instructions, then read `skill-creator/references/openai_yaml.md`. These instructions control this task; note any required validation commands before scaffolding.

- [ ] **Step 2: Scaffold the repo-local skill**

Run:

```bash
python /home/ahart/.codex/skills/.system/skill-creator/scripts/init_skill.py create-depthsprite-assets \
  --path .codex/skills \
  --resources references \
  --interface display_name="Create DepthSprite Assets" \
  --interface short_description="Create and validate fixed-scale DepthSprite source charts" \
  --interface default_prompt='Use $create-depthsprite-assets to create or repair a DepthSprite asset and prove it in rendered views.'
```

Expected: the three specified files/directories exist and no example placeholder resource is created.

- [ ] **Step 3: Replace scaffold text with the approved claim-first workflow**

Use this frontmatter and keep `SKILL.md` concise:

```markdown
---
name: create-depthsprite-assets
description: Use when creating, repairing, or reviewing DepthSprite PNG charts, deterministic fixtures, example packages, baked lighting, seams, or rendered asset acceptance.
---

# Create DepthSprite Assets

1. Read `docs/specs/oriented-relief-model.md` and the relevant example claim in `docs/specs/depthsprite-demo-assets.md`.
2. State the visual claim and choose the minimum explicit canonical observations that can prove it without accidental opposite symmetry.
3. Derive every raster dimension and signed edge from `AuthoredModel` bounds.
4. Build tight masks and shared landmarks; use `[255, 0, 255, 0]` for empty pixels.
5. Derive relief from cross-sections at eight units per pixel without exceeding maximum inward depth.
6. Add coherent upper-left-front lighting in RGB only.
7. Generate through `fixture-gen`, inspect source color/depth and several rendered views, and run the focused acceptance tests.
8. Reject disconnected seams, unexplained padding, rear-facing bleed, ambiguous curvature, or source-only proof.

Read `references/asset-principles.md` for canonical axes, formulas, and the acceptance checklist.
```

The reference must include the six signed axis rows, `h = 255 - alpha`, `h_max = 4L`, absolute grayscale formula, resize synchronization rule, tight-crop/seam rules, integer cross-section and lighting guidance, exact generator/test commands, and links to all three governing specs.

- [ ] **Step 4: Validate structure and perform one forward-use trial**

Run:

```bash
python /home/ahart/.codex/skills/.system/skill-creator/scripts/quick_validate.py .codex/skills/create-depthsprite-assets
```

Expected: validation succeeds.

Use the skill instructions to audit `assets/examples/globe.depthsprite`; record the concrete evidence by adding or strengthening a globe source/render assertion if the workflow finds a gap. Rerun `cargo test -p relief-render --test demo_acceptance foundational_globe` and expect PASS.

- [ ] **Step 5: Commit the skill**

```bash
git add .codex/skills/create-depthsprite-assets crates/relief-render/tests/demo_acceptance.rs
git commit -m "docs: add DepthSprite asset authoring skill"
```

---

### Task 8: Integrated conformance and realistic visual handoff

**Files:**
- Modify: `crates/editor-core/tests/file_lifecycle.rs`
- Update: `/tmp/sprite-models-foundation-20260714-24.md` conformance record after all checks pass

**Interfaces:**
- Consumes: the complete integrated application, examples, tests, and skill
- Produces: authoritative evidence that the approved model works and displaced paths are absent

- [ ] **Step 1: Prove displaced structures no longer exist**

Run:

```bash
rg -n "DepthSpriteModel|SourceSprite|add_next_source|resolve_charts|IdentityAllCharts|front_for_test|Front → Back" crates docs/specs
```

Expected: no matches.

Run: `rg -n "MICROCELLS_PER_AXIS|microtriangle|rasterize.*triangle" crates docs/specs`

Expected: no matches; inverse image warping remains the sole renderer.

- [ ] **Step 2: Run formatting, lint, tests, and release build**

Run: `cargo fmt --all -- --check`

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings`

Run: `cargo test --workspace`

Run: `cargo build --release`

Expected: every command exits zero.

- [ ] **Step 3: Capture bounded visual evidence for the editor and ambitious assets**

Launch the release viewer under a bounded X server for every shipped model, capture the root window, and terminate each instance in the same bounded command:

```bash
for name in block bowl globe gyroscope tent dome; do
  timeout 20s xvfb-run -a -s "-screen 0 1600x1000x24" sh -c '
    target/release/depthsprite "assets/examples/$1.depthsprite" &
    pid=$!
    sleep 3
    import -window root "/tmp/depthsprite-$1-final.png"
    kill "$pid"
    wait "$pid" || true
  ' sh "$name"
done
```

Inspect all six `/tmp/depthsprite-*-final.png` images with the image-viewing tool. Confirm the conventional top menu, vertical tools and color picker, dominant model viewport, compact Add Sprite control, two-column source cards, color-over-depth canvases, visible baked lighting, flat block, bowl concavity and connected exterior, distinct globe hemispheres, asymmetric gyroscope overlaps, readable tent opening/folds, and connected dome crown/ribs/drum. If a visual defect is found, repair only the responsible task surface and rerun its focused tests plus the six captures.

- [ ] **Step 4: Add and run exact save/reopen integration coverage**

Add `explicit_side_resize_save_reopen` to `crates/editor-core/tests/file_lifecycle.rs`. It must create a document, add Back explicitly, resize one signed edge, undo and redo that single change, save, reopen, and compare exact bounds, canonical views, RGBA bytes, and preview framebuffer.

Run: `cargo test -p editor-core --test file_lifecycle explicit_side_resize_save_reopen -- --exact`

Expected: PASS.

- [ ] **Step 5: Record conformance and confirm a clean branch**

If any validation fails, return to the task that owns the behavior, add or strengthen the focused failing regression, repair that task's implementation, rerun its focused validation and all Task 8 checks, and commit exactly that task's changed files. After every check passes, append `<conformance>` to `/tmp/sprite-models-foundation-20260714-24.md` with owners implemented, obsolete structures deleted, surfaces migrated, exact commands/results, rendered image paths, and clean-worktree evidence.

Run: `git status --short`

Expected: no output.
