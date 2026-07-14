# Oriented Relief Sprite Renderer Design

Date: 2026-07-13

Status: Implemented and validated reference design

## Outcome

Build a native Rust desktop tool that opens one portable model file containing a bundle of canonical color-plus-depth PNG sprites, directly warps those images to create a rotatable pseudo-3D pixel-art impression, and exports deterministic directional PNG sprite sheets.

The application does not reconstruct or render an authoritative mesh, solid, voxel volume, signed-distance field, or inferred hidden surface. Its authoritative model remains the source images and their canonical placement.

The decisive proof is a bowl described by two sprites: a top sprite whose relief field contains the rim and recessed basin, and a side sprite whose mask and relief contain the rounded exterior/profile. The bowl must read correctly over the angular sector described by those two sprites.

## Initial Scope

The first release includes:

- Native desktop support for Linux, Windows, and macOS.
- Single-file model open, save, and save-as.
- Canonical front, back, left, right, top, and bottom relief sprites; any nonempty subset is allowed.
- Orbit, zoom, reset, and fixed isometric/directional viewport presets.
- Transparent directional PNG sprite-sheet export with explicit direction count, elevation, integer scale, padding, and frame order.
- A two-sprite bowl model and a second, nonconcave reference model.
- Diagnostics for invalid packages, unsupported viewing regions, chart overlap, and depth conflicts.

The first release excludes:

- Painting, erasing, selection, extrusion, or other in-application image editing.
- Animation and GIF export.
- OBJ, glTF, or other mesh export.
- Web deployment.
- Per-model or per-view depth calibration.
- Loose multi-file model loading as an alternative authority.
- Geometry completion, symmetry completion, inpainting, or hidden-surface synthesis.

## Core Representation

The model is an **oriented relief-sprite bundle**. Each sprite is an independent image chart with:

- A canonical direction and fixed plane placement.
- A foreground mask derived from alpha.
- Immutable source RGB.
- Exact fixed-point relief samples derived from alpha.
- A stable chart rank used only for exact compositing ties.

Charts are never fused or connected to one another. A chart may define continuous relief inside its own foreground domain, but no rule joins the top chart of an object to its side chart or fills space between them.

### Alpha standard

For source alpha `a`:

```text
a = 0       => background; no chart sample
a in 1..255 => valid foreground sample
relief(a)   = (255 - a) / 8 source pixels
```

All geometry-related arithmetic represents relief in integer eighth-pixel units:

```text
relief_eighths(a) = 255 - a
```

The valid range is 0 through 254 eighth-pixels, or 0 through 31.75 source pixels. This conversion is global and immutable. The package contains no scale, depth-range, or calibration override.

Source alpha is not opacity. A winning foreground fragment becomes fully opaque in an exported sprite; an uncovered output pixel is transparent.

### Canonical placement

The package manifest declares integer model bounds `[width, height, depth]` in source pixels. Bounds register the sprite planes and do not alter relief interpretation.

Each canonical view uses one pixel per model-space pixel and has dimensions implied by the bounds:

- Front/back: `width × height`.
- Left/right: `depth × height`.
- Top/bottom: `width × depth`.

The format fixes axis direction, image origin, center sampling, handedness, view flips, and inward relief direction. Every chart begins on its corresponding bounding plane, and positive relief moves inward from that plane. These are signed-axis permutations and translations, not user-controlled transforms.

The bounds are registration scaffolding for the image bundle, not an occupied box. No material is implied inside them.

## Direct Relief Warp

For chart `i`, let:

- `p = (x, y, 1)` be a homogeneous source-image coordinate.
- `h_i(p)` be its reconstructed relief in eighth-pixel units.
- `H_i,V` be the flat-sprite transform from chart `i` to target view `V`.
- `e_i,V` be the target-view displacement produced by one eighth-pixel of inward relief.

The target-image warp is:

```text
W_i,V(p) = H_i,V p + h_i(p) e_i,V
```

