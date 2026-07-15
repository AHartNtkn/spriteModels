# Oriented relief-sprite mathematical specification

## Authority

An `AuthoredModel` is a bundle of one to six oriented RGBA images. Those images are
the complete authored model. Rendering directly resamples them into a target view
to produce a pseudo-3D impression. The only persistent spatial information is each
image's primary canonical side, whether that same PNG also supplies its compatible
opposite, whether that opposite is geometrically mirrored, and shared integer
bounds `(width, height, depth)`.
Every bound is in `1..=63`.

`relief-core` owns `AuthoredModel`, its bounds, canonical chart frames, validation,
dimension changes, and authored-to-resolved conversion. Package loading, editing,
fixture generation, and rendering use that authority rather than defining their
own chart interpretation.

For source alpha `a`:

```text
a = 0       => outside the chart domain
a in 1..255 => foreground
h = 255 - a => inward relief in eighth-pixel units
```

`RELIEF_UNITS_PER_PIXEL` is 8 for every model. Source RGB is authored color;
alpha encodes relief rather than display opacity. A selected foreground sample is
displayed with alpha 255.

For chart `i`, let `L_i` be the opposing model dimension: depth for Front/Back,
width for Left/Right, and height for Top/Bottom. Its maximum inward depth, expressed
in relief units, is:

```text
h_max(i) = 4 L_i
```

This is half the opposing dimension at eight relief units per pixel. Two explicit
opposing charts at their maximum inward depth therefore meet exactly at the model
midplane. They neither leave a gap nor pass through one another. The 63-pixel
bound limit keeps `h_max <= 252`, which remains representable by nonzero alpha.
Every model construction and mutation path rejects relief beyond this derived
maximum; it is not a separate model setting.

For example, along a depth axis of length `D`, the opposing surface coordinates
are exactly:

```text
front: 0 + (4D / 8) = D/2
back:  D - (4D / 8) = D/2
```

The same equality holds along width and height.

## Canonical charts

Bounds `(width, height, depth)` register up to six canonical charts:

| Chart | Image dimensions | Positive relief direction |
| --- | --- | --- |
| front, back | `width × height` | inward along model depth |
| left, right | `depth × height` | inward along model width |
| top, bottom | `width × depth` | inward along model height |

Use world `x` from left to right, `y` from top to bottom, and `z` from front to
back. Image `u` increases rightward and image `v` downward:

| Chart | Image `+u` | Image `+v` | Relief direction |
| --- | --- | --- | --- |
| front | `+x` | `+y` | `+z` |
| back | `-x` | `+y` | `-z` |
| left | `+z` | `+y` | `+x` |
| right | `-z` | `+y` | `-x` |
| top | `+x` | `+z` | `+y` |
| bottom | `+x` | `-z` | `-y` |

These signed frames are the authority for registration, opposite-side orientation,
and synchronized edge resizing. Bounds register image planes; they do not assert
that the enclosed box is occupied. Each chart remains an independent
two-dimensional source domain.

## Authored and resolved charts

Authored charts are the PNG evidence stored in the model. Each source has one
primary side and two independent opposite-side bits: **Also Opposite** determines
whether it supplies the compatible opposite observation, and **Mirror Opposite**
determines how that observation is registered. Mirror Opposite has no rendering
effect while Also Opposite is disabled. A source cannot claim a side already
assigned to another source.

With Mirror Opposite disabled, resolution preserves the existing direct-reuse
behavior: unchanged RGBA is observed through the opposite canonical frame. With
Mirror Opposite enabled, resolution reflects the authored surface through the
model midpoint plane. The primary and opposite canonical frames determine the
required raster transform:

```text
front <-> back:  opposite(u, v) = source(width  - 1 - u, v)
left  <-> right: opposite(u, v) = source(width  - 1 - u, v)
top   <-> bottom: opposite(u, v) = source(u, height - 1 - v)
```

The transform applies to complete RGBA texels, so color, alpha relief, silhouette,
and baked lighting remain registered. It does not create a second authored PNG or
introduce renderer-specific geometry.

`AuthoredModel::resolve` expands only those assignments into a `ResolvedCharts`
observation set in canonical rank order. An absent side remains absent; the
presence of its opposite does not infer it. Thus one Front source may explicitly
supply Front+Back, while one Top source may remain Top-only. `ResolvedCharts` has
read-only iteration and is the only chart collection accepted by the renderer.
Resolution never adds authored charts or copied PNGs to the document or package.

