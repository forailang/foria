# Linux UI Spikes

Phase 0 spikes for Linux native UI backend evaluation.

## Spikes

- `gtk4-spike/`:
  - GTK4 window
  - Label + button
  - Click updates text
- `skia-winit-spike/`:
  - `winit` window
  - `skia-safe` GPU surface
  - Mouse click toggles rendered color

## Run: GTK4 Spike

```bash
cd examples/linux-ui-spikes/gtk4-spike
cargo run
```

## Run: Skia + winit Spike

```bash
cd examples/linux-ui-spikes/skia-winit-spike
cargo run
```

## Linux Dependencies

### Arch

```bash
sudo pacman -S --needed gtk4 pkgconf clang cmake ninja python
```

### Ubuntu/Debian

```bash
sudo apt-get update
sudo apt-get install -y libgtk-4-dev pkg-config clang cmake ninja-build python3
```

### Fedora

```bash
sudo dnf install -y gtk4-devel pkgconf-pkg-config clang cmake ninja-build python3
```

Notes:

- `skia-safe` can take a long time on first build.
- Wayland/X11 backend selection is handled by the framework/toolkit.
