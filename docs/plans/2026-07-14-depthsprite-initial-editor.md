# DepthSprite Initial Editor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the approved desktop editor in which one to six oriented RGBA sprites are the complete model authority, color and depth are edited side by side with immediate pseudo-3D preview, and the exact authored model is saved in one `.depthsprite` file.

**Architecture:** Reconstruct charts around lossless raw RGBA, derive relief only at render time, and place all mutable authoring state in a headless `editor-core` crate. A thin `eframe`/`egui` desktop crate owns menus, the vertical tool palette, the 3×2 source grid, paired color/depth canvases, and the read-only orbitable preview. There is one document authority and one opposing-side fallback resolver; no parallel decoded model is retained.

**Tech Stack:** Rust 2024, `png`, `zip`, `serde`, `num-rational`, `eframe`/`egui`, `rfd`, and Cargo workspace tests.

## Global Constraints

- Work in the existing branch and repository; do not create a worktree.
- Follow red-green-refactor for every behavioral change: add a focused failing test, observe the intended failure, add the smallest complete implementation, and rerun it.
- Preserve raw RGB under alpha zero through editing and `.depthsprite` round trips.
- A model has one to six canonical source sprites. Missing opposing sides derive from their assigned opposite; a distinct source replaces that fallback.
- The model viewport is inspection-only. All document changes originate in source management, source canvases, tools, menus, or undo/redo.
- Color is always above depth for each source. Source cards populate in canonical 3×2 order. The model viewport remains at least 3× each mini-canvas in width and height.
- Keep the selected feature set limited to the approved specification. Do not add export, release, archive-adversary, diagnostics, support-overlay, or compatibility systems.

---

## Task 1: Make raw RGBA the chart and package authority

**Files:**

- Modify: `crates/relief-core/src/chart.rs`
- Modify: `crates/relief-core/src/relief.rs`
- Modify: `crates/depthsprite-format/src/save.rs`
- Modify: `crates/depthsprite-format/src/load.rs`
- Modify: affected tests under `crates/relief-core/tests/` and `crates/depthsprite-format/tests/`
- Test: `crates/depthsprite-format/tests/round_trip.rs`

- [ ] Add a failing package round-trip test with a pixel such as `[17, 31, 47, 0]`; assert that reopening returns the same four bytes, not `[0, 0, 0, 0]`.

Run:

```bash
cargo test -p depthsprite-format --test round_trip hidden_rgb_survives_alpha_zero_round_trip -- --exact
```

Expected: failure because `Chart` stores `DecodedTexel::Background` and saving reconstructs transparent black.

- [ ] Replace `Chart`'s decoded texel vector with raw pixels and make relief decoding a derived operation:

```rust
pub struct Chart {
    view: CanonicalView,
    width: u32,
    height: u32,
    rgba: Vec<[u8; 4]>,
}

impl Chart {
    pub fn from_rgba(
        view: CanonicalView,
        width: u32,
        height: u32,
        rgba: Vec<[u8; 4]>,
    ) -> Result<Self, ChartError>;

    pub fn rgba(&self) -> &[[u8; 4]];
    pub fn rgba_at(&self, x: u32, y: u32) -> Option<[u8; 4]>;
    pub fn texel_at(&self, x: u32, y: u32) -> Option<DecodedTexel>;
    pub fn texels(&self) -> impl ExactSizeIterator<Item = DecodedTexel> + '_;
}
```

`texels()` must decode every raw pixel on demand with `decode_rgba`. Migrate renderer and component code to consume that iterator rather than a stored decoded slice.

- [ ] Change PNG serialization to flatten `chart.rgba()` verbatim. Loading remains an RGBA decode followed by `Chart::from_rgba`, with no alpha-based RGB normalization.

- [ ] Update displaced tests that assumed `texels()` returned a slice. Add core assertions that `rgba_at()` retains transparent RGB while `texel_at()` still reports `Background` for rendering.

Run:

```bash
cargo test -p relief-core
cargo test -p depthsprite-format
```

Expected: all core and package tests pass, including exact hidden-RGB preservation.