For orthographic and isometric views this is an affine image transform plus a depth-proportional 2D displacement. It can be evaluated without constructing world geometry. Perspective support is outside the first release.

A corresponding transient compositor depth is:

```text
Z_i,V(p) = g_i,V · p + gamma_i,V h_i(p)
```

`Z` exists only while producing a target image. It is not saved as geometry or used to infer occupancy.

### Relief continuity inside a chart

Each four-connected foreground component has a continuous, exact relief interpolant. Let `K` be its foreground texel centers, let `h_k` be the encoded relief at center `k`, and define the compact bilinear tent kernel:

```text
phi(dx, dy) = max(0, 1 - |dx|) × max(0, 1 - |dy|)
```

For a point `p` in the union of the component's half-open unit texel cells:

```text
h(p) = sum(k in K, h_k × phi(p - k)) / sum(k in K, phi(p - k))
```

The denominator is positive everywhere in that domain because every point lies within half a pixel of at least one valid center. The rule is exact at every texel center, continuous through ordinary gradients and one-pixel-wide regions, and terminates at the alpha-zero mask boundary. Samples from another four-connected component or another chart never enter the sum.

The numerator, denominator, and division use specified fixed-point/rational arithmetic in authoritative exports. This is a normative image-resampling rule. It is not stored as a mesh, and it does not create a backside, thickness, closed volume, or connector between separate sprites.

Source RGB is not interpolated with relief. Each transient microcell has the immutable source texel containing its strictly interior center. Both triangles carry that texel's RGB and owner; barycentric interpolation is used only for transient depth. Exact output-edge ownership follows the top-left coverage rule, never a second nearest-color search. Pixel-art colors therefore remain authored values.

An artist who needs an actual discontinuity must separate the regions with alpha-zero pixels or place them in separate charts. Adjacent nonzero-alpha pixels intentionally mean continuous relief.

## Sampling and Compositing

For each output pixel center and each eligible front-facing chart:

1. Find every source preimage whose relief warp covers the output sample.
2. Account for folds in the warp by retaining every valid preimage rather than assuming the mapping is one-to-one.
3. Compute transient `Z` for each candidate.
4. Select the lexicographically smallest key `(Z, chart_rank, source_y, source_x)`.
5. Copy the winning source texel's RGB unchanged and emit output alpha 255.
6. Emit transparent black when there is no candidate.

True projected visibility may change as the camera moves. Persistent ownership may not: the same chart location always carries the same source RGB. The renderer never chooses a texture based on camera angle, blends between canonical images, or applies temporal ownership heuristics.

The stable tie key acts as an exact symbolic infinitesimal ordering. It replaces floating-point depth bias and prevents coplanar overlap from flickering between source images.

Charts are one-sided. Rendering a chart from behind would display color that was never authored for that direction. At edge-on orientation its projected support collapses naturally.

Unsupported disocclusion remains transparent. The renderer does not dilate silhouettes, stretch edge colors, create skirts, add sidewalls, or inpaint missing pixels. Additional authored charts are the only way to expand reliable angular coverage.

## Two-Sprite Bowl

The proof model contains:

- A top chart with a circular foreground domain. The rim has relief near zero; the inner wall increases continuously in relief; the basin floor has the largest relief.
- A front or side chart whose silhouette forms the outer bowl profile and whose relief gradient suggests curvature across the visible exterior.

For an elevated oblique target view:

- The top chart's interior moves farther along its parallax direction than the rim, producing a visible recession.
- The normalized tent relief keeps the inner slope continuous rather than opening cracks between depth samples.
- The side chart provides the rounded exterior pixels absent from the top observation.
- Transient depth lets the nearer rim/exterior hide the appropriate part of the basin.
- Stable ownership prevents the rim from switching color sources during orbit.

The acceptance claim is deliberately limited to the supported viewing sector. Two images do not determine the bottom, rear exterior, or every possible orbit angle, and the renderer does not invent them.

