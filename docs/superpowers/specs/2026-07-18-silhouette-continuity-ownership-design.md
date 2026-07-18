# Silhouette-continuity surface ownership in mesh-import capture

Replaces the fabricated-wall cut pass and the sheet-blind parts of surface
ownership in `crates/mesh-import`. The `.depthsprite` format and the renderer
are untouched: a chart remains a tent-interpolated height field over
4-connected foreground texels, and disconnection is representable only as an
alpha-0 gap.

## The bug class this eliminates

Observed on the Stanford bunny fixture: spikes rising from the back into the
ear, and a seam ring in the back around the ear silhouette. Mechanism,
confirmed by texel-level diagnostics of the captured Top chart:

1. Per-texel best-facing ownership keeps two disjoint surface sheets (ear
   top, back top) 4-adjacent in the Top chart. The format defines 4-adjacent
   covered texels as one continuous surface, so the renderer fabricates a
   wall between them.
2. The fabricated-wall cut pass repairs this after the fact with two proxy
   tests, and each proxy's failure mode is one of the symptoms:
   - Candidate pairs are found by a relief-jump threshold
     (`CUT_CANDIDATE_UNITS`), and judged by whether the segment between the
     two samples passes within one texel of *any* mesh triangle. Beside the
     ear, that segment hugs the ear's own side wall, so fabricated ear-back
     adjacencies are judged "real walls" and survive: the spikes.
   - Where the cut does fire, the far texel is emptied — but ownership
     already told every other chart that Top owns that point, so nothing
     renders the dropped strip: the seam.
3. Closure dilation bridges sheets the same way (it dilates into any covered
   texel), relying on the cut pass to undo it.

The root cause is that connectivity between adjacent kept texels is decided
by proxies (thresholds, distance-to-any-mesh) instead of by the surface
itself, and that a chart forbidden from keeping a texel discards the surface
instead of handing it to another chart.

## Design overview

Three changes, all inside `mesh-import`:

1. **An exact per-pair continuity primitive** labels every 4-adjacency edge
   between covered texels of a side as *continuous* or *cut*, from the mesh
   itself. There is no global sheet segmentation — silhouette curves can
   terminate mid-surface (the ear merges smoothly into the head), so only
   the local edge labels are meaningful.
2. **Ownership enforces the chart invariant** — no chart keeps both
   endpoints of a cut edge — inside the ownership fixpoint, and a texel a
   chart may not keep is re-owned by the next-best observing side rather
   than discarded.
3. **Closure respects edge labels**, and a final sweep restores the
   invariant where dilation created new cut-edge adjacencies.

The emitted charts satisfy a checkable invariant: no two 4-adjacent covered
texels in any chart are joined by a cut edge.

## The continuity primitive