## Continuous relief within a chart

Foreground texels are partitioned into four-connected components. Let `K` be the
texel centers in one component, `h_k` their encoded relief, and

```text
phi(dx, dy) = max(0, 1 - |dx|) × max(0, 1 - |dy|)
```

For a source point `p` in the union of that component's half-open texel cells:

```text
N(p) = sum(k in K, h_k × phi(p - k))
D(p) = sum(k in K,       phi(p - k))
h(p) = N(p) / D(p)
```

`D` is positive throughout the component domain. This normalized tent field is
exact at texel centers, continuous through connected relief gradients, works for
one-pixel-wide regions, and ends at the alpha-zero boundary. Samples from another
component or chart never enter `N` or `D`.

## Direct image warp

For chart `i` and target view `V`, let `p = (x, y, 1)` be a homogeneous source
coordinate. Canonical registration and the camera compile to:

- `H_i,V`, the flat two-dimensional image transform;
- `e_i,V`, the screen displacement per relief unit;
- `g_i,V`, the flat transient-depth coefficients;
- `gamma_i,V`, the transient-depth displacement per relief unit.

The image warp and compositor depth are:

```text
W_i,V(p) = H_i,V p + h_i(p) e_i,V
Z_i,V(p) = g_i,V · p + gamma_i,V h_i(p)
```

`W` is the entire rendering model: an affine image transform plus relief-driven
two-dimensional displacement. `Z` exists only to choose among image samples for
the current frame. Both can be evaluated without constructing world geometry.

## Exhaustive inverse sampling

Rendering is output-first. For every output pixel center `s` and every resolved
chart, the renderer solves `W(p) = s` and retains every solution.

Write the projected equation as two affine constraints in `(x, y, h)`. When the
combined coefficient matrix has rank two, its solution is an affine line. Select
the pair with the greatest absolute determinant, using `(x,y)`, `(x,h)`, `(y,h)`
as the stable tie order, and solve it as affine functions of the remaining
parameter:

```text
(x(t), y(t), h(t)) = c0 + c1 t
```

This definition does not require the flat source-image basis to remain invertible.
A curved chart can therefore retain projected area at a canonical edge-on view.
If the combined mapping has rank below two, it has no two-dimensional projected
support and contributes no output samples.

The chart domain, `0 <= h <= h_max(i)`, source-cell boundaries, and tent-kernel
break lines divide the affine line into analytic intervals. On each interval,
substitution into the normalized relief equation yields:

```text
Q(t) = h(t) D(x(t), y(t)) - N(x(t), y(t))
```

`Q` has degree at most three. The solver retains ordinary roots, repeated roots at
fold tangencies, and roots on interval endpoints. A root on a shared source-cell or
tent-quadrant boundary is evaluated against every foreground closure incident to
that point. Duplicate roots are removed only within the same source texel and
analytic branch; distinct roots remain distinct preimages. If `Q` is identically
zero on an interval, transient depth is linear there, so the minimum-depth valid
endpoint is sufficient for that texel's compositing candidate.

The finite parameter interval is the intersection of `0 <= x <= width`,
`0 <= y <= height`, and `0 <= h <= h_max(i)` along the affine line. A constant
coordinate outside its legal range empties the interval. The prototype
reconstructs each interval polynomial at fixed nodes, partitions it
at derivative roots, and bisects every sign-changing monotone span. Critical points
are tested directly so tangent roots are retained. Resolved relief is quantized to
`1 / 2^24` of an eighth-pixel before rational depth comparison. This numerical
precision rule affects only root representation; it does not change the warp or
replace it with another rendering representation.

## Transformed-image orientation and compositing

The colored side of a sprite is intrinsic to the local transformed image. For a
canonical chart frame `(O,U,V,N)`, the unnormalized oriented local normal covector
at a differentiable relief location is:

```text
q(x,y) = N - U (dh/dx)/8 - V (dh/dy)/8
```