- [ ] Commit:

```bash
git add crates/relief-core crates/depthsprite-format
git commit -m "refactor: preserve raw rgba model sources"
```

## Task 2: Add the authoritative editable document and side fallback

**Files:**

- Modify: `Cargo.toml`
- Create: `crates/editor-core/Cargo.toml`
- Create: `crates/editor-core/src/lib.rs`
- Create: `crates/editor-core/src/source.rs`
- Create: `crates/editor-core/src/document.rs`
- Create: `crates/editor-core/src/fallback.rs`
- Test: `crates/editor-core/tests/document.rs`
- Test: `crates/editor-core/tests/fallback.rs`

- [ ] Add failing tests for these invariants:

  - a new document owns one empty Front source and reports no unsaved changes;
  - adding sources follows canonical order and refuses a seventh source;
  - one Front source resolves as both Front and Back;
  - adding a distinct Back source replaces only the Back fallback;
  - removing Back immediately restores the Front-to-Back fallback;
  - resolved opposing charts retain the same raw pixel array but have the target canonical view.

Run:

```bash
cargo test -p editor-core --test document
cargo test -p editor-core --test fallback
```

Expected: Cargo reports that `editor-core` does not yet exist.

- [ ] Add the crate and these durable types:

```rust
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSprite {
    view: CanonicalView,
    width: u32,
    height: u32,
    rgba: Vec<[u8; 4]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActiveLayer { Color, Depth }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tool { Pencil, Eraser, Fill, Eyedropper }

pub struct EditorDocument {
    state: DocumentState,
    saved_state: DocumentState,
    undo: Vec<DocumentState>,
    redo: Vec<DocumentState>,
    stroke_before: Option<DocumentState>,
    path: Option<PathBuf>,
    revision: u64,
}
```

`DocumentState` owns bounds, canonical sources, selection, active layer, tool, current RGB, and current relief. Keep it private and equality-comparable so `is_dirty()` means `state != saved_state`, including after undoing back to the saved state.

- [ ] Implement document construction and source management:

```rust
pub fn new(bounds: Bounds, initial: CanonicalView) -> Self;
pub fn from_model(model: DepthSpriteModel, path: Option<PathBuf>) -> Result<Self, EditorError>;
pub fn to_model(&self) -> Result<DepthSpriteModel, EditorError>;
pub fn sources(&self) -> impl ExactSizeIterator<Item = &SourceSprite>;
pub fn source(&self, view: CanonicalView) -> Option<&SourceSprite>;
pub fn add_source(&mut self, view: CanonicalView) -> Result<(), EditorError>;
pub fn replace_source(&mut self, source: SourceSprite) -> Result<(), EditorError>;
pub fn remove_source(&mut self, view: CanonicalView) -> Result<(), EditorError>;
pub fn resolved_charts(&self) -> Result<Vec<Chart>, EditorError>;
```

Validate dimensions against the canonical view and model bounds. Store sources once, in canonical order; do not store fallback copies in `DocumentState`.

- [ ] Implement `opposite(view)` and resolve every absent opposing side from its present counterpart. A present target source always wins. The result can contain up to six derived `Chart`s, but only the one-to-six authored sources serialize.

Run:

```bash
cargo test -p editor-core
```

Expected: document and fallback tests pass.

- [ ] Commit:

```bash
git add Cargo.toml Cargo.lock crates/editor-core
git commit -m "feat: add editable sprite model document"
```

## Task 3: Implement exact layer editing, color selection, and history

**Files:**

- Modify: `crates/editor-core/src/document.rs`
- Create: `crates/editor-core/src/edit.rs`
- Create: `crates/editor-core/src/history.rs`
- Test: `crates/editor-core/tests/editing.rs`
- Test: `crates/editor-core/tests/history.rs`

