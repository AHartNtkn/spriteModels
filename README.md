# DepthSprite

DepthSprite is a Rust model-authoring project inspired by
[PixZels](https://pixel-salvaje.itch.io/pixzels). A model is a bundle of oriented
PNG sprites. RGB supplies color and inverted alpha supplies relief. The renderer
transforms and composites the sprites into an orbitable pseudo-3D image.

## Current implementation

- `.depthsprite` model packages with a manifest and one to six canonical RGBA PNG
  charts
- one program-wide relief scale of eight alpha steps per model pixel
- exact image warping, relief displacement, depth compositing, and stable overlap
  ownership
- semantic model open, save, and reopen
- a two-chart bowl with a rounded outer profile and recessed basin

Generate the example models with:

```sh
cargo run -p fixture-gen -- assets/examples
```

Run the validation suite with:

```sh
cargo test --workspace
```

See [the application specification](docs/specs/depthsprite-app.md) and
[the image-relief model](docs/specs/oriented-relief-model.md).
