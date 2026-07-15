# DepthSprite

DepthSprite is a Rust model-authoring project inspired by
[PixZels](https://pixel-salvaje.itch.io/pixzels). A model is a bundle of oriented
PNG sprites. RGB supplies color and inverted alpha supplies relief. The renderer
transforms and composites the sprites into an orbitable pseudo-3D image.

## Run the editor

Start with a new model or open an existing model directly:

```sh
cargo run -p desktop-app
cargo run -p desktop-app -- path/to/model.depthsprite
```

## Model files

A `.depthsprite` is one model file containing one to six canonical RGBA PNG
sources: Front, Right, Top, Back, Left, and Bottom. RGB stores color. Inverted
alpha stores inward relief at eight units per model pixel: alpha 255 is zero
relief, decreasing nonzero alpha moves inward, and alpha zero means the pixel is
empty. RGB remains stored beneath an empty pixel so geometry can be removed and
restored without losing its color.

Each source always supplies its named side. Its **Also Opposite** toggle controls
whether the same PNG also supplies the compatible opposite side. With the toggle
off, that opposite remains absent unless it has its own source. **Mirror Opposite**
optionally reflects the paired observation through the model midpoint plane while
keeping color and depth registered. Both toggles begin off; disabling Also Opposite
remembers the mirror choice.

## Authoring workflow

Use **File → New** to start a model, **File → Open** to open a `.depthsprite`, and
**File → Save** or **Save As** to write the complete model back to one file. New,
Open, and Quit ask whether to save, discard, or cancel when there are unsaved
changes.

Source cards fill the canonical three-by-two grid. Use **Add Sprite** to choose an
unassigned canonical side. Open a card's side menu to reassign it or toggle **Also
Opposite**; a paired card is labeled, for example, `Front + Back`. A card's menu
can **Import PNG…** to replace it with a same-sized RGBA image or **Remove** it.
Painting, importing, saving, and reopening preserve the side assignment.

Each source card places its color canvas above its depth canvas. Both canvases use
the same pixel coordinates, zoom, and pan. The color canvas shows stored RGB even
where depth is empty. The depth canvas shows empty pixels as magenta, zero relief
as black, and greater inward relief as brighter gray. Click a canvas to make its
layer active.

The vertical palette provides:

- **Pencil:** paints RGB on the color canvas or the selected nonempty relief on
  the depth canvas while preserving the other data.
- **Eraser:** empties depth while preserving RGB; it is disabled for color.
- **Fill:** flood-fills contiguous equal color or equal depth on the active layer.
- **Eyedropper:** selects the stored RGB, relief, or empty-depth value under the
  cursor.

The color control supports hue and saturation/value selection plus direct RGB and
six-digit hexadecimal entry. The relief control uses eighth-pixel units and also
shows the value in model pixels. Use **Edit → Undo** and **Redo** to move through
completed strokes, fills, and source changes.

Drag the model viewport to orbit and use the mouse wheel to zoom. These controls
change only the view, not the model. Use **View → Reset Model View** to restore the
default camera.

See [the application specification](docs/specs/depthsprite-app.md) for the complete
behavior contract.
