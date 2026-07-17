# Renderer Performance Plan

Goal: make `render_model` conceptually fast (old-hardware-viable) without changing
rendered output. Every task below must keep the golden-image tests bit-identical
(the one flagged exception is Task 5, which must escalate if goldens change) and
must keep `cargo test --workspace` green.

## Global Constraints

- **Pixel exactness:** `tests/golden_render.rs` hashes (RGBA + fragment owners,
  including exact rational depth) must be identical before and after every task.
  Do not regenerate `tests/golden/render_hashes.txt` to make a task pass.
- **No new dependencies** in library crates. Dev-dependencies only where stated.
- **No heuristics:** every constant or bound introduced must carry a principled
  justification in code (doc comment stating the derivation).
- **No dual systems:** each task fully replaces the code path it optimizes.
  Delete the old path; no fallbacks, no feature flags.
- **Benchmarks:** after the change, run `cargo bench --bench render` and report
  criterion's comparison against the previous run for every benchmark id.
- **Tests:** `cargo test --workspace` and `cargo test --test golden_render`
  must pass; report full command output summaries.
- Follow repository CLAUDE.md edicts (no silent failures, no placeholder code,
  comments only where the code cannot speak).

## Baseline (done by controller, not a task)

Criterion benches at workspace root (`benches/render.rs`) over fixture models
(globe, gyroscope) × orientations (front, default orbit 45°/35.264°, dragged
oblique) through public APIs, plus an orbit-sweep bench through
`editor_core::PreviewCache`. Golden hashes for all six fixture models × four
views in `tests/golden_render.rs` + `tests/golden/render_hashes.txt`
(regenerate explicitly with `GOLDEN_REGEN=1`). Flamegraph attribution recorded
in this file under Baseline Results.

## Task 1: Split orientation-independent preparation from per-frame rendering

Profiling shows ~40% of a default-orbit `render_model` call is spent inside the
per-chart preparation closure (`compositor.rs:214-230`): `ReliefField::new`
(including the `ComponentMap` BFS) and `PreparedRelief::new` (exact-rational
hat-kernel corner evaluation). None of that depends on the camera.

Introduce a public `PreparedModel` (name at implementer's discretion, matching
crate naming) built once from `ResolvedCharts`, holding per chart: the chart
reference/data, its `PreparedRelief`, and `maximum_relief`. `render_model`
becomes a function of (`&PreparedModel`, `&RenderRequest`) and computes only
the camera-dependent `WarpCoefficients`/`FacingCoefficients` per chart before
the pixel loop. Update `editor_core::PreviewCache` to cache the
`PreparedModel` keyed by (document identity, revision) — reusing it across
orientation changes — while keeping the existing frame-level cache behaviour.
Update the root `benches/render.rs`: the `render/*` benchmarks measure the
per-frame call with preparation hoisted out of the iteration, and a new
`prepare/globe` benchmark measures preparation alone. Delete the old
one-argument entry point; update all callers and tests.

**Exactness argument required:** identical values flow into the pixel loop;
only when they are computed changes. Golden hashes must match.

## Task 2: Eliminate hot-loop heap allocations

`crates/relief-render/src/compositor.rs` allocates per pixel × chart × segment:
`boundaries` Vec (solve_preimages), `preimages` Vec, `containing_cells` returns
Vec (two call sites), `quadrants_at` returns Vec, `roots_in_unit_interval`
returns Vec plus internal `partitions`/`quadratic_roots` Vecs.

Replace with: caller-provided scratch buffers reused across pixels (cleared, not
reallocated) for `boundaries`/`preimages`, and fixed-size stack arrays
(`[T; N]` + length, or `arrayvec`-style manual structs — no new deps) for the
bounded-cardinality results: `containing_cells` yields ≤ 4 cells,
`quadrants_at` yields ≤ 4 quadrants, a cubic has ≤ 3 roots, a quadratic ≤ 2,
partitions ≤ 4 boundaries. Each bound is a mathematical fact — document it at
the type.

**Exactness argument required in the task report:** identical floating-point
operations in identical order; only storage changes. Golden hashes must match.

## Task 3: Per-frame affine hoisting + streamed grid crossings

Currently `WarpCoefficients::inverse_line` solves a 2×2 exact-rational system
per pixel per chart (`crates/relief-core/src/warp.rs:76-131`), and
`add_grid_crossings` performs one f64 divide per half-texel line per pixel,
then sorts and dedups.

(a) The inverse-line coefficients are affine functions of (screen_x, screen_y):
the matrix part of the solve depends only on the chart and camera. Factor
`inverse_line` into a per-(chart, frame) solved form (pivot choice, inverted
2×2 minor, free-column coefficients — all computed once), leaving per pixel
only the substitution of screen_x, screen_y into affine rational expressions.
The per-pixel rational values must be exactly the values the current code
produces (same reduced rationals), so the downstream f64 conversion is
bit-identical.

(b) Grid crossings along the line are monotone in the parameter for each axis
(fixed slope sign). Replace collect-sort-dedup with a two-stream merge that
emits boundary parameters in sorted order directly, computing each crossing
with the *same* f64 expression the current code uses
(`(boundary - offset) / slope` with identical operand values), preserving the
existing epsilon-dedup semantics. No Vec, no sort.

**Exactness argument required:** same f64 crossing values, same order after
merge as after sort, same dedup decisions. Golden hashes must match.

## Task 4: Fixed-point common-denominator arithmetic

