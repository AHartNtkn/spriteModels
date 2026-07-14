# Oriented relief-sprite mathematical specification

## Authority

A model is a bundle of oriented RGBA images. The images are the complete authored
model. Rendering directly resamples those images into a target view to produce a
pseudo-3D impression. The only persistent spatial information is each image's
canonical side and the shared integer model bounds used to register it.

For source alpha `a`:

```text
a = 0       => outside the chart domain
a in 1..255 => foreground
h = 255 - a => inward relief in eighth-pixel units
```

`RELIEF_UNITS_PER_PIXEL` is 8 for every model. Source RGB is authored color;
alpha encodes relief rather than display opacity. A selected foreground sample is
displayed with alpha 255.

## Canonical charts

Bounds `(width, height, depth)` register up to six canonical charts:

| Chart | Image dimensions | Positive relief direction |
| --- | --- | --- |
| front, back | `width × height` | inward along model depth |
| left, right | `depth × height` | inward along model width |
| top, bottom | `width × depth` | inward along model height |

Opposing charts use mirrored signed-axis frames. Bounds register image planes; they
do not assert that the enclosed box is occupied. Each chart remains an independent
two-dimensional source domain.

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

The finite relief range is `0 <= h <= 254`. Intersections of `p(h)` with source
cell boundaries and tent-kernel break lines divide that range into analytic
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

The prototype reconstructs each interval polynomial at fixed nodes, partitions it
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

Charts are eligible only while their authored side faces the camera. Regions not
covered by an authored chart remain transparent. Additional angular coverage comes
from additional authored charts.

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

The editor changes RGB and alpha in those source images. Every document edit and
orbit change reruns the same inverse-warp compositor used by model viewing, so the
dominant model viewport is always derived from the current source sprites.

## Decisive validation

- Alpha decoding and normalized tent interpolation are exact at encoded samples
  and component boundaries.
- Inverting `W` recovers the source point as a function of relief.
- A normalized-tent fold with three source preimages retains all three.
- A tangent fold retains its repeated preimage.
- Exact source-cell ties choose the lowest nearest texel.
- Transient depth chooses the correct source when relief preimages overlap.
- The two-chart bowl shows both its front rim and recessed top basin.
- The editor preview, save, and reopen path renders through this compositor.