- [ ] Write failing tests proving:

  - color pencil changes RGB and preserves alpha;
  - depth pencil changes alpha to `255 - relief` and preserves RGB;
  - depth pencil on an empty pixel adds geometry;
  - depth eraser writes alpha zero and preserves RGB;
  - color eraser is unavailable and cannot mutate a pixel;
  - color and depth fills flood only a contiguous region whose active-layer value matches the seed;
  - color eyedropper sets current RGB; depth eyedropper selects relief or the explicit empty state;
  - a drag containing several pixel writes is one undo step;
  - fill, add, replace, and remove are each one undo step;
  - a new edit clears redo and every successful mutation advances the monotonic revision.

Run:

```bash
cargo test -p editor-core --test editing
cargo test -p editor-core --test history
```

Expected: failures because no edit commands exist.

- [ ] Expose focused command methods; UI widgets must not mutate source buffers directly:

```rust
pub fn begin_stroke(&mut self) -> Result<(), EditorError>;
pub fn pencil_pixel(&mut self, view: CanonicalView, x: u32, y: u32) -> Result<bool, EditorError>;
pub fn erase_pixel(&mut self, view: CanonicalView, x: u32, y: u32) -> Result<bool, EditorError>;
pub fn finish_stroke(&mut self) -> Result<bool, EditorError>;
pub fn cancel_stroke(&mut self);
pub fn fill(&mut self, view: CanonicalView, x: u32, y: u32) -> Result<bool, EditorError>;
pub fn eyedrop(&mut self, view: CanonicalView, x: u32, y: u32) -> Result<(), EditorError>;
pub fn undo(&mut self) -> bool;
pub fn redo(&mut self) -> bool;
```

Use one `DocumentState` snapshot per user command for the initial editor. During a stroke, mutate live and increment revision for visible feedback, but push only the pre-stroke state when the stroke finishes with a change. Cancel restores the pre-stroke state and increments revision once.

- [ ] Represent selected depth as an explicit enum so magenta/empty cannot be confused with a grayscale relief value:

```rust
pub enum DepthValue { Empty, Relief(u8) }
```

Pencil accepts only `Relief(0..=254)`. Eraser is the only painting path to `Empty`; eyedropper may select either. The desktop palette displays relief in eighth-pixel units and as `relief / 8.0` model pixels.

- [ ] Implement iterative four-neighbor flood fill bounded to the active source. Compare only RGB for color fill and only alpha for depth fill; preserve the other channel data.

Run:

```bash
cargo test -p editor-core
```

Expected: all edit and history invariants pass.

- [ ] Commit:

```bash
git add crates/editor-core
git commit -m "feat: add sprite color and depth editing"
```

## Task 4: Add deterministic orbit state and one-render preview invalidation

**Files:**

- Modify: `crates/editor-core/Cargo.toml`
- Create: `crates/editor-core/src/camera.rs`
- Create: `crates/editor-core/src/preview.rs`
- Test: `crates/editor-core/tests/preview.rs`

- [ ] Write failing tests showing:

  - dragging changes camera yaw/pitch without changing document state, dirty state, revision, or undo depth;
  - reset restores the default view;
  - requesting the same document revision, camera, and framebuffer size twice renders once;
  - several mutations before the next request cause exactly one new render;
  - an orbit change causes one new render but no document mutation;
  - editing either source of the two-source bowl changes the preview and preserves the recessed basin after save/reopen.

Run:

```bash
cargo test -p editor-core --test preview
```

Expected: failure because camera and preview ownership do not exist.

- [ ] Implement quantized camera state and conversion into the renderer's exact rational basis:

```rust
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct OrbitCamera {
    yaw_millidegrees: i32,
    pitch_millidegrees: i32,
    zoom_milli: u32,
}

impl OrbitCamera {
    pub fn drag(&mut self, delta_x: f32, delta_y: f32);
    pub fn zoom(&mut self, wheel_delta: f32);
    pub fn reset(&mut self);
    pub fn target_view(self) -> TargetView;
}
```

Clamp pitch short of the poles and zoom to a usable fixed range. Quantize before constructing `Ratio<i64>` coefficients so an equal camera key always means an equal render basis.

- [ ] Implement a preview cache keyed by `(revision, OrbitCamera, width, height)`. `frame()` resolves derived charts and calls `relief-render` only when the key changes:

```rust
pub struct PreviewCache { /* key, framebuffer, render_count */ }

pub fn frame(
    &mut self,
    document: &EditorDocument,
    camera: OrbitCamera,
    width: u32,
    height: u32,
) -> Result<&FrameBuffer, EditorError>;
```

The render count is exposed only under `cfg(test)` or through a narrow diagnostic accessor used by the cache test; it is not application UI.

Run:

```bash
cargo test -p editor-core --test preview
```

Expected: all preview, orbit, and bowl checks pass.

- [ ] Commit:

```bash
git add Cargo.toml Cargo.lock crates/editor-core
git commit -m "feat: add live orbit preview cache"
```

## Task 5: Add package lifecycle and lossless source PNG import

**Files:**

- Modify: `crates/depthsprite-format/src/lib.rs`
- Create: `crates/depthsprite-format/src/png_source.rs`
- Modify: `crates/editor-core/src/document.rs`
- Create: `crates/editor-core/src/io.rs`
- Test: `crates/depthsprite-format/tests/png_source.rs`
- Test: `crates/editor-core/tests/file_lifecycle.rs`

- [ ] Write failing tests for decoding RGBA, RGB, and indexed PNG input into raw RGBA; rejecting a wrong source dimension; opening without mutating an existing document on failure; saving and reopening the exact authored bytes; and marking the saved document clean.

Run:

```bash
cargo test -p depthsprite-format --test png_source
cargo test -p editor-core --test file_lifecycle
```

Expected: failure because public source-PNG decoding and document file lifecycle are absent.

- [ ] Add a general PNG decoder that uses PNG transformations to return normalized 8-bit RGBA without interpreting alpha as depth:

```rust
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[u8; 4]>,
}

pub fn load_rgba_png(path: impl AsRef<Path>) -> Result<RgbaImage, PackageError>;
```

- [ ] Add transactional document methods:

```rust
pub fn open(path: impl AsRef<Path>) -> Result<Self, EditorError>;
pub fn import_source_png(&mut self, view: CanonicalView, path: impl AsRef<Path>) -> Result<(), EditorError>;
pub fn save(&mut self) -> Result<(), EditorError>;
pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<(), EditorError>;
```

Complete all parsing and validation before replacing the current document or source. A successful save updates `path` and `saved_state`; a failed save changes neither.

Run:

```bash
cargo test -p depthsprite-format -p editor-core
```

Expected: all package and lifecycle tests pass.

- [ ] Commit:

```bash
git add crates/depthsprite-format crates/editor-core
git commit -m "feat: add model lifecycle and png source import"
```

## Task 6: Build the desktop shell and prove the workspace geometry

**Files:**

- Modify: `Cargo.toml`
- Create: `crates/desktop-app/Cargo.toml`
- Create: `crates/desktop-app/src/main.rs`
- Create: `crates/desktop-app/src/lib.rs`
- Create: `crates/desktop-app/src/app.rs`
- Create: `crates/desktop-app/src/layout.rs`
- Create: `crates/desktop-app/src/menu.rs`
- Test: `crates/desktop-app/tests/layout.rs`
- Test: `crates/desktop-app/tests/menu.rs`

- [ ] Add failing pure-layout tests for a 1600×1000 window and the enforced minimum window size. Assert:

  - menu is at the top;
  - tool palette is a narrow vertical column;
  - source area is exactly three columns by two rows in canonical order;
  - each card contains equal-sized color and depth rectangles with color above depth;
  - model width and height are each at least 3× every individual canvas;
  - extra width is assigned to the model before mini-canvases grow.

Add menu tests that map File/Edit/View labels to New, Open, Save, Save As, Quit, Undo, Redo, and Reset Model View.

Run:

```bash
cargo test -p desktop-app --test layout
cargo test -p desktop-app --test menu
```

Expected: Cargo reports that `desktop-app` does not exist.

- [ ] Add a binary using `eframe`/`egui` and `rfd`, with an optional `.depthsprite` path argument. `main.rs` only parses startup input and calls `run_native`; durable behavior remains testable in the library.

