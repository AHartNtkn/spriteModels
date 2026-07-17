# DepthSprite editor specification

## Purpose

DepthSprite is a desktop sprite-model authoring application. The user edits a
bundle of oriented PNG sources and sees their transformed pseudo-3D result update
immediately. The source sprites are the complete model authority.

Each source stores RGB color and inverted-alpha relief. Editing occurs only through
the source sprites. The model viewport derives an image from those sources and
provides orbit and zoom inspection.

## Model document

The document owns one core `AuthoredModel` plus editing state. The model contains:

- integer bounds `(width, height, depth)`, each in `1..=63`;
- one to six raw RGBA source charts, each assigned to one canonical side, with
  independent bits for also supplying and geometrically mirroring its compatible
  opposite.

The editing state contains:

- the selected source and active color or depth layer;
- current color and relief values;
- undo and redo history;
- file path and unsaved-change state;
- a monotonically increasing render revision.

Raw RGBA is authoritative. RGB remains stored when alpha is zero, so removing a
surface sample and adding it again preserves its authored color.

The canonical display order is Front, Right, Top, Back, Left, Bottom. Every source
supplies its primary side. Its explicit **Also Opposite** toggle determines whether
the same PNG also supplies the opposite side. **Mirror Opposite** independently
chooses geometric midpoint-plane reflection instead of direct opposite-frame
reuse. Both bits begin false. An unchecked source never gains an opposite
observation implicitly. No two sources may claim the same side.

## File lifecycle

The top menu contains:

- **File:** New, Open, Save, Save As, Quit
- **Edit:** Undo, Redo
- **View:** Reset Model View

New creates a document from model bounds and an explicitly selected initial side,
defaulting to Front. Its source starts magenta and empty. Open reads one
`.depthsprite` package. Save and Save As write one `.depthsprite` package while
preserving every RGBA value, including RGB beneath empty depth.

Closing, opening, or replacing a document with unsaved changes presents Save,
Discard, and Cancel choices. A failed file operation leaves the current document
unchanged and reports the specific error in the application.

## 3D model import

**File → Import 3D Model…** opens the native file picker filtered to
`.gltf`/`.glb`. Cancelling the picker leaves the editor untouched. A load
failure — malformed glTF, an unsupported extension, an undecodable texture,
or a scene with no triangle geometry — reports the file path and the
underlying error through the same file-operation error dialog Open and Save
use, and the import dialog never opens.

