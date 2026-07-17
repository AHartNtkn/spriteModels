# 3D model import design

Convert an existing 3D model (glTF/GLB) into a `.depthsprite` through a graphical
import dialog. Conversion is orthographic height-field capture: for each chosen
canonical side, the mesh is rasterized along that side's inward axis; nearest-hit
depth becomes inward relief and the shaded surface color becomes RGB. Undercuts,
concavities past the midplane, and geometry invisible from every captured axis
are lost by construction — the dialog exists to let the user see and steer that
loss before accepting.

## Architecture

One new crate, `crates/mesh-import`, independent of the editor and UI:

- **Scene loading** (`scene.rs`): reads glTF/GLB via the `gltf` crate and
  flattens the node hierarchy into a world-space triangle soup: positions,
  per-vertex normals (face normals computed when absent), UVs, and
  per-primitive material (base-color factor and decoded base-color texture).
  Output type `TriangleScene` is pure data; nothing downstream knows glTF
  exists.
- **Rasterizer** (`raster.rs`): hand-written orthographic edge-function
  rasterizer over a `TriangleScene`. Given a 3×3 rotation, target dimensions, a
  light direction, and an ambient level, it produces a nearest-depth buffer and
  a shaded color buffer. One code path serves both the dialog's mesh-preview
  viewport (large target, arbitrary camera rotation) and capture (≤63² target,
  canonical rotations). Benchmarked single-threaded on the development machine:
  ~70 fps at 512² with 100k triangles; a full six-side 63² capture of the same
  mesh in ~21 ms.
- **Capture** (`capture.rs`): for each side to capture, runs the rasterizer
  along that side's inward axis using the canonical chart frames from
  `relief-core`, converts depth to eighth-pixel relief, and packs RGBA charts.
  Output is `Bounds` plus `Chart`s — directly an `AuthoredModel`.
- **Settings** (`lib.rs`): `ImportSettings { rotation, side_modes,
  longest_axis_pixels, light_azimuth, light_elevation, ambient }` — the
  complete, UI-independent description of one conversion. Conversion is a pure
  function `convert(&TriangleScene, &ImportSettings) -> Result<AuthoredModel,
  ImportError>`.

`desktop-app` gains **File → Import 3D Model…** and a modal import dialog
(`import_dialog.rs`). `editor-core` gains `EditorDocument::from_unsaved_model`
(an untitled, dirty-until-saved document for a freshly imported model) and
`OrbitCamera::basis_f32` (a float camera basis for the mesh rasterizer).

Dependencies: `mesh-import` → `relief-core`; `desktop-app` → `mesh-import`.
New external crates (`mesh-import` only): `gltf` plus a PNG/JPEG decoder for
embedded textures.

## Conversion geometry and sampling

**Fitting.** Apply the import rotation to the mesh and take its axis-aligned
bounding box. Scale uniformly so the longest box axis equals the longest-axis
setting `N` (1..=63). The other two bounds are the scaled extents rounded up
(`ceil`, floor of 1), so the mesh always fits inside the model box and fitting
itself never causes clamping. The mesh is centered in the box. The default
import rotation is a half-turn about X (`diag(1, −1, −1)`): glTF's convention
is +Y up and +Z toward the viewer, while the box frame's y points down and its
front face looks along +z, so the identity mapping would import every model
upside down and back-to-front.

**Depth → relief.** For each captured side, rasterize along that side's inward
axis. Each covered texel keeps its nearest hit; depth `d` (pixels from the box
face) becomes relief `h = round(8·d)` eighth-pixel units, clamped to
`h_max = 4·L` where `L` is the opposing dimension. Alpha is `255 − h`, which is
at least 3 for covered texels (`h_max ≤ 252`), so covered texels never collide
with the empty encoding. Uncovered texels are alpha 0, RGB black. A hit past
the midplane (`d > h_max/8`) is **dropped** — the texel stays empty from that
side. This is exact, not lossy: along each axis every surface point lies within
reach of at least the nearer of the two opposing sides (`d_front > D/2 ⇔
d_back < D/2`, the midplane boundary belonging to both), so geometry beyond one
side's reach is precisely the geometry the opposite side captures. A chart is a
height field of the surface visible from its side; a texel whose nearest hit is
unreachable stays empty rather than encoding the occluded geometry behind it.
A side with zero coverage becomes an empty chart, which `AuthoredModel`
permits.

## Surface ownership

Full projections would store one surface region in up to six charts, so the
same feature would be drawn several times and captured obliquely by sides that
barely face it. Instead, each observed surface point is kept by exactly one
side — the one that represents it best — plus a one-texel seam closure:

