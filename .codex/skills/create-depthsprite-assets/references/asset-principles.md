# DepthSprite asset principles

Use these specifications as the authority:

- `docs/specs/oriented-relief-model.md`
- `docs/specs/depthsprite-app.md`
- `docs/specs/depthsprite-demo-assets.md`

## Source representation

Each stored source is an RGBA PNG with one primary canonical side and two explicit
booleans. `opposite` determines whether the same PNG also resolves through the
compatible opposite frame. `mirror` determines whether that derived observation is
geometrically reflected through the model midpoint plane. Both begin false; mirror
is remembered but has no rendering effect while opposite is false. Assignments
cannot overlap.

For alpha `a`, `a = 0` is empty and visible as magenta in the depth editor. For
foreground, inward relief is `h = 255 - a` eighth-pixel units. RGB is stored color.
The global scale is eight relief units per model pixel.

For bounds `(W,H,D)`:

| Side | Raster | Maximum relief |
| --- | --- | --- |
| Front / Back | `W × H` | `4D` |
| Left / Right | `D × H` | `4W` |
| Top / Bottom | `W × D` | `4H` |

At maximum legal relief, compatible opposing surfaces meet at the midplane.

## Canonical orientation

| Side | image +u | image +v | inward relief |
| --- | --- | --- | --- |
| Front | +x | +y | +z |
| Back | -x | +y | -z |
| Left | +z | +y | +x |
| Right | -z | +y | -x |
| Top | +x | +z | +y |
| Bottom | +x | -z | -y |

Use these signs when matching landmarks and directional lighting. Direct opposite
reuse keeps the authored raster unchanged. Geometric mirroring reverses `u` for
Front/Back and Left/Right, and reverses `v` for Top/Bottom. The complete RGBA texel
is transformed, keeping color, relief, silhouette, and baked lighting registered.

## Geometry construction

Start from the intended cross-section, then sample it into relief units. For a
rounded span with doubled radius `R` and doubled offset `d`, integer sampling can
use:

```text
span(d) = floor_sqrt(R² - d²)
```

Derive each row's actual span before computing horizontal relief. Normalizing every
row back to the maximum radius produces a cone or cylinder instead of the intended
vertical curvature.

For a hollow vessel, use one mirrored exterior source assigned to a compatible
side pair and one opening source assigned only to Top. Share the zero-depth rim
landmark. Keep the cavity profile strictly inside the exterior for interior radii.
A Bottom exists only when a source explicitly supplies it.

Tight masks place intended seam or silhouette foreground on the relevant first and
last rows or columns. Global transparent padding changes registration and can make
surfaces float apart.

## Baked cartoon lighting

Apply lighting to RGB only. Use one world-directed key light and a strong response,
for example a clamped linear or gently curved exposure function:

```text
exposure = face_bias + directional_position - relief_darkening
rgb' = clamp(base_rgb + contrast * exposure)
```

Choose contrast large enough that highlights and shadows are obvious at native
scale. Curved surfaces need a graduated main response with many ordered values
across the cavity or exterior. Material regions, rim highlights, outlines, and
specular accents may use hard graphic steps without replacing that form gradient.

Useful quantitative evidence includes luminance range, highlight/shadow
population, distinct ordered values along relief-matched scans, and comparison of
upper-left and lower-right samples. Unique-color count by itself is insufficient.

## Validation sequence

1. Test exact bounds, source dimensions, assignments, masks, relief formulas, and
   relief maxima.
2. Test that `resolve()` yields exactly the intended canonical sides.
3. Regenerate all packages twice and compare bytes with committed assets.
4. Render canonical, edge-on, and elevated oblique views. For rounded or paired
   assets, sample a full orbit around transition angles.
5. Inspect native-scale color sources, depth sources, final RGB renders, and owner
   maps. Confirm the visual claim without relying on motion or overlays.
6. Run `cargo fmt --all -- --check`, focused fixture/renderer tests,
   `cargo test --workspace`, and `cargo build --release`.
