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
correctly rounded quantum directly. The quantum that matters is the RAY
PARAMETER's (parameter = segment_start + span * unit_variable, quantized to
denominator 2^24 downstream) — NOT the cubic's unit variable: converging the
unit variable to 2^-25 is off by the segment span (median ~3, observed max
~192), which is what a first draft of this task tripped over. Bracket via the
cubic's critical points (as today), converge with Newton iterations
safeguarded by the bracket until the bracket MAPPED TO PARAMETER SPACE fits
within a half-quantum (2^-25) window, and verify by sign check at the
parameter-quantum boundaries (mapped back to the unit variable) that the
returned quantum q provably contains the root. Iteration bound: safeguarded
steps halve the bracket; the initial parameter-space bracket length is
bounded by the clipped parameter range (derive the static bound from the
Task 4 clip bounds), so the step bound is 25 + ceil(log2(max span)) — derive,
state, and enforce it, no magic numbers.

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

## Task 7: Corner-resolved bilinear relief (cubic -> quadratic)

USER-APPROVED SURFACE CHANGE (2026-07-16): unlike Tasks 1-6 this task changes
rendered output. The user accepts surface changes "within reason" and gets an
image-approval gate before goldens are regenerated.

Today the relief surface over each quarter-cell patch is the QUOTIENT of two
bilinear interpolants: relief(u,v) = weighted(u,v)/total(u,v), with
(weighted, total) hat-kernel corner pairs stored in `PreparedRelief`. The
ray-surface equation is multiplied through by total, making it cubic in the
ray parameter and forcing iterative root finding.

Change the surface definition: resolve relief to a single value per patch
corner at prepare time (value = weighted/total at that corner), and make the
patch surface the plain bilinear interpolant of those corner values. Then
with u,v affine in the ray parameter t, the intersection equation
relief_line(t) = value(u(t), v(t)) is QUADRATIC in t: closed-form roots, no
Lagrange fit, no iteration — the most old-hardware-friendly form. The facing
check uses the bilinear surface's gradient (quotient rule gone).

Analysis required before coding (state in the report):
- Corner-sample totals: prove or verify that total > 0 at every corner of
  every foreground cell's quadrants (the hat kernel includes the cell's own
  texel), so the corner division is always defined. If a zero-total corner is
  possible, define and justify the principled handling; no silent fallbacks.
- Which downstream semantics change (root count per segment, direct-hit and
  tangency cases, dedup) and which are preserved.
- The root solver: closed-form quadratic with the numerically stable
  formulation (avoid cancellation: q = -(b + sign(b) sqrt(disc))/2 form),
  reusing the existing quantization-to-2^-24 and acceptance semantics.

Approval gate (replaces the bit-exact golden constraint for this task only):
1. Implement; `cargo test --workspace` must pass except golden_render.
2. Render all 24 golden scenarios before/after (the golden test writes actual
   PNGs on mismatch; generate the "before" set from the parent commit).
3. STOP and report the image pairs to the controller. The controller shows
   the user. Only after explicit user approval: regenerate
   `tests/golden/render_hashes.txt` with GOLDEN_REGEN=1, inspect, commit.
4. If the user rejects, the task is abandoned and the branch reset to its
   parent commit.

Benchmarks after approval as usual.

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

### Task 2 (hot-loop allocations) — commits 9ffa8d7, 3ed2664

Controller-measured: run-to-run noise is ~±10% on this machine. Deltas vs
Task 1: within noise on render/globe/front; ~7-13% faster on the heavy
oblique scenes; orbit_sweep/globe ~-10% (0.594 s -> ~0.54 s). Consistent with
malloc/free's ~5% profile share; the structural value is an allocation-free
per-pixel loop for later tasks. Workspace tests green, goldens bit-identical,
review approved (fix: added Bounded capacity panic test).

### Task 3 (affine hoisting + streamed crossings) — commit 810b7b8