- [ ] Implement a deterministic layout calculation returning all major rectangles. Enforce a minimum native window size derived from the same constants used by the layout test. Do not make the model and source panels peer-sized.

- [ ] Implement the top menu and unsaved-change state machine:

```rust
enum MenuAction { New, Open, Save, SaveAs, Quit, Undo, Redo, ResetView }
enum PendingDestructiveAction { New, Open(PathBuf), Quit }
enum UnsavedChoice { Save, Discard, Cancel }
```

The Save/Discard/Cancel modal must complete or cancel the pending action. File errors appear as a dismissible modal and never replace the current document.

Run:

```bash
cargo test -p desktop-app --test layout --test menu
cargo check -p desktop-app
```

Expected: layout and menu tests pass and the desktop binary type-checks.

- [ ] Commit:

```bash
git add Cargo.toml Cargo.lock crates/desktop-app
git commit -m "feat: add desktop editor shell"
```

## Task 7: Build the vertical palette and paired source canvases

**Files:**

- Create: `crates/desktop-app/src/palette.rs`
- Create: `crates/desktop-app/src/canvas.rs`
- Create: `crates/desktop-app/src/source_grid.rs`
- Modify: `crates/desktop-app/src/app.rs`
- Test: `crates/desktop-app/tests/palette.rs`
- Test: `crates/desktop-app/tests/canvas.rs`
- Test: `crates/desktop-app/tests/source_grid.rs`

- [ ] Add failing tests for:

  - vertical tool ordering Pencil, Eraser, Fill, Eyedropper;
  - RGB swatch updates through direct channels and hexadecimal input;
  - the selected picker color is consumed by color pencil and fill;
  - relief values display both eighth-pixel units and model pixels;
  - magenta/black/white depth visualization at alpha 0/255/1;
  - shared color/depth coordinate mapping for the same source transform;
  - pointer interpolation paints every crossed pixel and one drag becomes one command;
  - only the next empty canonical position shows Add Sprite;
  - adding, importing/replacing, and removing use document commands;
  - card headers report fallback assignment and update when an override appears.

Run:

```bash
cargo test -p desktop-app --test palette --test canvas --test source_grid
```

Expected: failures because the widgets do not exist.

- [ ] Implement palette state as a view of `EditorDocument`, not a second source of truth. Use `egui`'s compact color editor for hue/saturation-value plus direct RGB controls, and parse/format six-digit hexadecimal through one tested helper. Arrange tools with `ui.vertical`.

- [ ] Implement one custom pixel-canvas widget with a shared `CanvasTransform` per source card. Render nearest-neighbor pixels and translate pointer locations to exact chart coordinates. Color view always shows stored RGB. Depth view maps:

```rust
fn depth_display(pixel: [u8; 4]) -> Color32 {
    if pixel[3] == 0 {
        Color32::MAGENTA
    } else {
        let relief = 255 - pixel[3];
        let gray = ((u16::from(relief) * 255) / 254) as u8;
        Color32::from_gray(gray)
    }
}
```

Use the same transform, hover coordinate, zoom, and pan for the paired canvases. Begin a document stroke on pointer-down, apply interpolated pixels while dragged, and finish it on pointer release.

- [ ] Implement source cards in canonical 3×2 slots with color above depth. Route add/import/remove through `EditorDocument`; never edit raw vectors from the widget. Import uses a file picker filtered to PNG.

Run:

```bash
cargo test -p desktop-app --test palette --test canvas --test source_grid
cargo check -p desktop-app
```

Expected: palette, canvas, and source-grid tests pass.

- [ ] Commit:

```bash
git add crates/desktop-app
git commit -m "feat: add sprite editing canvases and tools"
```

## Task 8: Integrate the dominant read-only model viewport

**Files:**

- Create: `crates/desktop-app/src/model_view.rs`
- Modify: `crates/desktop-app/src/app.rs`
- Test: `crates/desktop-app/tests/model_view.rs`
- Test: `crates/desktop-app/tests/application.rs`

