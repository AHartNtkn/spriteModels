---
name: create-depthsprite-assets
description: Use when creating, repairing, reviewing, or validating DepthSprite PNG sources, deterministic example packages, relief geometry, seams, baked lighting, or rendered asset demonstrations.
---

# Create DepthSprite Assets

## Overview

Build each asset as a small set of editable oriented PNGs whose rendered result
clearly demonstrates a chosen form. Treat source assignment, relief construction,
color, and rendered inspection as one design problem.

Read [references/asset-principles.md](references/asset-principles.md) before changing
an asset. Use the repository specifications it links as the semantic authority.

## Workflow

1. State the visual claim in one sentence: what must be obvious in a still render?
2. Choose each stored PNG's primary side, **Also Opposite**, and **Mirror
   Opposite** values. Use an opposite pair only when the same image genuinely
   describes both sides. Enable mirroring when landmarks and baked lighting must
   retain their world registration across the pair.
3. Select tight bounds and derive every raster dimension from its canonical side.
4. Construct the silhouette and relief from explicit cross-sections. Check shared
   rim, ridge, eave, or seam landmarks in model coordinates.
5. Bake a coherent upper-left-front light into RGB. Use strong contrast and
   saturated material colors. On curved forms, keep the main light response
   graduated enough that changing slope remains visible; reserve hard bands for
   narrow accents, outlines, rims, and material changes.
6. Add formula-level source tests, explicit assignment tests, deterministic package
   tests, and multi-angle renderer tests before accepting the fixture.
7. Regenerate through `fixture-gen`, inspect color/depth sources and native-scale
   renders, and revise the asset when the intended form is not immediately legible.

## Acceptance evidence

| Concern | Required evidence |
| --- | --- |
| Ownership | Exact stored sources and resolved sides; absent sides remain absent |
| Geometry | Formula checks plus representative row and column relief variation |
| Contact | Shared landmarks and rendered connected ownership |
| Lighting | Large value range, many ordered intermediate values on curves, and visible highlight/shadow regions |
| Rendering | Front, opposite, edge-on, and elevated oblique views as applicable |
| Packaging | Byte-deterministic regeneration matching committed `.depthsprite` files |

## Common mistakes

- A few flat toon bands can satisfy color-count tests while hiding curvature.
  Validate ordered gradients and inspect the still render.
- A primary side does not imply its opposite. Set the source toggle deliberately.
- Direct opposite reuse and geometric mirroring are different. Check world-space
  landmark and lighting orientation before choosing the mirror bit.
- Matching image extents do not prove a seam. Compare shared model-space landmarks
  and rendered ownership.
- Source formulas alone do not prove the direct warp. Exercise folds, edge-on
  views, occlusion, and full-orbit silhouettes appropriate to the asset.