- Every hit records the face normal `n̂` of its triangle and the observation
  orientation `σ = sign(−n̂ · axis_S)` (+1: the front face; −1: the reverse
  face, as when open-mesh interiors are observed two-sided).
- Candidate owners for a hit are the enabled Capture sides `T` that observe
  the **same oriented face** (`σ · (−n̂ · axis_T) > 0`), reach the point
  (`d_T ≤ h_max_T/8`), and see it: its depth lies within the first-hit
  interval of `T`'s reachability-filtered depth buffer at the projected
  texel. That interval spans the buffer's local depth gradient times half a
  texel plus one relief quantum — a derived bound, conservative toward
  overlap, because consistently-placed overlap composites harmlessly while a
  hole shows the background. The capturing side is always a candidate for its
  own hit, so no seen-and-reachable point is orphaned when its ideal side is
  occluded or disabled.
- The owner is the candidate maximizing `σ · (−n̂ · axis_T)` — the side
  viewing the surface most head-on, which maximizes texel density on that
  surface. Exact ties resolve by canonical side rank.
- After ownership filtering, each side dilates its kept texels by one texel
  (4-neighborhood) into its own reachability-filtered hits. Tent
  interpolation ends at the alpha-zero boundary, so a strict partition would
  open sub-texel gaps where differently-owned regions abut; the closure ring
  is the support the interpolation needs to meet the neighboring chart, and
  it carries true geometry, so the overlap is consistent rather than doubled.

## Fabricated-wall cuts

