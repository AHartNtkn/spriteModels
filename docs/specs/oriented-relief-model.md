# Oriented relief model and renderer specification

## Model authority

The authoritative model is a bundle of oriented color-and-relief images. Each
rendered frame derives displaced samples from those images, composites them, and
then discards the samples. Model editing changes the images themselves. This keeps
the representation aligned with the goal: transform authored sprites to create the
impression of depth, including concave detail.

## Model space and charts

A model has positive integer bounds `(width, height, depth)` in model pixels and
one to six canonical charts. Their image dimensions are:

| Chart | PNG width | PNG height | Relief direction |
| --- | ---: | ---: | --- |
| front, back | model width | model height | inward along model depth |
| left, right | model depth | model height | inward along model width |
| top, bottom | model width | model depth | inward along model height |

Each chart defines its own oriented plane and inward unit axis. Opposing charts
use mirrored frames so that their pixels refer to the same model-space bounds.

## Alpha semantics

The PNG must be non-premultiplied 8-bit RGBA.

- `alpha = 0` means background and contributes no surface sample.
- `alpha > 0` means foreground color `(red, green, blue)`.
- The inverted relief value is `h = 255 - alpha`.
- One relief unit is `1 / RELIEF_UNITS_PER_PIXEL` model pixel.
- `RELIEF_UNITS_PER_PIXEL = 8` is the shared program standard for every model.

Thus opaque alpha 255 lies on its chart plane, while decreasing nonzero alpha
moves the sample inward. This convention supports both protruding profiles and
recessed surfaces depending on which oriented chart supplies the visible surface.

## Relief field

Foreground pixels are divided into four-connected components. Relief is
interpolated only among nearby samples belonging to the same component. Background
does not become zero relief and disconnected islands do not influence each other.
This preserves holes, gaps, and separate parts rather than bridging them with an
invented surface.

For rasterization, each foreground pixel owns its closed cell. The relief field is
sampled across that cell and subdivided into small triangles. Subdivision is an
accuracy choice, not a persistent geometric representation.

## Direct image transform

For chart `i`, let:

- `P_i(u, v)` map an image coordinate to its point on the canonical chart plane;
- `n_i` be the chart's inward model-space axis;
- `h_i(u, v)` be inverted-alpha relief in model pixels;
- `r` and `s` be the camera's screen-right and screen-down rows;
- `q` be the camera depth row.

The displaced sample exists only for the current render:

```text
X_i(u, v) = P_i(u, v) + n_i * h_i(u, v)
screen_i  = (dot(r, X_i), dot(s, X_i))
depth_i   = dot(q, X_i)
```

A chart is drawn only when its inward axis faces the camera. The screen transform
and depth comparison use exact rational coefficients so equivalent inputs do not
change because of floating-point drift.

## Compositing and stability

All visible chart samples compete in one transient framebuffer. The sample nearest
the camera owns the pixel. Exact depth ties use a permanent canonical chart rank,
then source row and column, never the order in which files or charts were loaded.
This makes a fixed model and camera deterministic and prevents alternating edge
ownership.

The winning chart supplies RGB directly. The renderer does not blend chart colors
or create new edge colors. If authored charts disagree where their displaced
surfaces meet, the deterministic visibility rule still selects one authored color;
the application must not turn that disagreement into temporal flicker.

## Single-file model format

A `.depthsprite` file is a ZIP archive containing:

```text
manifest.json
views/front.png
views/right.png
views/back.png
views/left.png
views/top.png
views/bottom.png
```

Only the views declared by the manifest are present. Version 1 manifest data is:

```json
{
  "format": "depthsprite",
  "version": 1,
  "bounds_pixels": [32, 16, 32],
  "views": ["front", "top"]
}
```

The manifest and decoded PNG contents define the model. The loader validates the
manifest, unique canonical views, chart dimensions, and PNG representation.

## Decisive example

The reference bowl contains only a front chart and a top chart. The front chart's
silhouette and relief form the rounded outer wall and near rim. The top chart's
radial inverted-alpha field forms a recessed basin. At an oblique camera both
charts remain visibly responsible for the result, the center is deeper than the
rim, and transparent gaps remain transparent. Passing this case demonstrates the
additional depth behavior that the inspiration does not provide.
