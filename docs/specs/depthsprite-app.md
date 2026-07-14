# DepthSprite initial editor specification

## Purpose

DepthSprite is a desktop sprite-model authoring application. The user edits a
bundle of oriented PNG sources and sees their transformed pseudo-3D result update
immediately. The source sprites are the complete model authority.

Each source stores independent RGB color and inverted-alpha relief. Editing occurs
only through the source sprites. The model viewport derives an image from those
sources and provides orbit and zoom inspection.

## Model document

The document owns:

- positive integer model bounds `(width, height, depth)`;
- one to six raw RGBA source charts assigned to canonical sides;
- the selected source and active color or depth layer;
- current color and relief values;
- undo and redo history;
- file path and unsaved-change state;
- a monotonically increasing render revision.

Raw RGBA is authoritative. RGB remains stored when alpha is zero, so removing a
surface sample and adding it again preserves its authored color.

The canonical source order is:

```text
Front  Right  Top
Back   Left   Bottom
```

An assigned source supplies its own side. When the opposing side has no distinct
source, the same sprite supplies that side through the opposing canonical frame,
which mirrors its orientation. A distinct opposing source overrides that fallback.

## File lifecycle

The top menu contains:

- **File:** New, Open, Save, Save As, Quit
- **Edit:** Undo, Redo
- **View:** Reset Model View

New creates a document from model bounds and an initial canonical side, defaulting
to Front. Its source starts with zeroed RGB and empty depth. Open reads one
`.depthsprite` package. Save and Save As write one `.depthsprite` package while
preserving every RGBA value, including RGB beneath empty depth.

Closing, opening, or replacing a document with unsaved changes presents Save,
Discard, and Cancel choices. A failed file operation leaves the current document
unchanged and reports the error in the application.

## Window composition

The application uses a conventional top menu, followed by one uninterrupted
workspace:

```text
┌ File  Edit  View ────────────────────────────────────────────────┐
│ tools │               MODEL               │ SOURCE SPRITES       │
│       │                                   │ Front  Right  Top    │
│       │                                   │ Back   Left   Bottom │
└──────────────────────────────────────────────────────────────────┘
```

The editing tools form a slim vertical palette. The model viewport is the dominant
surface. The source area is a three-column by two-row grid that fills in canonical
order as sources are added.

Each source card contains:

```text
side name and fallback assignment
color canvas
depth canvas
```

Color is always above depth. The two canvases have identical pixel coordinates and
share cursor position, zoom, and pan. Clicking either canvas makes that layer
active while retaining the shared view transform.

The model viewport is at least three times the displayed width and three times the
displayed height of any individual color or depth canvas. The window enforces a
minimum size that preserves this ratio. Resizing allocates additional space to the
model before enlarging source canvases.

The model viewport accepts drag-to-orbit, wheel-to-zoom, and reset-view input. Its
pointer input changes camera state only. Document edit commands originate from the
source canvases and source-card controls.

## Source management

The next empty grid position is an **Add Sprite** card. Adding a sprite selects an
unassigned canonical side and creates a correctly sized empty RGBA chart. A source
card can import or replace its chart from an RGBA PNG with the required dimensions,
or remove its distinct side assignment. Removing an override restores the opposing
source fallback immediately.

The card header states both the assigned side and any side currently using it as a
fallback, for example `Front → Back`. Once Back has its own source, the headers read
`Front` and `Back`.

## Layer visualization

The color canvas displays stored RGB. Empty depth does not hide the stored color in
this authoring canvas.

The depth canvas uses this exact visual mapping:

- alpha zero: magenta, meaning no surface sample;
- relief zero / alpha 255: black;
- increasing inward relief: increasing grayscale brightness;
- relief 254 / alpha 1: white.

Magenta is categorical and does not participate in the relief scale.

## Tools

The vertical palette contains Pencil, Eraser, Fill, and Eyedropper, followed by the
current RGB swatch and current relief value. The current relief is editable in
eighth-pixel units and also displayed in model pixels.

Tool behavior depends on the active canvas:

| Tool | Color canvas | Depth canvas |
| --- | --- | --- |
| Pencil | Set RGB; preserve alpha | Set selected nonzero alpha; preserve RGB |
| Eraser | Disabled | Set alpha to zero; preserve RGB |
| Fill | Flood equal contiguous RGB; preserve alpha | Flood equal contiguous alpha with selected nonzero alpha; preserve RGB |
| Eyedropper | Select RGB | Select relief, or empty when sampling magenta |

Pencil drag is one undoable stroke. Fill is one command. Adding, replacing, or
removing a source is one command. Undo and redo restore exact RGBA data, source
assignments, selection, and derived preview revision.

## Live model preview

Each document mutation increments the render revision. During the next interface
frame, the preview converts the current raw RGBA sources into relief charts,
resolves opposing-side fallbacks, renders the current camera, and caches the
resulting framebuffer under the document revision and camera state. Multiple input
events received before one frame produce one preview render.

RGB and depth changes become visible without a save operation. Orbit changes
rerender the preview without changing the document or its undo history.

Camera rows are derived deterministically from orbit yaw, pitch, and zoom. The
renderer continues using exact rational coefficients after camera quantization, so
the same document and camera state produce the same framebuffer.

## Architecture

The application consists of four owners:

- `relief-core`: raw-RGBA canonical charts and derived relief sampling;
- `depthsprite-format`: one-file raw-RGBA package loading and saving;
- `editor-core`: mutable document, commands, history, side fallback, and preview
  invalidation;
- `desktop-app`: top menu, vertical palette, source-card grid, custom pixel
  canvases, model orbit input, and framebuffer presentation.

The desktop application uses `eframe`/`egui`. Custom canvas widgets translate
pointer coordinates into exact chart pixels. Widget state contains only transient
interaction such as hover and an in-progress stroke; durable values live in the
document.

## Validation

Headless tests prove:

- raw RGB survives alpha-zero chart construction, save, and reopen;
- one source resolves to its opposing side with mirrored canonical orientation;
- a distinct opposing source replaces the fallback;
- color edits preserve alpha;
- depth pencil adds a sample while preserving RGB;
- depth eraser removes a sample while preserving RGB;
- layer fills affect only the selected contiguous value;
- eyedroppers select the exact active-layer value;
- stroke, fill, source, undo, and redo commands restore exact document states;
- one revision invalidation produces one matching preview render;
- orbit input changes camera state without changing document state;
- the two-source bowl retains its rounded wall and recessed basin after an
  edit-save-reopen cycle.

A realistic application check proves the top-menu file lifecycle, vertical tools,
progressive three-by-two source grid, color-over-depth card composition, shared
canvas coordinates, minimum 3× model-to-canvas dimensions, immediate preview
updates, and read-only model interaction.

## Acceptance

The initial editor is complete when a user can create or open one model file,
manage any of its source sides, paint stored color and surface relief with the
approved basic tools, see every edit in the dominant orbitable model viewport,
undo and redo edits, save one file, and reopen the exact authored RGBA model.
