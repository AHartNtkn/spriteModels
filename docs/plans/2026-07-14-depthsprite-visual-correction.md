# DepthSprite Visual Correction

## Outcome

Make the approved editor legible and useful in the native 1600×1000 window: the model is materially large, the vertical editing controls and color picker are visibly identifiable, source assignments fit their cards, and orbit/zoom presentation remains stable without changing model geometry.

## Binding design

- Render previews into a stable native inspection cell derived from registered model bounds. Its dimensions must not depend on the desktop viewport.
- Orbit changes orientation inside that cell. Presentation zoom does not alter projection geometry or cause preview regeneration.
- Present the native framebuffer centered in the model stage at an integer nearest-neighbor scale. Fit uses the largest integer scale within a modest margin; wheel zoom changes that integer presentation scale relative to fit; reset restores fit.
- Apply only incremental pointer motion while orbit-dragging. Do not reapply egui's cumulative drag delta.
- Preview identity must distinguish document replacement even when revision, orbit, and bounds happen to match.
- Replace unsupported tool glyphs and cryptic initials with visible stacked text labels: `Pencil`, `Eraser`, `Fill`, `Eyedropper`, `Color`, `Color Layer`, `Depth Layer`, and `Relief`. `Color` opens the existing basic RGB/hue-SV/hex picker. Keep the palette vertical and no wider than these short labels require.
- Use compact source assignment headers such as `Front → Back` and `Top → Bottom`. When the opposing source is authored, show only the authored side name.
- Preserve the top menu, dominant read-only model view, color-above-depth source canvases, progressive canonical 3×2 source grid, raw RGBA semantics, fallback ownership, package lifecycle, and renderer authority.
- Add no sidebar, overlay, diagnostics, export, release, archive-hardening, compatibility path, or alternate explanation surface.

## Implementation sequence

1. Write focused failing tests for native inspection-cell stability/containment, preview identity, presentation fit/zoom/reset, incremental drag, palette labels/layout width, and exact fallback headers.
2. Reconstruct camera and preview ownership so render geometry uses only registered bounds plus orbit, while presentation owns fit and zoom.
3. Reconstruct the model widget around centered integer-scaled nearest-neighbor presentation and incremental drag.
4. Replace palette and header copy, then migrate all derived layout and application tests.
5. Run focused tests, workspace fmt/clippy/tests/release build, and commit the complete correction.

## Decisive validation

- Focused tests prove viewport-independent native geometry, safe containment, presentation-only zoom/resize, reset-to-fit, incremental drag, fresh preview after document replacement, readable vertical controls, derived minimum layout, and exact arrow headers.
- Existing editing, package, renderer, workflow, and application tests pass.
- Root agent performs a hard-bounded native 1600×1000 capture and visually confirms a large centered bowl, readable controls, untruncated headers, color above depth, and no overlap or clipping.
