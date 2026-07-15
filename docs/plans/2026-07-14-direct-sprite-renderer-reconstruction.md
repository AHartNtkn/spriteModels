# Direct Sprite Renderer Reconstruction

## Outcome

Replace the chart-eligibility renderer with one direct transformed-image sampler.
Curved opposing sprites combine continuously through orbit, exact edge-on views
remain defined when relief has projected area, locally reversed image branches do
not expose their colored backside, and nearest transformed depth owns each output
pixel. Rebuild the bowl as a rounded, nonintersecting two-sprite asset.

## Selected model

For every resolved sprite, the renderer uses only:

```text
X(u,v) = O + Uu + Vv + N h(u,v)/8
```

For each output pixel it finds every source preimage of the projected mapping. A
foreground preimage supplies color when its local transformed colored side faces
the camera. The orientation is derived from the mapping itself:

```text
q(u,v) = N - U h_u/8 - V h_v/8
```

All valid samples compete by transient camera depth. There is no chart-wide
eligibility, culling mode, chart priority, mesh, solid, voxel field, or alternate
renderer.

The inverse sampler solves the two projected equations in `(u,v,h)`. The strongest
rank-two pair among `(u,v)`, `(u,h)`, and `(v,h)` parameterizes an affine line;
intersecting that line with the analytic relief patch finds ordinary, folded, tangent, and
canonical edge-on preimages. A flat exactly edge-on image has rank below two and
naturally has no projected area.

## Implementation sequence

### 1. Replace the behavioral contract with failing evidence

- Delete specification and tests that select sprites by canonical hemisphere.
- Add globe orbit tests at 89, 90, and 91 degrees. The 90-degree frame must be a
  connected full disc with ownership from both opposing source observations.
- Add direct-warp tests for a curved edge-on chart, a flat edge-on chart, locally
  reversed fold branches, tangent silhouettes, and deterministic equal-depth ties.
- Run focused tests and retain the expected failures before implementation.

### 2. Rebuild inverse-warp representation

- Replace `InverseWarpLine`, which parameterizes source only by relief after
  inverting the flat source basis, with a generalized affine line in `(u,v,h)`.
- Select the largest-magnitude rank-two column pair with a deterministic tie order and derive source,
  relief, and transient depth as affine functions of the remaining parameter.
- Migrate existing fold-root tests and add exact canonical edge-on coverage.
- Do not retain the old inverse as a fallback or compatibility path.

### 3. Rebuild transformed-image sampling

- Make `TargetView` compile every canonical frame to warp coefficients; delete
  `is_front_facing` and optional chart rejection.
- Extend analytic relief patches with local derivatives for every incident
  quadrant.
- Clip the line against source width, source height, and legal relief before
  splitting it at source-cell and tent-quadrant boundaries.
- Enumerate all preimages from the generalized line, preserving source texel and
  incident quadrant identity, and admit a sample precisely
  when at least one incident sector belongs to the closure of the locally
  front-facing locus.
- Preserve the existing global transient-depth framebuffer and deterministic
  source ownership after candidate validity.

### 4. Rebuild the bowl sources

- Define one exterior ellipsoid with row radius
  `R * sqrt(1 - (y/H)^2)` and derive Front relief directly from that row radius.
- Define a shallower concentric Top cavity with the same rim and `B < H`.
- Quantize with separation margin so the interpolated cavity remains strictly
  inside the exterior except at the rim.
- Regenerate `bowl.depthsprite`; replace monotonic/contact-only tests with nonlinear
  profile, bottom-depth, interpolated separation, and orbit owner-map tests.

### 5. Validate the reconstructed foundation

- Run focused core, renderer, globe, and bowl tests.
- Render fresh full-orbit contact sheets and inspect the globe at exact side view,
  the bowl silhouette, basin/exterior ordering, and owner continuity.
- Run `cargo fmt --all -- --check`, `cargo test --workspace`, and
  `cargo build --release`.
- Search for and remove the old canonical-normal rejection and single-owner globe
  expectation. Resume paused ambitious demos only after this foundation passes.

## Change isolation

The existing uncommitted dome, gyroscope, tent, and demo-acceptance work is paused
and preserved. Renderer reconstruction avoids overwriting those files except for
the narrow globe assertions already present in the same acceptance file; changes
there must be merged around the paused additions.
