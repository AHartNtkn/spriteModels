# DepthSprite fallback and demonstration-asset design

## Outcome

DepthSprite must behave coherently when a model contains fewer than six authored
sprites and must ship examples that make its image-warp capabilities legible
without editing them first.

A single authored side supplies its opposite observation until the model contains
a distinct sprite for that opposite side. The bundled bowl clearly shows a
recessed basin, rounded exterior, and connected rim through its authored color and
relief. Repository-local authoring guidance makes those qualities repeatable.

## Authored charts and resolved observations

Authored charts are the PNG evidence stored in a `.depthsprite`. Resolved
observations are the canonical charts presented to the renderer. `relief-core`
owns the conversion between them.

`resolve_charts(authored)` validates that every authored canonical side is unique,
then visits the six canonical views in rank order. For each view it selects:

1. the chart explicitly authored for that view; or
2. the explicitly authored opposite chart when the requested view is absent.

The selected RGBA bytes are assigned to the requested canonical view. Canonical
placement supplies the required opposing orientation; the resolver does not
rewrite or flip the pixels. An explicit opposite chart therefore overrides only
its own derived observation.

Resolution returns a `ResolvedCharts` value whose contents are private and
canonical-order iteration is read-only. The reference renderer accepts
`ResolvedCharts`, not an arbitrary chart slice. Editor preview, fixture rendering,
and tests all cross this boundary, so raw authored charts cannot accidentally
bypass fallback behavior.

Duplicate authored views are a resolution error. Chart construction continues to
own pixel-count validity, and the model/package layer continues to own canonical
dimension validity. Resolution does not mutate the document or serialized model.

## Bowl construction

The bowl package contains exactly two authored charts:

- `top.png`, `32×32`;
- `front.png`, `32×12`.

The model bounds are `[32, 12, 32]`. The front image height is the actual exterior
silhouette height. At least one foreground texel occupies row 0 and row 11, so the
chart has no globally empty padding above or below the bowl.

### Top chart

The top mask is a centered circular domain. Its outer rim carries zero or near-zero
inward relief. Relief increases continuously across the inner wall and reaches its
maximum on the basin floor. Component-local normalized tent interpolation remains
the renderer's sole continuity rule.

The top RGB field uses a warm ceramic base with deterministic baked lighting:

- an upper-left highlight and lower-right falloff;
- a restrained darkening with basin depth;
- a distinguishable rim band;
- bounded channel values that preserve authored pixel-art colors.

The result remains legible when displayed as a source sprite and when transformed
in the model viewport.

### Front chart

The front mask is derived row by row from a vertical bowl cross-section. The rim
occupies row 0 at its full projected width. Successive rows narrow smoothly toward
the bottom, and the last row contains the bottom-center foreground texels.

For each occupied row, an elliptical horizontal cross-section determines inward
relief from the front registration plane. Because the horizontal radius changes
with row height, relief varies both horizontally and vertically. The near exterior
is shallow; points approaching the projected silhouette move farther inward.

The front RGB field uses the same light direction and ceramic palette as the top:

- the upper-left belly receives the strongest highlight;
- side and lower silhouette regions darken gradually;
- the rim remains identifiable against the exterior body.

Lighting is stored only in RGB. Alpha continues to encode relief exactly.

### Seam authority

The top rim and front rim are two observations of the same intended boundary. The
front row-0 cross-section uses the same horizontal radius and front-distance
calculation as the near half of the top rim. At an elevated oblique view, the two
charts therefore meet along the rim instead of being separated by transparent
rows.

Rendered connectivity is authoritative. A fixture test identifies the top and
front rim ownership regions and proves that their occupied output pixels touch
within one output-pixel neighborhood across the supported oblique sector.

## Example roles

`bowl.depthsprite` is the defining concavity and multi-chart seam demonstration.
`block.depthsprite` remains the flat transformed-sprite control. Together they show
the difference between ordinary oriented sprites and inverted-alpha relief without
introducing a second rendering model.

## Asset-authoring skill

The repository contains `.codex/skills/create-depthsprite-assets` with:

```text
SKILL.md
agents/openai.yaml
references/asset-principles.md
```

The skill triggers when an agent creates, repairs, or reviews DepthSprite source
charts, fixtures, or example packages. Its workflow is:

1. State the visual claim the asset must prove.
2. Allocate visible responsibilities among the minimum authored charts.
3. Derive every PNG dimension from canonical model bounds.
4. Construct masks from silhouettes and shared landmarks, with no global padding.
5. Derive relief from explicit horizontal and vertical cross-sections.
6. Add restrained baked RGB lighting that reveals the intended form.
7. Generate the package deterministically.
8. Inspect color, depth, and multiple rendered target views.
9. Reject disconnected seams, ambiguous curvature, fabricated coverage, or tests
   that merely preserve incumbent pixel values.

The reference file contains chart-axis conventions, alpha encoding, seam
construction, relief cross-section formulas, lighting heuristics, and the rendered
acceptance checklist. It points to the governing mathematical and application
specifications rather than restating their full contracts.

## Validation

### Fallback

- An asymmetric front chart resolves as both Front and Back with identical RGBA.
- Rendering from the front and rear camera sectors produces visible authored
  colors with the expected canonical orientation.
- A distinct Back chart replaces the derived Back observation while Front remains
  unchanged.
- A one-chart package saves and reopens with one authored chart and still resolves
  to two render observations.

### Bowl source evidence

- Bounds are exactly `[32, 12, 32]`; chart dimensions are `32×12` and `32×32`.
- Front rows 0 and 11 contain foreground; no row or column exists solely as canvas
  padding around the complete silhouette.
- Front relief has multiple values within representative rows and columns.
- Top relief increases from rim toward basin and preserves its circular component.
- Both charts contain a deliberate range of RGB values under one coherent light
  direction.
- Equal inputs regenerate byte-identical example packages.

### Rendered evidence

- The standard oblique bowl view contains top-basin, top-rim, and front-exterior
  ownership.
- Top and front rim coverage is connected.
- RGB variation makes the basin and exterior curvature visible without source
  edits or diagnostic overlays.
- Editor recoloring, relief editing, saving, and reopening preserve the rebuilt
  bowl and its connected rendered result.

### Skill

- Skill metadata passes the standard structural validator.
- A forward use of the skill on a concrete DepthSprite asset task produces
  canonical dimensions, cross-section-derived relief, coherent lighting, and
  rendered validation rather than source-only assertions.