Per-pixel exact-rational work remains in: screen coordinate construction
(`compositor.rs:234-236`), the hoisted per-pixel affine substitution from
Task 3, `depth_at`, `FragmentKey` ordering (rational comparisons), and root
quantization to denominator 2^24.

All inputs have bounded power-of-two-friendly denominators: camera basis
denominator 1024, pixel centers denominator 2, root parameters denominator
2^24. Put each per-frame-constant quantity over one per-frame common
denominator and represent per-pixel quantities as wide integers (i128 where
the static bound requires it — derive and document the bit-width bound).
Comparisons become integer comparisons via cross-multiplication with
per-frame-constant denominators, or direct comparison when denominators are
shared. No gcd reduction anywhere in the per-pixel path.

Public API: `FragmentOwner::depth` remains `Ratio<i64>`, produced by exact
conversion at read time. Values must be equal as rationals to today's values
(golden hash includes them).

**Exactness argument required:** integer arithmetic computes the same rational
values exactly; f64 conversions receive identical rational inputs, hence
produce identical bits. Golden hashes must match.

## Task 5: Correctly-quantized root finding

`roots_in_unit_interval` bisects up to 56 iterations per sign-change bracket,
then the root is quantized to denominator 2^24 (`compositor.rs:632-634`).
Replace bisection-then-quantize with a solver that returns the provably
correctly rounded quantum directly: bracket via the cubic's critical points
(as today), converge with Newton iterations safeguarded by the bracket, and
finish by verifying the sign of the polynomial at the two neighbouring
half-quantum boundaries (2^-25 offsets) so the returned quantum q is proven to
contain the root. Iteration bound: Newton safeguarded by bisection halves the
bracket at worst, so ≤ 25 safeguarded steps reach quantum resolution — state
and enforce the bound, no magic numbers.

**Semantics note:** this defines the root as the correctly rounded quantum,
which the current 56-iteration bisection almost always but not provably
produces. Run the golden tests: if all hashes match, done. If any differ,
STOP, write both renders to PNGs, and report to the controller for user
escalation. Do not regenerate goldens.

## Task 6: Forward patch splatting

Replace the per-pixel outer loop (every pixel × every chart × full-ray grid
march) with forward traversal: for each chart, for each foreground quarter-cell
patch, compute the exact screen-space bounding region of the patch's swept
volume (patch rectangle × its relief interval under the affine warp — the
warp of a box is a zonotope; its screen bbox is exact and cheap: sum of
absolute column contributions). For each pixel center inside that bbox,
construct the same inverse line (Task 3 form), intersect only against this
patch using the same segment boundaries (the patch's own grid lines clip the
parameter interval via the same f64 expressions), the same cubic construction,
and the same root acceptance — producing identical `FragmentKey`s. The
z-buffer (`commit_fragment`) is order-independent for distinct keys, and equal
keys imply the same chart texel and thus the same color, so traversal order
cannot change the image.

Delete the old per-pixel outer loop entirely. A pixel touched by no patch bbox
must end transparent exactly as today (background never written).

Guard against double-committing the same root when a patch is visited once —
each (pixel, patch) pair is visited exactly once by construction, and roots on
shared patch boundaries must reproduce today's dedup semantics: today
boundaries are deduped per pixel across the whole ray with
COORDINATE_EPSILON; reproduce the same accepted-root set (state the argument
in the report; if today's epsilon dedup can merge roots from *different*
patches, handle that case identically — analyze before coding).

**Exactness argument required.** Golden hashes must match.

## Baseline Results

Measured 2026-07-16 on the development machine (release profile, criterion
sample size 20, single-threaded):

| benchmark                      | time (median) |
|--------------------------------|---------------|
| render/globe/front             | 79.1 ms       |
| render/globe/default_orbit     | 138.1 ms      |
| render/globe/oblique           | 133.4 ms      |
| render/gyroscope/front         | 202.7 ms      |
| render/gyroscope/default_orbit | 277.9 ms      |
| render/gyroscope/oblique       | 282.4 ms      |
| orbit_sweep/globe (8 frames)   | 1.134 s       |

perf attribution (globe/default_orbit, self time): `Ratio::reduce` 29%,
`Ratio::{mul,sub,add}` ~22%, `Ratio as Ord::cmp` 7%, `render_model` body 11%,
`PreparedRelief::sample_patch` 7%, malloc/free ~5%, sort ~2%,
`ReliefField::sample_terms_component` 2-6%. With children: the per-chart
preparation closure (ReliefField + ComponentMap + PreparedRelief) accounts for
~40% of the whole render — all orientation-independent.

## Per-Task Results

### Task 1 (prepare/render split) — commits 2f4eafb, bcd0d89

Controller-verified medians (vs baseline; render/* baselines included the
preparation that is now hoisted, orbit_sweep is apples-to-apples end-to-end):

| benchmark                      | baseline  | after     |
|--------------------------------|-----------|-----------|
| render/globe/front             | 79.1 ms   | 8.8 ms    |
| render/globe/default_orbit     | 138.1 ms  | 63.5 ms   |
| render/globe/oblique           | 133.4 ms  | 67.3 ms   |
| render/gyroscope/front         | 202.7 ms  | 83.6 ms   |
| render/gyroscope/default_orbit | 277.9 ms  | 160.5 ms  |
| render/gyroscope/oblique       | 282.4 ms  | 181.9 ms  |
| prepare/globe (once per edit)  | —         | 56.8 ms   |
| orbit_sweep/globe (8 frames)   | 1.134 s   | 0.594 s   |

Workspace tests green, goldens bit-identical, review approved.