For side `S` and two 4-adjacent covered texels, consider the vertical plane
through the segment joining their centers (the plane containing the segment
direction and `S`'s view axis). The mesh's intersection with that plane is a
set of polylines in `(t, d)` coordinates — `t` the segment parameter
restricted to the strip `t in [0, 1]`, `d` depth along `S`'s axis. Both
sample points lie on these polylines by construction (each texel's winning
triangle covers its center, so its cross-section segment passes through the
sample).

The pair is **connected** iff both samples lie on the same polyline
component within the strip, where:

- Segments join into components where they share endpoints (a mesh edge
  crossing the plane). Endpoint gaps smaller than half a relief quantum
  (1/16 px) are treated as closed: a crack the encoding cannot even
  represent must not produce a cut. This constant is derived from
  `RELIEF_UNITS_PER_PIXEL`, not tuned.
- Every point of the connecting path must satisfy the side's reachability
  rule (post-quantization relief `<= h_max`, identical to the capture
  filter). A path dipping past the midplane belongs to the opposite chart;
  bridging over it would fabricate a roof.

A continuous path from `t = 0` to `t = 1` passes through every intermediate
`t` (it can fold back but never skip), so restriction to the strip is
well-defined.

The verdict is exactly "is the tent bridge backed by continuous, reachable
surface." Deliberately, occlusion of the in-between surface is irrelevant: a
bridge *behind* nearer geometry is harmless because transient depth draws
the nearer chart in front; a bridge through empty space is the artifact.
Consequences:

- Ear-over-back adjacency: the ear's cross-section is a closed loop separate
  from the back's polyline — **cut**. At the ear base, where the two
  surfaces genuinely join, the cross-sections connect — **continuous**,
  which is correct: that wall is real.
- A sub-resolution sliver occluding the middle of a same-surface pair, or a
  silhouette clipping the corner of the segment between two same-surface
  samples: the samples share a polyline — **continuous**. (A visible-depth
  formulation such as a lower envelope cuts both of these spuriously and
  perforates real surface; that formulation was considered and rejected.)
- Two towers with empty space between, and coverage holes mid-segment:
  different components — **cut**.

Triangles are gathered from per-side screen-space buckets (texel to indices
of triangles overlapping that texel's column), built once per side per
capture. Degenerate zero-area triangles are skipped and all triangles
participate regardless of facing, both matching the rasterizer.

## Ownership

Scoring (`sigma * (-n . axis_T)`), the candidacy conditions (same oriented
face, reach, in-bounds, visible within the gradient-derived tolerance), and
"a side is always a candidate for its own hit" are unchanged.

Ownership computes kept masks satisfying the invariant, with re-owning
instead of discarding:

1. **Initial masks**: each side keeps the texels where it is the
   best-scoring candidate (today's rule).
2. **Constraint enforcement**: for every cut edge with both endpoints kept
   in one chart, the **far** endpoint (deeper along that side's axis) yields
   and is *banned* for that side. The near texel's boundary is the true
   silhouette of its surface from this view; the far surface continues
   underneath, so its margin is exactly what other sides can still observe.
3. **Rescue**: side `S` keeps its covered, unbanned texel iff no
   strictly-better-scoring candidate side *currently keeps* that surface
   point. "Currently keeps" projects the point into the candidate chart and
   compares against its kept depth using the existing condition-4 tolerance
   machinery. When Top is banned from the strip beside the ear silhouette,
   the strip's samples in the side/back charts stop deferring and become
   kept.
4. **Fixpoint**: rescued texels can create new both-kept cut edges in their
   own chart, triggering new bans and rescues. Bans only accumulate and each
   surface point only moves down its preference order, so the loop
   terminates. Within each round, "who keeps what given the bans" is
   resolved by a single sweep in descending score order, making the result
   deterministic and independent of side iteration order.
5. **End state**: every observed surface point is kept by the best side that
   can legally hold it. A point is kept nowhere only if no enabled side
   observes it — the format's by-construction loss, which the import dialog
   exists to show.

Constraint enforcement never fires on a continuous edge, so mid-surface
ownership handoffs (the back transitioning from Top-owned to Back-owned as
the normal swings) are untouched; those seams remain closure's job.

## Closure and the post-closure sweep

Closure keeps its purpose — one texel of tent support so abutting regions of
different charts meet without sub-texel gaps — with two restrictions:

1. A kept texel dilates into a 4-neighbor only if the neighbor is covered
   **and the connecting edge is continuous**.
2. Dilation never adds a **banned** texel, even via a continuous edge from
   another direction; re-adding it would recreate the wall its ban removed.

Dilation can still create new cut-edge adjacencies (support added beside a
kept texel of another surface, typical along staircase silhouettes). A final
sweep enforces the invariant on the outgoing masks: for any cut edge with
both endpoints present, drop the support (dilated) endpoint if exactly one
endpoint is support; drop the far endpoint if both are. Two kept endpoints
cannot occur — the fixpoint terminated with none and closure adds no kept
texels.

Dropping support never loses surface, by construction: a covered, unbanned,
unkept texel exists only because a strictly better side currently keeps its
point (that is the fixpoint condition), so support is always redundant
geometry. The sweep trades at worst a sub-texel closure gap for not
fabricating a wall.

`convert` debug-asserts the final invariant on every emitted chart.

## Module changes

- **Delete `cuts.rs`** entirely: `CUT_CANDIDATE_UNITS`,
  `apply_fabricated_wall_cuts`, `TriangleGrid`,
  `closest_point_on_triangle`, `within_one_texel`. No shims; callers are
  rewritten. Tests of the deleted pass are removed with it.
- **New `continuity.rs`**: per-side triangle buckets; cross-section
  component search; public surface is bucket construction plus the edge
  label map for a side.
- **`capture.rs`**: `owning_mask` becomes the fixpoint above;
  `dilate_keep_mask` takes edge labels and the ban set; the post-closure
  sweep and the invariant assertion run before chart encoding.
- **Docs**: the import design doc's "Surface ownership" and
  "Fabricated-wall cuts" sections are rewritten to this model.

## Testing

Bug-first: each behavioral test is written to fail on the current
implementation where the current implementation is wrong.

1. **Continuity unit tests** on synthetic geometry: silhouette pair (two
   overlapping quads) cut; fold sharing a mesh edge connected;
   sub-resolution sliver over a same-surface pair connected; corner-clip
   beside a silhouette connected; two towers cut; groove dipping past reach
   cut; hairline crack below half a quantum connected.
2. **Ownership and rescue** on a synthetic tab-over-plane scene: top chart
   keeps the tab with silhouette intact and stops one texel short on the far
   side; the relinquished ring is kept by the side chart; and the coverage
   property — every pre-ownership covered sample observed by at least one
   candidate side is represented in some emitted chart within the derived
   tolerance. The coverage property is the seam bug's test and fails today.
3. **Invariant property test on real fixtures** (bunny, dragon, teapot,
   earth; several bounds and rotations): no emitted chart contains a
   4-adjacent covered pair labeled cut, plus the existing format
   invariants. This is the spike bug's test and fails today on the bunny's
   Top chart at the ear staircase.
4. **Fixpoint behavior**: a staircase silhouette configuration exercising
   ban, rescue, and cascading bans; assert termination and iteration-order
   independence.
5. Existing capture, registration, and dialog tests stay green.
6. **Benchmark**: extend the capture bench; compare against the ~21 ms
   interactive budget on the bunny and the 100k-triangle dragon.

Acceptance beyond tests: import the bunny in the dialog — no spikes from the
back into the ear, no seam ring around the silhouette, relinquished back
coverage rendered by the side/back charts. Rendered-image approval is the
gate; byte-level differences from today's output are expected.
