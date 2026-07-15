# Optional Opposite Mirroring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a durable mirror bit beside opposite-side reuse, make geometric mirroring exact for all canonical pairs, and use it for the bowl.

**Architecture:** `relief-core::Chart` owns the two assignment booleans. `AuthoredModel::resolve` is the only place that derives an opposite raster and mirrors complete RGBA texels when requested. Format, editor, UI, and fixtures preserve or mutate those authoritative bits; the renderer remains unchanged.

**Tech Stack:** Rust workspace, serde/JSON ZIP packages, egui, deterministic fixture generator, Rust integration tests.

## Global Constraints

- Do not create a worktree.
- New sources start with `opposite = false` and `mirror = false`.
- Disabling opposite reuse retains the dormant mirror bit.
- Replacement and import preserve both bits.
- The sole version-1 package schema requires `view`, `opposite`, and `mirror`; add no legacy detection or migration.
- Direct reuse remains available and unchanged.
- Mirroring transforms complete RGBA texels and is derived from canonical frames.
- Only the bowl changes from direct to mirrored opposite reuse.

---

### Task 1: Core two-bit assignment and exact mirror resolution

**Files:**
- Modify: `crates/relief-core/src/chart.rs`
- Modify: `crates/relief-core/src/model.rs`
- Test: `crates/relief-core/tests/authored_model.rs`

**Interfaces:**
- `Chart::supplies_opposite() -> bool`
- `Chart::mirrors_opposite() -> bool`
- `Chart::with_opposite_assignment() -> Chart`
- `Chart::without_opposite_assignment() -> Chart`
- `Chart::with_mirrored_opposite() -> Chart`
- `Chart::without_mirrored_opposite() -> Chart`
- `AuthoredModel::set_opposite_assignment(view, enabled)`
- `AuthoredModel::set_opposite_mirror(view, enabled)`

- [ ] **Step 1: Write failing core tests**

Add tests proving a new chart is false/false; disabling opposite retains mirror; direct reuse leaves RGBA unchanged; and mirrored resolution produces these exact outputs from asymmetric 2×2 input `[A,B,C,D]`:

```text
Front/Back and Left/Right: [B,A,D,C]
Top/Bottom:                [C,D,A,B]
```

Exercise both directions of all three pairs and use distinct RGB and alpha in every texel.

- [ ] **Step 2: Verify RED**

Run:

```bash
cargo test -p relief-core --test authored_model mirror
```

Expected: compilation or assertion failure because the mirror bit/API does not exist.

- [ ] **Step 3: Implement the two-bit chart state and resolver**

Add `mirrors_opposite: bool`, initialized false. Preserve both fields through resizing, RGBA replacement, and reassignment. Add one internal helper that copies both assignment bits to a rebuilt chart so mutation paths cannot diverge.

When resolving a mirrored opposite, build only the derived `Vec<[u8;4]>`:

```rust
let (source_x, source_y) = match source.view() {
    Front | Back | Left | Right => (width - 1 - x, y),
    Top | Bottom => (x, height - 1 - y),
};
```

The primary resolved chart always uses authored RGBA unchanged. Mirror has no effect unless opposite is enabled.

- [ ] **Step 4: Verify GREEN**

Run:

```bash
cargo test -p relief-core
```

Expected: all core tests pass.

### Task 2: One current package schema

**Files:**
- Modify: `crates/depthsprite-format/src/manifest.rs`
- Modify: `crates/depthsprite-format/src/load.rs`
- Modify: `crates/depthsprite-format/src/save.rs`
- Test: `crates/depthsprite-format/tests/package_roundtrip.rs`

**Interfaces:**
- `SourceV1 { view, opposite, mirror }`, with all fields required by serde.

- [ ] **Step 1: Write failing package tests**

Assert that false/false, true/false, and true/true sources round-trip exactly. Inspect `manifest.json` and require:

```json
{"view":"front","opposite":true,"mirror":true}
```

Do not add fixtures or branches for any other source schema.

- [ ] **Step 2: Verify RED**

Run:

```bash
cargo test -p depthsprite-format --test package_roundtrip
```

Expected: failure because the manifest still has only `symmetric`.

- [ ] **Step 3: Replace the manifest representation**

Rename the serialized assignment field to `opposite`, add required `mirror`, load both into the core chart, and save both from it. Keep manifest version `1`.