A preimage supplies its source color when `q` faces the camera. At a tent-gradient
boundary, every incident analytic sector is evaluated; the boundary is included
when it lies in the closure of at least one locally front-facing sector. This keeps
silhouette samples without exposing a finite-area reverse side. It is part of
direct transformed-image sampling, not a chart-wide eligibility or culling step.

Each locally valid preimage produces the key:

```text
(Z, chart_rank, source_y, source_x)
```

The lexicographically smallest key owns the output pixel. Canonical chart rank is
only an exact-depth deterministic tie; it never controls visibility. The owning source texel
supplies RGB unchanged. At an exact source-cell edge or corner, the stable source
coordinates select the lowest nearest texel. An output pixel with no preimage is
transparent black.

This rule preserves multiple preimages under folds, lets the nearer portion of a
relief image occlude a farther portion, makes overlap independent of chart loading
order, and prevents camera motion from inventing or blending colors.

There is no whole-chart camera eligibility. Opposing resolved observations can
contribute complementary locally facing regions to the same output frame. A flat
sprite viewed from its reverse side supplies no valid colored sample; a curved
sprite can remain locally visible around its transformed silhouette. Regions with
no valid preimage remain transparent.

## Two-sprite rounded bowl

The reference bowl uses a Top-only chart and one Front chart explicitly assigned
to Front+Back, derived from compatible rounded profiles. There is no Bottom
observation. With rim radius `R`, exterior height `H`, and radial coordinate
`r`, the exterior follows `H sqrt(1 - (r/R)^2)`. The Front mask therefore follows
the elliptical row radius `R sqrt(1 - (y/H)^2)`, and its inward relief is computed
directly from that row radius rather than normalizing every row back to `R`.

The Top cavity uses a shallower concentric profile with depth `B < H`. Its rim and
the Front exterior meet at the common outer radius, while the cavity remains
strictly inside the exterior at every interior radius. The encoded, interpolated
fields—not merely the generator formula at texel centers—must preserve that
separation.

In an elevated oblique view, exhaustive sampling and transient depth let the near
rim hide the farther basin where appropriate while Front supplies the exterior.
The result is a rounded, recessed bowl made from two transformed images without a
conical side or an internal Top/Front crossing.

## File and editor integration

A `.depthsprite` is one ZIP file containing `manifest.json` and the declared
canonical PNGs under `views/`. The version-2 manifest records integer bounds,
each source's primary side, its explicit opposite-side boolean, and its independent
mirror-opposite boolean. The PNG contents remain the model authority. The current
schema names those source fields `opposite` and `mirror`; it does not retain the
older one-bit `symmetric` representation.

The editor changes RGB and alpha in those source images. It can assign or reassign
canonical sides and can change a model dimension by inserting or removing an image
edge. A dimension edit uses the signed chart frames to apply the corresponding
edge operation to every authored color/relief raster that shares that world axis.
There is no isolated chart resize, scaling, interpolation, or recentering.

Every document edit and
orbit change reruns the same inverse-warp compositor used by model viewing, so the
dominant model viewport is always derived from the current source sprites.

## Decisive validation

- Alpha decoding and normalized tent interpolation are exact at encoded samples
  and component boundaries.
- Bounds and maximum inward depth are enforced by model construction, package
  loading, PNG import, editing, resizing, and saving.
- Explicit opposite charts at maximum inward depth meet exactly at the midplane.
- A single-side source resolves only its primary side; enabling its opposite
  assignment resolves both sides from one stored PNG and survives edit/save/reopen.
- Direct opposite reuse keeps unchanged raster coordinates, while mirrored
  opposite reuse reverses `u` for Front/Back and Left/Right and reverses `v` for
  Top/Bottom. Both modes remain distinct through edit, undo, save, and reopen.
- All 24 chart-edge operations preserve signed-world-edge registration across
  every affected chart.
- Inverting `W` recovers the source point as a function of relief.
- A normalized-tent fold with three source preimages retains all three.
- A tangent fold retains its repeated preimage.
- Exact source-cell ties choose the lowest nearest texel.
- Transient depth chooses the correct source when relief preimages overlap.
- An individual observation produces no pixels from its reverse side.
- The two-source bowl resolves mirrored Front+Back and Top only, shows matching
  world-directed lighting across its exterior pair, and shows its front rim and
  recessed top basin without a Bottom surface.
- The editor preview, save, and reopen path renders through this compositor.
