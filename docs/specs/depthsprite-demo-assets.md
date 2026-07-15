# DepthSprite demonstration assets and authoring guidance

## Outcome

The repository ships deterministic examples that make the direct image-warp model
legible without editing them first. Each asset has a distinct visual claim, uses
tightly bounded sprites, stores lighting in RGB, respects fixed-scale relief, and
is judged through both its source charts and rendered observations.

Examples use the core `AuthoredModel` and its `ResolvedCharts`; they do not define a
fixture-only chart interpretation or rendering path.

## Shared construction rules

- PNG dimensions come from canonical model bounds and contain no globally empty
  padding around the intended silhouette.
- Empty pixels are `[255, 0, 255, 0]`.
- Nonempty alpha encodes inward relief at eight units per model pixel.
- Relief never exceeds the side's maximum inward depth: half the opposing model
  dimension, or four times that dimension in relief units.
- Cross-sections determine relief in both source axes wherever the intended form
  curves in both axes.
- One upper-left-front light direction controls all baked RGB shading in an asset.
  The response is deliberately high-contrast and saturated, but curved forms keep
  a graduated value ramp with enough intermediate values to expose their changing
  slope. Lighting never changes alpha or adds a renderer lighting model.
- Shared landmarks and rendered connectivity, rather than matching canvas extents,
  establish seams between charts.
- Equal generator inputs produce byte-identical `.depthsprite` packages.

## Lit flat block

`block.depthsprite` is the non-relief control. It contains six explicit canonical
charts so its world-fixed lighting remains coherent on every side. Every
foreground texel has zero relief. Top-facing surfaces are brightest, front and
left are intermediate, and surfaces facing away from the light are darker.
Restrained within-face gradients make orbit motion legible without suggesting
curvature.

The block proves canonical placement, explicit six-side assignment, backface
eligibility, and the visual difference between baked color and relief.

## Rounded bowl

`bowl.depthsprite` contains exactly two authored charts:

- Top-only, `32×32`;
- mirrored Front+Back, `32×12`, stored as one PNG.

Bounds are `[32, 12, 32]`. The front image height is the actual exterior silhouette
height. Foreground occupies its first and last rows, so no transparent strip can
separate the exterior from the rim.

The Top chart is a circular domain. Its outer rim has zero or near-zero inward
relief, the inner wall descends continuously, and the basin floor reaches the
greatest intended bowl depth without crossing the model midpoint. Its warm ceramic
RGB has an upper-left highlight, lower-right falloff, identifiable rim band, deep
shadow, and a strong continuous value ramp that makes the cavity readable in a
still image.

The Front mask is built row by row from a vertical bowl profile. The rim fills the
first row at full projected width; rows narrow smoothly to bottom-center foreground
on the final row. Each row has its own elliptical horizontal cross-section, so
inward relief varies horizontally and vertically. The front rim uses the same
radius and near-front landmark as the Top rim. Its opposite assignment uses true
midpoint-plane mirroring, so the baked directional gradient has the same world
orientation on both exterior observations rather than rotating with unchanged
raster coordinates. A standard elevated oblique render must show connected rim
ownership, a recessed basin, and a rounded exterior. No Bottom observation exists.

## Two-sided globe

`globe.depthsprite` has cubic bounds `[48, 48, 48]` and exactly two explicit charts:
Front and Back. Each chart contains a circular hemisphere. Relief is shallowest at
the projected center and increases radially to the maximum inward depth at the
silhouette band, so opposite surfaces meet without a gap and never cross.

The charts carry different but geographically corresponding continent and ocean
patterns, coherent high-contrast light gradients, latitude/longitude accents, and
a shallow inset feature kept inside the silhouette. Opposite target views must
visibly use their explicit chart. Oblique views must retain a continuous outline
without exposing either chart from behind.

## Six-sided gyroscope

`gyroscope.depthsprite` has bounds `[48, 48, 48]` and all six explicit canonical
charts. It depicts three differently colored nested gimbals in a deliberately
asymmetric set of tilts. Each chart is a separately authored observation with its
own projected ellipses, interruptions, overlap order, hub placement, and baked
lighting. A shared landmark table keeps ring identity, tilt direction, pivot
locations, and near/far ordering coherent across the six observations.

Every gyroscope side is separately authored. Front/Back, Left/Right, and Top/Bottom
views must show meaningfully different overlaps. Annular relief varies across ring
width and around visible arcs to give each band curvature. Transparent gaps between
rings remain empty. Oblique renders prove transient occlusion between eligible
charts while back-facing charts contribute nothing.

