# Oriented relief-sprite mathematical specification

## Authority

An `AuthoredModel` is a bundle of one to six oriented RGBA images. Those images are
the complete authored model. Rendering directly resamples them into a target view
to produce a pseudo-3D impression. The only persistent spatial information is each
image's unique canonical side and shared integer bounds `(width, height, depth)`.
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

These signed frames are the authority for registration, fallback orientation, and
synchronized edge resizing. Bounds register image planes; they do not assert that
the enclosed box is occupied. Each chart remains an independent two-dimensional
source domain.

## Authored and resolved charts

Authored charts are the PNG evidence stored in the model. `AuthoredModel::resolve`
produces a `ResolvedCharts` observation set in canonical rank order. For each side,
the explicit authored chart wins. If that side is absent and its opposite is
authored, the opposite RGBA is observed through the requested side's canonical
frame without flipping or rewriting its pixels. If neither side is authored, that
axis contributes no observation.

`ResolvedCharts` has read-only iteration and is the only chart collection accepted
by the renderer. Resolution never adds authored charts to the document or package.
An explicit opposite replaces only its own derived observation.

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

Rendering is output-first. For every output pixel center `s` and eligible chart,
the renderer solves `W(p) = s` and retains every solution.

Write the nonhomogeneous part of `H` as an invertible `2 × 2` matrix `A` and a
translation `b`. For a fixed output sample, inversion of the flat transform gives
a source line parameterized by relief:

```text
p(h) = A^-1 (s - b - e h)
     = p0 + d h
```

The chart-specific range `0 <= h <= h_max(i)` and intersections of `p(h)` with
source cell boundaries and tent-kernel break lines divide the search into analytic
intervals. On each interval, `N(p(h))` and `D(p(h))` are polynomials of degree at
most two. Every preimage is therefore a real root in the interval of:

```text
Q(h) = h D(p(h)) - N(p(h))
```

`Q` has degree at most three. The solver retains ordinary roots, repeated roots at
fold tangencies, and roots on interval endpoints. A root on a shared source-cell
boundary is evaluated against every foreground cell closure that owns that point.
Duplicate roots are removed only within the same source texel; distinct roots in
one texel remain distinct preimages. If `Q` is identically zero on an interval,
`Z(p(h))` is linear there, so the interval endpoint with minimum depth is sufficient
for that texel's compositing candidate.

The finite interval for chart `i` is `0 <= h <= h_max(i)`. The prototype
reconstructs each interval polynomial at fixed nodes, partitions it
at derivative roots, and bisects every sign-changing monotone span. Critical points
are tested directly so tangent roots are retained. Resolved relief is quantized to
`1 / 2^24` of an eighth-pixel before rational depth comparison. This numerical
precision rule affects only root representation; it does not change the warp or
replace it with another rendering representation.

## Color ownership and compositing

Each retained preimage produces the key:

```text
(Z, chart_rank, source_y, source_x)
```

The lexicographically smallest key owns the output pixel. The owning source texel
supplies RGB unchanged. At an exact source-cell edge or corner, the stable source
coordinates select the lowest nearest texel. An output pixel with no preimage is
transparent black.

This rule preserves multiple preimages under folds, lets the nearer portion of a
relief image occlude a farther portion, makes overlap independent of chart loading
order, and prevents camera motion from inventing or blending colors.

Before inverse sampling, a resolved chart is eligible only when its canonical
inward normal faces the camera. It contributes no candidates from the opposite
hemisphere and is also culled when exactly edge-on. A fallback observation is
front-facing through its requested canonical side; it is not the backside of its
source chart. Regions without an eligible resolved observation remain transparent.

## Two-sprite rounded bowl

The reference bowl uses a top chart and a front chart. In the top chart, the rim
has low inward relief, the inner wall increases continuously, and the basin floor
has the greatest inward relief. The front chart supplies the rounded exterior and
near rim.

In an elevated oblique view, the top chart's relief warp moves the basin farther
than the rim. Exhaustive preimage sampling retains the fold around the near rim,
and transient depth lets that rim hide the farther basin where appropriate. The
front chart supplies the exterior pixels. The result is a rounded, recessed bowl
made from two transformed images.

## File and editor integration

A `.depthsprite` is one ZIP file containing `manifest.json` and the declared
canonical PNGs under `views/`. The manifest records format version, integer bounds,
and present sides. The PNG contents remain the model authority.

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
- All 24 chart-edge operations preserve signed-world-edge registration across
  every affected chart.
- Inverting `W` recovers the source point as a function of relief.
- A normalized-tent fold with three source preimages retains all three.
- A tangent fold retains its repeated preimage.
- Exact source-cell ties choose the lowest nearest texel.
- Transient depth chooses the correct source when relief preimages overlap.
- An individual chart produces no pixels from behind, while its resolved opposite
  observation remains visible from the opposite hemisphere.
- The two-chart bowl shows both its front rim and recessed top basin.
- The editor preview, save, and reopen path renders through this compositor.
