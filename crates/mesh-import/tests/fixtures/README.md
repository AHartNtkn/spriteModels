# Fixtures: real-model provenance

Fixture provenance (verified 2026-07-17):

| Fixture | Source | License/provenance |
| --- | --- | --- |
| teapot.glb | `https://raw.githubusercontent.com/alecjacobson/common-3d-test-models/master/data/teapot.obj` | Martin Newell's Utah teapot; repo gives source attribution, no formal license |
| stanford-bunny.glb | `https://raw.githubusercontent.com/alecjacobson/common-3d-test-models/master/data/stanford-bunny.obj` | Stanford 3D Scanning Repository scan |
| xyzrgb_dragon.glb | `https://raw.githubusercontent.com/alecjacobson/common-3d-test-models/master/data/xyzrgb_dragon.obj` | Stanford 3D Scanning Repository scan (repo-decimated ~10 MB OBJ) |
| earth.glb | `https://assets.science.nasa.gov/content/dam/science/psd/solar/2023/09/e/Earth_1_12756.glb` (12.32 MB) | NASA VTAD; NASA media usage guidelines |

`teapot.glb`, `stanford-bunny.glb`, and `xyzrgb_dragon.glb` were produced by downloading the
corresponding `.obj` file from the source above and converting with:

```bash
npx --yes obj2gltf -i model.obj -o model.glb
```

using `obj2gltf` (CesiumGS, Apache-2.0), version **3.2.0** (`npx --yes obj2gltf --version`).

`earth.glb` is used as downloaded, with no conversion step.

## Test premise correction

None. All `real_models.rs` assertions passed against the fixtures as provisioned; no geometric
premise about any fixture (including the earth sphere/cloud-layer assumption) needed correction.
