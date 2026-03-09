# web-simple-linux

Native Linux GTK-backend version of `web-simple` using the same `ui.*` primitives.

## Build

```bash
cargo run -p forai -- build examples/web-simple-linux
```

## Run (Linux UI)

```bash
./examples/web-simple-linux/dist/linux-ui/web-simple-linux
```

Or use the wrapper:

```bash
./examples/web-simple-linux/dist/linux-ui/run-linux-ui.sh
```

The wrapper defaults `FORAI_UI_BACKEND=gtk`.

## Linux Dependencies

### Arch/Hyprland

```bash
sudo pacman -S --needed gtk4 glib2 pango cairo pkgconf
```

### Ubuntu/Debian

```bash
sudo apt-get update
sudo apt-get install -y libgtk-4-1 libgtk-4-dev pkg-config
```

## Notes

- Uses `ui.update` for rendering and `ui.events` for interaction.
- Uses `ui.current_path` + `ui.navigate` for route state parity with browser apps.
