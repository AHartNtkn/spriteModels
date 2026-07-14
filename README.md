# DepthSprite

DepthSprite is a native Rust viewer and deterministic sprite-sheet exporter for oriented relief-sprite bundles. A model is a small set of canonical RGBA PNG observations registered to integer bounds. The renderer directly warps those authored images; it does not reconstruct a mesh, solid, voxel volume, occupancy field, signed-distance field, or hidden surface.

Unsupported viewing regions remain transparent. Add another authored canonical chart when a model needs more angular coverage.

## Relief encoding

Source alpha is a mask plus an exact global relief value, not output opacity:

```text
alpha = 0       => background; no chart sample
alpha in 1..255 => foreground
relief_eighths  = 255 - alpha
relief_pixels   = (255 - alpha) / 8
```

Foreground winners are emitted with alpha 255 and their original RGB. Relief is continuous only within each four-connected foreground component; it never joins separate charts or fills missing space.

## Model files

A `.depthsprite` file is a comment-free canonical ZIP32 archive containing `manifest.json` followed by one to six declared views in canonical order:

```text
manifest.json
views/front.png
views/back.png
views/left.png
views/right.png
views/top.png
views/bottom.png
```

Absent views are omitted and undeclared entries are rejected. Version 1 limits each image dimension to 512 pixels, the archive to 65 MiB, and aggregate compressed and expanded payloads to 64 MiB each. Saving normalizes JSON, PNGs, entry order, timestamps, permissions, and transparent RGB, so reopening and saving an unchanged model is byte-stable.

The bundled bowl contains only `manifest.json`, `views/front.png`, and `views/top.png`. Across the adjacent Front-sector v1 directions, its front chart provides the rounded exterior while its top chart provides the near rim and recessed basin; unobserved regions remain transparent. Exhaustive fixture tests prove the symmetric/intermediate source relief profile, while render tests prove that broad evidence survives several directions with attached colors and an honest inner gap.

## Prerequisites

- Rust 1.92.0 through `rustup`.
- Linux: a working Vulkan or OpenGL driver plus the X11/Wayland, GTK 3, and XDG Desktop Portal development/runtime packages used by `eframe`, `wgpu`, and `rfd`. On Ubuntu/Debian:

  ```sh
  sudo apt-get update
  sudo apt-get install -y build-essential pkg-config libgtk-3-dev libxkbcommon-dev libwayland-dev libx11-xcb-dev libx11-dev libxcursor-dev libxi-dev libxrandr-dev libudev-dev libgl1-mesa-dev xdg-desktop-portal xdg-desktop-portal-gtk
  ```

- macOS: current Xcode Command Line Tools and a Metal-capable system.
- Windows: current Visual Studio Build Tools with “Desktop development with C++” and a DirectX 12-capable graphics driver.

The desktop file picker uses XDG Portal on Linux. A desktop session normally starts the portal automatically; minimal window-manager sessions must provide a portal backend such as `xdg-desktop-portal-gtk`.

## Build and run

Build the complete workspace:

```sh
cargo build --workspace --release
```

Open the bundled model directly:

```sh
cargo run -p desktop-app -- assets/examples/bowl.depthsprite
```

With no path, the application opens the bundled bowl. An invalid explicit startup path is reported in the status area and the bundled bowl remains available. Use **Open…** and **Save As…** for another package.

In the viewport:

- Primary-button drag changes the oblique view.
- The mouse wheel changes integer presentation zoom from 1× through 4× without changing rendered pixels.
- **Front**, **Top**, **Side**, and **Isometric** select fixed reference views.
- The document panel reports bounds, included charts, current view, and renderer diagnostics.
- The export panel produces a transparent PNG sheet with 8 or 16 directions, integer scale 1–8, padding 0–128, and the fixed version-1 elevation.

Preview jobs use immutable document snapshots. New interaction replaces queued intermediate work and only the latest generation may reach the viewport. Export rejects concurrent work rather than silently replacing it.

Renderer diagnostics are nonfatal warnings about the evidence the authored charts provide:

- `WarpFold` means one warped source microcell reverses orientation and may contribute multiple preimages.
- `HeavyChartOverlap` means different charts compete over more than one fifth of covered output pixels.
- `InsufficientCoverage` means authored charts cover fewer than seven tenths of the registered projected region.

Related relief-bound and exact-depth color-conflict warnings are also observational. Diagnostics do not alter the model, fill gaps, or infer hidden surfaces, and export may proceed so incomplete or conflicting authored evidence remains visible.

Regenerate the committed example packages from their authoritative fixture definitions with:

```sh
cargo run -p fixture-gen -- assets/examples
```

## Validation

Run the same authoritative checks as Linux CI:

```sh
cargo run -p fixture-gen -- assets/examples
git diff --exit-code -- assets/examples
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
```

The root end-to-end tests open the bowl, verify the exact reference owners plus source-derived summaries across three adjacent v1 directions, prove a horizontally bracketed transparent gap strictly inside projected content, save and reopen a byte-identical package, and compare two independently produced 16-direction PNG sheets byte for byte.

Automated native evidence covers the same document, viewport-state, worker, and export services used by the GUI, plus release-window liveness and title under software rendering. Pointer clicks, native file dialogs, and complete interactive orbit/save/export fidelity remain a manual release check; they are not represented as synthetic GUI automation.

The detailed mathematical and serialization contract is in [the design specification](docs/superpowers/specs/2026-07-13-oriented-relief-sprite-design.md).