Within a chart, four-connected foreground texels interpolate as one
continuous surface, so adjacency across an occlusion boundary fabricates a
wall the model does not have (an ear silhouetted over the back renders as a
cliff joining them). After ownership and closure, each chart is scanned for
4-adjacent covered pairs whose relief differs by more than one continuous
best-faced sheet can produce: 8 units (the 45° ownership slope bound over one
texel) plus 2 for the two roundings. Each candidate pair is tested against
the model itself — the segment between the two reconstructed sample points is
sampled at one-texel spacing, and the pair stays connected only if every
interior sample lies within one texel of the mesh, the discretization's own
resolution. A real steep wall (a cavity side that only its grazing fallback
owner can see, like the bowl's) passes this test; an occlusion cut, whose
would-be wall crosses free space, does not. At a fabricated cut the far
(deeper) texel of the pair is emptied: the near sheet ends at its true
silhouette and keeps that outline intact, while the far sheet loses only the
fragmentary margin that better-facing sides cover. Where no other side sees
the dropped strip, a one-texel gap remains — preferred to fabricated
geometry. Cuts run after closure so dilation cannot re-bridge them.
Point-to-mesh distance queries use a uniform triangle grid built once per
conversion.

**Color.** At the winning hit, interpolate UV and vertex color and compute base
color per the glTF definition: `baseColorFactor × baseColorTexture(uv) ×
COLOR_0`, with bilinear texture lookup. One sample per output texel center — no
supersampling; the compositor never blends, and crisp edges suit the format.
Materials with `alphaMode: MASK` discard samples below the material's own
cutoff. `BLEND` materials are treated as opaque: the format has no translucency,
and this is the importer's defined behavior for such materials.

**Lighting.** `shaded = base × (ambient + (1 − ambient) · max(0, n̂ · l̂))`,
with the light direction expressed in the model-box frame (the capture axes). It
does not rotate with the mesh or the camera, so all sides show consistent
world-directed lighting. Shading is two-sided: a normal
pointing away from the capture direction is flipped before lighting, so open
meshes do not capture black interiors. Azimuth, elevation, and ambient are
dialog settings; defaults are light from upper-front-left and ambient 0.25.

## Side modes and pairing

Sides are configured as three pairs (Front/Back, Left/Right, Top/Bottom). Each
side has a mode:

- **Capture** — rasterized independently; produces its own source PNG.
- **From opposite** — no capture; the opposite side's source supplies it via
  the format's Also Opposite bit.
- **From opposite, mirrored** — as above with the Mirror Opposite bit also set.
- **Off** — absent.

"From opposite" (either variant) is selectable only while the paired side's
mode is Capture, and the two sides of a pair cannot both be "From opposite".
These modes map exactly onto the per-source `opposite`/`mirror` fields already
in the manifest; the importer never invents a second PNG for a supplied
opposite. Default: all six sides Capture.

## Import dialog

**Entry.** File → Import 3D Model… opens the native file picker (`rfd`,
filtered to `.gltf`/`.glb`). Load failure shows the error and returns to the
untouched editor. Success opens the modal dialog.

**Layout.** Two equal square viewports side by side on top; a settings panel of
four aligned groups beneath; Cancel/Import in the footer. Viewport footers hold
no controls.

- *Source Mesh* (left): the `TriangleScene` rendered by the rasterizer with the
  current lighting.
- *Converted Preview* (right): the captured `AuthoredModel` rendered through
  the existing editor preview/compositor path.
- Settings groups:
  1. **Orientation** — Snap to 90°, axis presets (Z-up → Y-up, Flip X/Y/Z),
     and the Ctrl+drag hint.
  2. **Sides** — the three pair rows with per-side mode selectors.
  3. **Bounds** — longest-axis slider (1..=63, default 63) with a live
     `W×H×D` readout.
  4. **Lighting** — azimuth, elevation, ambient.

**Camera.** Both viewports share one orbit camera: plain drag orbits, wheel
zooms, identical to the main model viewport's controls, and the shared state
keeps the two views at the same angle and scale for consistent comparison.

**Model rotation.** Ctrl+drag in the Source Mesh viewport rotates the model
itself (arcball) relative to the capture axes and triggers re-capture. The
Orientation group's buttons snap or re-axis the same rotation. Camera orbit
never changes the conversion; model rotation always does.

**Recompute.** Any change to `(rotation, side modes, N, light, ambient)` runs
`convert()` and invalidates the converted preview. During a Ctrl+drag,
re-capture is throttled to the frame rate; at the measured ~21 ms per full
capture this stays interactive, so there is no background thread.

**Accept/cancel.** Import builds `EditorDocument::from_unsaved_model(model)` — an
untitled, dirty document — after the same unsaved-changes prompt New/Open use.
Cancel discards everything. The current document is untouched until Import is
confirmed.

## Error handling

- **Unloadable file** (malformed glTF, unsupported extensions, undecodable
  texture): the dialog never opens; a message box names the file and the
  underlying error.
- **No triangle geometry** after flattening (points/lines only): rejected the
  same way, with a message saying the scene contains no triangles.
- **Degenerate captures are not errors:** zero-coverage sides become empty
  charts; zero-area triangles are skipped by the rasterizer's area test; a
  bound that rounds to 1 is valid.
- **Internal invariants** (relief exceeding `h_max` after clamping, chart
  dimensions disagreeing with bounds) are bugs: `convert` returns an error that
  surfaces loudly. `AuthoredModel::new` re-validates on construction, so an
  invalid capture cannot reach the document.

## Testing

All assertions are properties derived from the input geometry — never stored
reference images or byte comparisons.

**Synthetic-geometry tests** (development tier, insufficient alone):

- *Scene loading:* a handcrafted minimal glTF (few triangles, known node
  transforms, tiny embedded texture) loads into exactly the expected
  world-space triangles, normals, UVs, and colors; missing normals get correct
  face normals.
- *Rasterizer:* an axis-aligned quad at depth `d` covers exactly its projected
  texels with interpolated depth `d`; of two overlapping quads the nearer wins
  everywhere; a known normal under a known light yields the lambert formula's
  value.
- *Capture:* a cube spanning the box gives relief 0 on all six faces; a quad
  past the midplane clamps to `h_max`; alpha is `255 − h` where covered and 0
  elsewhere; derived bounds obey the ceil rule and 1..=63; pair modes set
  exactly the Also Opposite / Mirror bits and capture only the primary side.

**Real-model tests:** GLB fixtures committed under the `mesh-import` crate's
test directory (never fetched at test time; a missing fixture fails loudly):
Utah teapot, decimated Stanford bunny, decimated Stanford dragon, and a
textured Earth sphere. The implementation plan pins exact sources, licensing,
and any one-time conversion from OBJ/PLY. Assertions:

- each fixture loads with a plausible nonzero triangle count;
- full six-side conversion succeeds at several bounds settings;
- every produced texel satisfies the format invariants (`h ≤ h_max`, alpha
  `255 − h` or 0, chart dimensions match bounds);
- every side of these closed meshes has nonempty coverage;
- the Earth sphere's front-center texel has relief 0 and its silhouette is
  circular within a texel;
- captured Earth color varies across the surface (texture sampling is live,
  not constant).

**Dialog logic** (following the existing `desktop-app` test style): pair-mode
constraints, shared camera state feeding both viewports, recompute on every
setting change, Import routing through the unsaved-changes prompt, and Cancel
leaving the document untouched.