A successful load opens a modal import dialog with two equal square
viewports side by side: *Source Mesh* on the left renders the loaded scene
with the dialog's own lighting; *Converted Preview* on the right renders the
in-progress `AuthoredModel` through the same preview path as the main model
viewport. Both viewports share one orbit camera: plain drag orbits it and
the wheel zooms it, in either viewport, identically to the main model
viewport. Ctrl+drag in the Source Mesh viewport instead rotates the model
itself (an arcball drag about the camera's screen right and down axes)
relative to the capture axes and triggers reconversion; Ctrl+drag in the
Converted Preview viewport still orbits the camera. Any change to the model
rotation, side modes, longest axis, or lighting recomputes the Converted
Preview; camera orbit and zoom never do.

Below the viewports, four settings groups configure the conversion:

- **Orientation** — a Snap to 90° button rotates the model to its nearest
  axis-aligned orientation; four preset buttons (Z-up → Y-up, Flip X, Flip Y,
  Flip Z) each compose one additional fixed rotation onto the current one.
- **Sides** — three rows (Front/Back, Left/Right, Top/Bottom), each holding
  one independent mode selector per side. A side's mode is Capture, From
  opposite, From opposite mirrored, or Off, defaulting to Capture on all six
  sides. From opposite and From opposite mirrored are offered for a side only
  while its paired side's mode is Capture, so a pair can never hold two From
  opposite sides at once; setting a side to any mode other than Capture
  resets a paired From-opposite side back to Off.
- **Bounds** — a longest-axis slider ranging `1..=63`, defaulting to 63, with
  a live `W × H × D` readout of the bounds the current rotation and axis
  setting would produce.
- **Lighting** — azimuth (`−180°..=180°`, default −35°), elevation
  (`−90°..=90°`, default 35°), and ambient (`0.0..=1.0`, default 0.25)
  sliders.

Conversion geometry, sampling, and side-mode semantics follow
`docs/superpowers/specs/2026-07-17-model-import-design.md`.

The footer holds Cancel and Import. Import is disabled while the current
settings fail to convert, for example every side set to Off. Cancel discards
the dialog and its loaded scene; the current document is unaffected. Import
takes the converted `AuthoredModel` through the same unsaved-changes prompt
New and Open use, then replaces the current document with an untitled, dirty
document holding the imported model. While an unsaved-changes prompt is
pending for any reason, the import dialog does not render.

## Window composition

The application uses a conventional top menu followed by one uninterrupted
workspace:

```text
┌ File  Edit  View ──────────────────────────────────────────────┐
│ tools │                    MODEL             │ SOURCE SPRITES   │
│       │                                      │ Front  Right     │
│       │                                      │ Top    Back      │
│       │                                      │ Left   Bottom    │
└────────────────────────────────────────────────────────────────┘
```

The editing tools form a slim vertical palette. The model viewport is the dominant
surface. Authored sources pack in canonical order into a two-column by three-row
grid as they are added.

Each source card contains:

```text
assigned side selector, resize control, and card menu
color canvas
depth canvas
```

Color is always above depth. The canvases have identical pixel coordinates and
share cursor position, zoom, and pan. Clicking either canvas selects that source
and layer while retaining the shared view transform.

The model viewport is at least three times the displayed width and three times the
displayed height of any individual canvas: at least nine times its area. Resizing
uses the available two-column source region rather than leaving unused space while
the model remains dominant. The Add Sprite control occupies only its compact action
height.

The model viewport accepts drag-to-orbit, wheel-to-zoom, and reset-view input. Its
pointer input changes camera state only. Document edit commands originate from the
source canvases and source-card controls.

## Source management

A compact **Add Sprite** control sits below the packed cards. It opens a chooser
containing the six canonical side names; sides owned either primarily or through
an opposite assignment are unavailable. Selecting
a side creates a correctly sized `[255, 0, 255, 0]` chart for that exact side.

The side name in each card header is also a selector. Its first control is **Also
Opposite**. Enabling it assigns the same PNG to the compatible opposite and changes
the header to, for example, `Front + Back`. The control is unavailable when another
source owns that opposite. A second **Mirror Opposite** checkbox is enabled only
while Also Opposite is enabled. It defaults off; when checked, the resolved
opposite reverses the canonical-frame-derived image axis for a true geometric
reflection. Disabling Also Opposite disables but does not clear Mirror Opposite;
reenabling it restores the remembered mirror behavior. Each checkbox is an
independent undoable command and never changes authored RGBA. Reassignment to an
unoccupied side is one undoable command. When old and new canonical dimensions
match, exact RGBA pixels are preserved. When they differ, the editor offers to
recreate that one chart empty at the required dimensions and states that its pixels
will be discarded. It never silently stretches, crops, or interpolates the chart.

A card can import or replace its chart from an RGBA PNG of the required dimensions,
or remove its source. Import, painting, and replacement preserve both opposite-side
bits. Removing a separately authored opposite leaves that side absent unless the
remaining source is explicitly toggled to supply it. The sole remaining authored
source cannot be removed.

## Dimension editing

The selected card's compact **Resize** control opens an edge diagram with add and
remove actions at image top, bottom, left, and right. Each action maps the chosen
image edge through the selected side's signed canonical frame to one signed world
edge and changes that model axis by exactly one pixel.

The model applies the corresponding local edge insertion or removal to every
authored chart whose raster uses that world axis. Opposite or orthogonal charts may
therefore change on different local image edges. Color and depth are one RGBA
raster and always change together. Inserted pixels are `[255, 0, 255, 0]`.
Existing pixels are copied exactly; there is no scaling, interpolation, centering,
or per-chart dimension override.

Charts normal to the changed axis do not change raster dimensions, but that axis is
their relief direction. Their maximum inward depth is half the new axis length and
is validated as part of the same transaction.

Adding beyond 63 or removing below 1 is unavailable. Removing an edge containing
any non-default RGBA opens one confirmation naming every chart edge that will be
discarded. A prospective shrink is rejected if remaining relief would exceed the
new maximum inward depth. The transaction then makes no changes, and the error
identifies the affected side, its deepest remaining pixel, and the new maximum in
both eighth-pixel units and model pixels. Authored relief is never clamped. The
complete synchronized resize is one undoable command and one preview revision.

## Layer visualization

The color canvas displays stored RGB. Empty depth does not hide stored color in the
authoring canvas.

The depth canvas uses this mapping:

- alpha zero: magenta, meaning no surface sample;
- relief zero / alpha 255: black;
- for nonempty alpha `a`, relief `h = 255 - a` and grayscale intensity is
  `round(255h / 254)`;
- relief 254 / alpha 1: white.

Magenta is categorical and does not participate in the relief scale. The mapping is
absolute and identical for every side and model: equal alpha, relief units, and
physical inward depth always display as equal gray. A shallow side's maximum inward
depth therefore need not appear white.

## Tools

The vertical palette contains Pencil, Eraser, Fill, Eyedropper, a basic RGB color
picker, and the current relief value. The color picker shows the current swatch and
opens compact hue and saturation/value controls with direct RGB and hexadecimal
entry. Changing it updates the current paint color without changing chart pixels.

The relief selector is editable in eighth-pixel and model-pixel units and is
limited to the selected side's maximum inward depth: half its opposing model
dimension, or `4 ×` that dimension in relief units. Selecting a side whose maximum
inward depth is smaller than the current pencil/fill value reduces only that
transient tool value. It does not edit chart pixels, dirty the document, or create
history.

| Tool | Color canvas | Depth canvas |
| --- | --- | --- |
| Pencil | Set RGB; preserve alpha | Set selected nonzero alpha; preserve RGB |
| Eraser | Disabled | Set alpha to zero; preserve RGB |
| Fill | Flood equal contiguous RGB; preserve alpha | Flood equal contiguous alpha with selected nonzero alpha; preserve RGB |
| Eyedropper | Select RGB | Select relief, or empty when sampling magenta |

Color Pencil and Color Fill consume the color picker's RGB value. Color Eyedropper
writes its sampled RGB into that same picker.

Pencil drag is one undoable stroke. Fill is one command. Adding, replacing,
reassigning, removing, or resizing sources is one command. Undo and redo restore
exact bounds, RGBA data, source assignments, selection, and preview revision.

## Live model preview

Each document mutation increments the render revision. During the next interface
frame, the preview asks the core `AuthoredModel` for `ResolvedCharts`, renders the
current camera, and caches the framebuffer under the document revision and camera
state. Multiple input events received before one frame produce one preview render.

RGB, relief, assignment, and dimension changes become visible without saving.
Orbit changes rerender the preview without changing the document or undo history.

Camera rows are derived deterministically from orbit yaw, pitch, and zoom. The
renderer continues using exact rational coefficients after camera quantization, so
the same document and camera state produce the same framebuffer.

## Architecture

The application consists of four owners:

- `relief-core`: `AuthoredModel`, bounds, signed canonical frames, synchronized
  dimension operations, validation, `ResolvedCharts`, and relief sampling;
- `depthsprite-format`: conversion between one-file packages and the core model;
- `editor-core`: mutable commands, history, selection, and preview invalidation
  around one core model;
- `desktop-app`: top menu, vertical palette, source-card grid, custom pixel
  canvases, model orbit input, and framebuffer presentation.

The desktop application uses `eframe`/`egui`. Custom canvas widgets translate
pointer coordinates into exact chart pixels. Widget state contains only transient
interaction such as hover and an in-progress stroke; durable values live in the
document.

## Validation

Headless tests prove:

- raw RGB survives alpha-zero chart construction, save, and reopen;
- a single-side source resolves only its primary observation;
- the explicit opposite toggle resolves both compatible observations from one PNG,
  is undoable, survives editing and save/reopen, and rejects assignment conflicts;
- the mirror toggle is available only for an opposite pair, is independently
  undoable, survives editing and save/reopen, and reverses the mathematically
  required raster axis without altering the authored PNG;
- new sources begin with both bits false, and disabling opposite reuse retains the
  dormant mirror bit;
- Add Sprite creates the explicitly selected unoccupied side;
- reassignment preserves matching pixels and requires explicit recreation for a
  dimension mismatch;
- every chart edge maps to the correct signed world edge;
- synchronized resizing changes every affected raster and no unrelated raster;
- bounds, relief limits, destructive removal, undo, redo, and save/reopen preserve
  the dimension-editing contract;
- color edits preserve alpha;
- depth pencil and eraser preserve RGB;
- layer fills affect only the selected contiguous value;
- eyedroppers select the exact active-layer value;
- the color picker accepts visual, RGB, and hexadecimal input and supplies color
  pencil and fill;
- one revision invalidation produces one matching preview render;
- orbit input changes camera state without changing document state;
- the two-source bowl retains its rounded wall and recessed basin after an
  edit-save-reopen cycle.

A realistic application check proves the top-menu lifecycle, vertical tools,
progressive two-by-three source grid, explicit side chooser, opposite and mirror
toggles and selector, compact edge resizing, color-over-depth composition, shared
canvas coordinates, minimum 3× model-to-canvas dimensions, color selection and
painting, immediate preview updates, and read-only model interaction.

## Acceptance

The editor is complete when a user can create or open one model file, assign any
combination of its source sides, resize their shared model dimensions, paint stored
color and surface relief, see every edit in the dominant orbitable model viewport,
undo and redo edits, save one file, and reopen the exact authored RGBA model.