Controller-measured medians: globe/front 5.4 ms, globe/default_orbit 41.7 ms,
globe/oblique 47.3 ms, gyroscope/front 59.5 ms, gyroscope/default_orbit
101.2 ms, gyroscope/oblique 118.5 ms, orbit_sweep 453 ms. Roughly 25-35%
faster than Task 2 across the board. Reviewer independently re-derived both
exactness arguments (affine algebra term-by-term; merge order incl. -0.0/tie
cases). Workspace tests green, goldens bit-identical, review approved.

### Task 4 (fixed-point arithmetic) — commits 8435024, 81f8c78

Controller-measured medians: globe/front 3.0 ms, globe/default_orbit 30.5 ms,
globe/oblique 32.0 ms, gyroscope/front 53.7 ms, gyroscope/default_orbit
69.0 ms, gyroscope/oblique 73.3 ms, orbit_sweep 354 ms. Reviewer re-derived
all five exactness constraints; the reviewer's exhaustive editor-camera domain
sweep (no legitimate input trips the 2^53 certification asserts) was committed
as `editor_camera_domain_never_trips_fixed_point_certification` (42,768
combinations). Workspace tests green, goldens bit-identical, review approved.

### Task 5 (correctly-quantized root finding) — commit 58e7074

User-approved golden regen: 14 hashes changed, all RGBA byte-identical
(controller cmp-verified); only fragment depth rationals moved, 220 of
~140k roots by exactly +-1 quantum, each provably at a rounding boundary the
old bisection rounded arbitrarily. Solver: Newton safeguarded by bracket,
converging in ray-parameter space (span bound 252 and step bound 33 derived
from the clip bounds and enforced by always-on asserts). Reviewer re-derived
the sign-check proof and both bounds; approved, no blocking findings. Bench:
neutral within noise (globe/front -15%, rest +-3%); the payoff is deleting
the 56-iteration loop ahead of Tasks 6-7.

### Test/asset decoupling (user policy) — commit 8754f1d

User ruling 2026-07-16: no test may reference assets/examples — assets are
the user's artifacts, generators are disposable scaffolding, and nothing may
check one against the other. Deleted the asset<->generator coupling tests,
converted all behavior tests (renderer acceptance, editor preview/workflow,
CLI round-trip) to fixture-gen inputs. Workspace fully green; goldens
untouched.

### Task 5 (correctly-quantized root finding) — commit 58e7074

Respecified mid-task: the quantum is the RAY PARAMETER's (parameter = start +
span*unit), not the cubic unit variable's; first draft in unit space failed 20
goldens and was reworked. Final solver: Newton safeguarded by bisection,
convergence when the bracket's parameter image fits a 2^-25 half-quantum,
sign-verified quantum, span bound 252 and step bound 33 derived from the clip
bounds and enforced by always-on asserts. Goldens: 14 hashes changed —
byte-identical RGBA on all 24 scenarios (controller cmp-verified), only
fragment depth rationals moved by +/-1 quantum on 220 of ~140k roots, all at
rounding boundaries the old 56-iteration bisection rounded arbitrarily. USER
APPROVED the regeneration 2026-07-16. Bench: neutral vs Task 4 within noise
(same-session attribution: globe/front -15%, rest +/-3%). Review approved (no
Critical/Important findings).

### Interlude: test/asset decoupling — commit 8754f1d (user directive)

The user hand-patched assets/examples/bowl.depthsprite (a0f7143) mid-pipeline
and ruled: NO test may reference assets/examples; assets are user artifacts,
generators are disposable and must never be checked against assets. Deleted
the two asset<->generator coupling tests (bowl authority, reproducibility's
committed-asset half); converted all seven behavior tests (relief-render
acceptance/orbit, editor preview/workflow, e2e CLI) to fixture-gen model
inputs (file-I/O tests save the generated model to a tempdir first). Verified
divergence between patched asset and generator is exactly 30 Front-chart
relief texels no converted assertion touches. Workspace fully green.