- [ ] **Step 4: Verify GREEN**

Run:

```bash
cargo test -p depthsprite-format
```

Expected: all format tests pass.

### Task 3: Editor commands, defaults, and source menu

**Files:**
- Modify: `crates/editor-core/src/document.rs`
- Modify: `crates/editor-core/tests/document.rs`
- Modify: `crates/editor-core/tests/history.rs`
- Modify: `crates/editor-core/tests/file_lifecycle.rs`
- Modify: `crates/editor-core/tests/model_resolution.rs`
- Modify: `crates/desktop-app/src/app.rs`
- Modify: `crates/desktop-app/src/source_grid.rs`
- Modify: `crates/desktop-app/tests/menu.rs`
- Modify: `crates/desktop-app/tests/source_grid.rs`

**Interfaces:**
- Replace `EditorDocument::set_source_symmetry` with `set_source_opposite`.
- Add `EditorDocument::set_source_mirror`.
- Source menu exposes `Also <Opposite>` and `Mirror Opposite` observations.

- [ ] **Step 1: Write failing editor tests**

Prove: new document false/false; mirror is one undoable command; turning opposite off leaves mirror true and removes only the resolved opposite; undo/redo restores each bit independently; replacement/import preserves both; reassignment preserves both.

- [ ] **Step 2: Write failing real egui tests**

Open the source menu, enable opposite, enable mirror, disable opposite, and assert that the mirror checkbox remains checked but disabled. Reenable opposite and assert the mirrored resolution returns without another mirror click.

- [ ] **Step 3: Verify RED**

Run:

```bash
cargo test -p editor-core
cargo test -p desktop-app source_menu
```

Expected: failures because mirror state and control do not exist and the new document currently enables opposite.

- [ ] **Step 4: Implement commands and UI**

Remove the initial forced opposite assignment. Preserve both bits through document replacement/import. Add the mirror command. In the side popover render Mirror Opposite directly below Also Opposite, using `add_enabled(supplies_opposite, Checkbox)` so disabling opposite never mutates the checked value.

- [ ] **Step 5: Verify GREEN**

Run:

```bash
cargo test -p editor-core
cargo test -p desktop-app
```

Expected: all editor and desktop tests pass.

### Task 4: Mirrored bowl and regenerated packages

**Files:**
- Modify: `crates/fixture-gen/src/bowl.rs`
- Modify: `crates/fixture-gen/tests/bowl_geometry.rs`
- Modify: `crates/relief-render/tests/compositing.rs`
- Modify: `crates/relief-render/tests/demo_acceptance.rs`
- Regenerate: `assets/examples/*.depthsprite`
- Modify: `README.md`
- Modify: `.codex/skills/create-depthsprite-assets/` only where package fields or mirror authoring are described.

**Interfaces:**
- Bowl Front uses `.with_opposite_assignment().with_mirrored_opposite()`.
- Bowl Top remains false/false.

- [ ] **Step 1: Write failing bowl tests**

Assert the authored Front is true/true, Top is false/false, resolved Back is the exact horizontal RGBA mirror of Front, and paired lighting samples at equal world positions match.

- [ ] **Step 2: Verify RED**

Run:

```bash
cargo test -p fixture-gen --test bowl_geometry
```

Expected: failure because the bowl has no mirror bit.

- [ ] **Step 3: Enable bowl mirroring and regenerate every package**

Run the repository fixture generator once so every example uses the required current manifest fields. Do not change other assets' assignment choices.

- [ ] **Step 4: Verify assets and rendered evidence**

Run fixture reproducibility and bowl render acceptance. Produce the existing focused bowl orbit/contact artifact and inspect both exterior sides for one coherent world-directed gradient.

### Task 5: Complete validation and conformance

**Files:**
- Update: `/tmp/sprite-models-foundation-20260714-32.md`

- [ ] **Step 1: Search for the displaced representation**

Run a repository search for `symmetric`, `set_source_symmetry`, and manifests lacking either required bit. Any remaining behavioral use is a failure; ordinary prose unrelated to assignment is not.

- [ ] **Step 2: Run full verification**

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo build --release
git diff --check
```

- [ ] **Step 3: Append conformance evidence**

Record implemented owners, deleted one-bit structures, migrated dependents, exact validation commands, package inspection, and bowl visual evidence without altering the pre-action foundation sections.
