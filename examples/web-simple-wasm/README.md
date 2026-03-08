# web-simple-wasm

Client-side/browser version of `web-simple` using the same `ui.*` primitives.

## Build

```bash
cargo run -p forai -- build examples/web-simple-wasm
```

## Run in Browser

```bash
cd examples/web-simple-wasm
./start.sh 3000
```

Then open `http://localhost:3000`.

## Dev Mode (Live Rebuild)

```bash
cd examples/web-simple-wasm
./dev.sh 3000
```

This runs a build once, starts the COOP/COEP server, then watches `src/`, `public/`, and `forai.json` for changes and rebuilds automatically.

## Notes

- Uses `ui.mount` for first render and `ui.update` for subsequent patches.
- Uses `ui.events` for button/nav events.
- Uses `ui.navigate` + nav events for client-side routing.