The bundled `bowl.depthsprite` proves this claim with one coherent camera basis rather than unrelated chart snapshots. The acceptance camera uses screen-right `(1/2, 0, 1/2)`, screen-down `(1, 1/2, -1)`, and transient depth `(-1, 4, 1)`. At 96 by 96 output pixels, the front-frame sample at `(48, 67)` is owned by front texel `(27, 2)` with relief 40 and RGB `[144, 76, 52]`; the top-frame sample at `(48, 48)` is owned by top texel `(16, 16)` with relief 64 and RGB `[216, 156, 85]`. The corner remains transparent, directly demonstrating that unsupported space is not completed.

## Model Package

The model extension is `.depthsprite`. Its contents are a comment-free canonical ZIP32 profile:

```text
manifest.json
views/front.png
views/back.png
views/left.png
views/right.png
views/top.png
views/bottom.png
```

Only entries declared by the manifest may be present. Absent views are omitted. Version 1 accepts one through six charts, dimensions no larger than 512 pixels on either axis, a package no larger than 65 MiB, and aggregate compressed and expanded payloads no larger than 64 MiB each.

The version 1 manifest contains:

```json
{
  "format": "depthsprite",
  "version": 1,
  "bounds_pixels": [32, 16, 32],
  "views": ["front", "top"]
}
```

The archive stores source evidence, not derived rendering caches. Saving uses canonical entry order, normalized timestamps and permissions, stable JSON formatting, fixed PNG settings, and atomic sibling-file replacement. Loading rejects absolute paths, parent traversal, duplicate entries, unsupported compression, undeclared files, excessive expanded size, excessive dimensions, and malformed manifests before decoding images.

PNG inputs must decode to 8-bit, nonpremultiplied RGBA without color conversion affecting alpha. Dimensions must match the manifest bounds for their view. RGB under alpha zero is ignored and normalized to black when a package is saved.

Normal model use opens and saves one `.depthsprite` file. The first release does not expose six independent open slots. Bundled fixtures and documented archive structure provide initial assets; a package-creation assistant can be designed separately without becoming an alternative runtime model loader.

## Application Architecture

The Rust workspace has one dependency direction toward the mathematical core:

```text
desktop shell → document/application service → package I/O
              ↘ preview/export orchestration → relief-warp core
```

### `relief-core`

Owns fixed-point coordinates, canonical chart frames, alpha decoding, foreground components, the normative relief interpolant, direct warp equations, transient-depth candidates, stable ownership, and reference compositing. It has no GUI, filesystem, ZIP, or GPU dependency.

### `depthsprite-format`

Owns manifest schema, safe bounded archive reads, PNG validation, canonical archive writes, schema migration to the current version, and atomic persistence. It returns validated source charts and never interprets rendering behavior independently.

### `relief-render`

Owns fixed target-view presets, deterministic reference rendering, output framing, transparent PNG encoding, and sprite-sheet packing. Exports always use the reference compositor.

### `desktop-app`

Owns the native window, menu commands, file dialogs, document dirty state, viewport controls, diagnostics, background work, and framebuffer presentation. The GPU may accelerate preview, but it consumes the same chart/warp contract and is never authoritative for exported pixels.

Long work runs against immutable document snapshots. Preview requests replace any queued intermediate request, and only the latest generation may install its result. Export requests reject concurrent work and propagate render diagnostics with the finished sheet.

## User Experience

The main window contains:

- A central pixel-scaled viewport.
- A compact document panel showing package name, bounds, included canonical charts, and validation state.
- Orbit drag, zoom, reset, free-view lock, and fixed top/front/side/isometric controls.
- A coverage diagnostic that distinguishes ordinary transparency from regions unsupported at the current angle.
- An export panel for 8 or 16 evenly spaced directions, fixed elevation, integer output scale, cell padding, and sprite-sheet order.

Opening a package either installs one fully validated document or leaves the current document unchanged. Errors identify the archive entry, canonical view, and pixel coordinate when applicable.