## Curved cloth tent

`tent.depthsprite` has bounds `[48, 28, 36]` and three authored PNGs: Front+Back,
Right+Left, and Top-only. The Front chart provides a peaked entrance, opening, and
hanging flap; Right provides a curved side wall; Top provides the ridge and roof
planes. There is no Bottom.

Relief uses broad roof curvature plus restrained ridge-to-eave sag, seam ridges,
and fabric folds rather than flat triangular fills. Stripes and stitching cross
those gradients under coherent baked lighting. The entrance remains alpha-empty,
the flap remains visibly separate, and shared ridge/eave landmarks connect across
standard elevated oblique views.

## Architectural dome

`dome.depthsprite` has bounds `[48, 32, 48]` and three authored PNGs: Front+Back,
Right+Left, and Top-only. The side charts provide the hemispherical profile, drum,
windows, and vertical ribs. Top provides the radial roof panels, crown, and
matching rib endpoints. There is no Bottom.

Cross-section relief produces a curved shell rather than a flat disk. Repeated ribs
and panel colors make registration errors visible. Standard oblique renders must
show a connected crown and drum, aligned ribs, readable windows, and no rear-facing
chart bleed.

## Repository-local asset-authoring skill

The repository contains:

```text
.codex/skills/create-depthsprite-assets/
├── SKILL.md
├── agents/openai.yaml
└── references/asset-principles.md
```

The skill triggers when creating, repairing, or reviewing DepthSprite sources,
fixtures, or examples. Its workflow is:

1. State the visual claim the asset must prove.
2. Assign the minimum suitable set of explicit canonical observations.
3. Derive raster dimensions and signed edge mappings from model bounds.
4. Construct tight masks and shared landmarks without global padding.
5. Derive relief from explicit cross-sections without exceeding maximum inward depth.
6. Add high-contrast baked RGB lighting with graduated form shading on curves.
7. Generate the package deterministically through the repository fixture path.
8. Inspect color, depth, and several rendered target views.
9. Reject disconnected seams, unintended opposite assignments, flat-looking
   curvature, rear-facing bleed, or source-only validation.

The reference explains canonical axes, synchronized dimension changes, alpha and
contact math, cross-section formulas, lighting heuristics, seam construction, and
rendered acceptance. It links to the governing model and editor specifications
instead of restating a competing contract.

## Validation

### Model and resolution

- Every package satisfies core bounds, canonical dimensions, uniqueness, and
  chart-specific relief limits.
- Every resolved side comes from an explicit single-side or opposite-pair assignment.
- The bowl resolves mirrored Front+Back and Top with no Bottom.
- A resolved chart contributes only where its locally transformed colored side
  faces the camera; opposing charts may contribute complementary regions in one
  frame.
- Save and reopen preserve only authored charts and reproduce the same observations.

### Source evidence

- The block has six explicit lit charts and zero relief everywhere.
- Bowl Front rows 0 and 11 contain foreground, its relief varies in representative
  rows and columns, both charts contain a strong graduated RGB range rather than a
  few flat bands, and the resolved Back is the exact canonical mirror of Front.
- Globe Front and Back are both explicit, geographically distinguishable, and
  reach the midplane at their silhouette bands.
- Gyroscope charts are all explicit, opposite pairs differ, rings remain annular,
  and transparent gaps remain empty.
- Tent and dome masks occupy their intended boundary rows and columns, contain
  two-axis relief variation, and share named landmarks between charts.
- Equal inputs regenerate byte-identical packages.

### Rendered evidence

- The block remains flat while its baked lighting makes orbit changes legible.
- The bowl contains basin, rim, and rounded exterior ownership with a connected
  rim, no internal Top/Front crossing, and no conical silhouette through orbit.
- Globe hemispheres meet without a transparent gap or backface contribution.
- Gyroscope opposite views show distinct overlaps and oblique views resolve ring
  occlusion without rear-facing bleed.
- Tent folds, opening, flap, ridge, and eaves remain readable and connected.
- Dome ribs, crown, windows, and drum remain readable and registered.
- Editing, synchronized resizing, saving, and reopening preserve valid rendered
  results and immediately invalidate the live preview.

### Skill

- Skill metadata passes the standard structural validator.
- A forward use on one concrete asset task produces canonical dimensions,
  in-range cross-section relief, coherent lighting, and multi-view rendered evidence.