- [ ] Write failing tests proving:

  - the framebuffer texture refreshes when document revision changes;
  - a drag over the model changes only `OrbitCamera`;
  - wheel input changes only preview zoom;
  - Reset Model View restores default camera;
  - pointer input over the model cannot create an undo entry or alter any RGBA pixel;
  - one UI frame with multiple edits requests one preview render;
  - a headless 1600×1000 `egui::Context` frame contains the top menu, vertical palette, dominant model area, and progressive 3×2 source grid with color above depth.

Run:

```bash
cargo test -p desktop-app --test model_view --test application
```

Expected: failures because the model widget is not integrated.

- [ ] Present `FrameBuffer` through one nearest-neighbor `egui` texture. Reuse its texture handle and update pixels only when `PreviewCache` returns a changed key.

- [ ] Route model drag and wheel input exclusively to `OrbitCamera`. Make no document command API available to `model_view.rs` beyond immutable document access needed for preview generation.

- [ ] Render the complete workspace in this fixed semantic order: top menu, vertical palette, model viewport, source grid, then transient modals. Ensure the app records computed layout metrics under `cfg(test)` so the realistic frame test proves ratios without screenshots or source-string assertions.

Run:

```bash
cargo test -p desktop-app
cargo run -p desktop-app -- --help
```

Expected: all headless desktop tests pass and the binary reports its accepted optional model path without starting the event loop.

- [ ] Commit:

```bash
git add crates/desktop-app
git commit -m "feat: integrate live read-only model viewport"
```

## Task 9: Prove the complete authoring workflow and document its use

**Files:**

- Create: `tests/editor_workflow.rs`
- Modify: `README.md`
- Modify: `docs/specs/depthsprite-app.md` only if implementation names need exact alignment; do not change approved behavior

- [ ] Add a workspace-level workflow test that opens the bundled two-source bowl, edits stored RGB under an empty depth pixel, paints depth to add that geometry, changes basin relief, renders before and after, undoes/redoes, saves to a temporary `.depthsprite`, reopens it, and proves exact raw RGBA plus the rounded recessed bowl render survive.

Run:

```bash
cargo test --test editor_workflow
```

Expected: failure until all public crate surfaces compose correctly; then pass without GUI or timing dependence.

- [ ] Update `README.md` with only the user-facing workflow:

```bash
cargo run -p desktop-app
cargo run -p desktop-app -- path/to/model.depthsprite
```

Explain that a `.depthsprite` is one model file containing one to six canonical RGBA PNGs; RGB is color, inverted alpha is inward relief at eight units per model pixel, alpha zero is empty, and a single source also supplies its missing opposite. Describe New/Open/Save, source add/import/remove, color-over-depth editing, the color picker, basic tools, and model orbit/zoom.

- [ ] Run focused and full validation:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
git status --short
```

Expected: formatting, linting, all tests, and release build pass; status lists only the intentional plan/spec/code/documentation changes that have not yet been committed.

- [ ] Inspect the built application once at the approved 1600×1000 minimum geometry. Confirm the menu is on top, tools are vertical, the model dominates, cards are 3×2, and every card stacks color above depth. Keep this inspection bounded; do not add capture infrastructure to the repository.

- [ ] Commit the workflow test and documentation:

```bash
git add README.md tests/editor_workflow.rs docs/specs/depthsprite-app.md
git commit -m "test: verify complete depthsprite authoring workflow"
```

## Plan Conformance Check

- [ ] Confirm every acceptance statement in `docs/specs/depthsprite-app.md` maps to a behavioral test or the single bounded visual inspection above.
- [ ] Search the implementation and documentation for displaced features and placeholders:

```bash
rg -n "TODO|TBD|Export PNG|sprite.?sheet|diagnostic|support overlay|release pipeline" Cargo.toml crates tests README.md docs/specs docs/plans
```

Expected: no implementation placeholders and no displaced feature paths. The plan's own constraint text may match the search and is not application behavior.

- [ ] Confirm type and responsibility consistency: raw RGBA exists once per authored source; relief is derived; fallbacks are derived; widgets issue document commands; model input owns camera only; package save serializes authored sources only.