Saving writes one package. Directional export writes one transparent PNG sprite sheet. These operations are visually and terminologically distinct.

## Error Semantics

Fatal load errors include unsafe archive structure, unsupported schema, missing or contradictory manifest entries, invalid PNG type, dimension mismatch, and resource-limit violations.

Nonfatal model diagnostics include:

- Relief extending beyond the opposing canonical plane.
- Heavy overlap between charts.
- Exact-depth color disagreement at coincident coverage.
- A requested export direction with insufficient authored coverage.
- A warp fold that maps several source regions to the same output area.

Diagnostics never silently modify the source images. Export may proceed through nonfatal warnings because incomplete coverage is an honest property of the image bundle.

## Determinism

Authoritative export uses:

- Integer eighth-pixel source coordinates.
- Versioned rational camera bases for directional presets.
- Fixed pixel-center and half-open coverage rules.
- Fixed-point transient depth and stable lexicographic ties.
- Immutable microcell-center source RGB and ownership.
- A CPU reference compositor.
- Fixed frame order, bounds, padding, and sheet packing.
- Canonical PNG encoding without time-dependent metadata.

Free-orbit preview may use floating-point camera input and GPU rasterization. It is an inspection aid and may differ at subpixel boundaries; selecting a fixed preset must reproduce the reference export framebuffer.

## Validation

### Mathematical tests

- Alpha 255 maps to zero relief; alpha 1 maps to 254 eighth-pixels; alpha 0 creates no sample.
- Constant relief produces the flat transform plus a constant parallax translation.
- Zero relief reduces exactly to the flat-sprite transform.
- Mirrored canonical views apply the correct signed-axis permutations.
- Foreground interpolation passes through every encoded depth sample.
- No interpolation crosses an alpha-zero boundary or chart boundary.
- A smooth relief ramp produces no internal coverage holes in supported views.
- A fold retains all preimages and selects the nearest transient depth.
- Exact overlaps always choose the same stable owner across camera motion.
- Every visible fragment retains its source RGB under orbit.

### Package tests

- Valid packages round-trip to byte-identical canonical archives.
- Entry order and input ZIP metadata do not change canonical output.
- Canonical output has the exact manifest/view allowlist, ordinary ZIP32 records, no archive or entry comments, no extra fields, fixed timestamps and permissions, and no trailing data.
- Traversal, duplicate entries, undeclared entries, compression bombs, malformed JSON, oversized images, and dimension mismatches are rejected.
- Saving through interruption cannot replace a valid package with a partial file.
- No derived mesh, volume, or renderer cache appears in the saved schema.

### Render and product tests

- Golden outputs for flat, stepped, recessed, overlapping, and disoccluded charts.
- Repeated directional exports are pixel-identical and byte-identical.
- The two-chart bowl's exact acceptance pixels prove a continuous recessed basin, visible near rim, and rounded exterior under one coherent oblique camera.
- The bowl exposes transparent unsupported regions rather than fabricated bottom or rear surfaces.
- A nonconcave fixture verifies ordinary transformed-sprite behavior was not regressed.
- Native scripted use opens one model file, orbits it, selects isometric view, saves and reopens it, exports one sheet, and verifies the resulting pixels.
- Formatting, linting, unit/integration tests, and release builds pass for every supported target available in CI.

Validation is against rendered behavior and serialized authority, not source-code labels or screenshots alone.

## Design Consequences

The application is intentionally an image-based renderer, not a lightweight modeler. It can produce strong parallax and occlusion cues inside the angular coverage of its sprites, but it cannot make sparse observations complete from every angle. That limitation is part of the model's honesty and keeps the two-sprite bowl an image illusion rather than an inferred object.

The durable authority is small: canonical PNG evidence, fixed registration, and one versioned warp/compositing contract. Everything visible in the viewport or export is derived from those inputs, and no parallel geometric representation can drift from them.
