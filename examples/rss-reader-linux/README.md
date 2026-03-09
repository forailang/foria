# rss-reader-linux

Linux-native two-pane RSS reader example for the `linux-ui` target.

## Current scope (Phase 0/1/2)

- Fetches live feeds from:
  - `https://simonwillison.net/atom/everything/`
  - `https://daringfireball.net/feeds/main`
- Left pane list + right pane detail view
- Interactive selection and source filters
- Atom and RSS parsers extract real `title`, `link`, and `summary` fields

## Build

```bash
cargo run -p forai -- build examples/rss-reader-linux
```

## Run

```bash
./examples/rss-reader-linux/dist/linux-ui/rss-reader-linux
```

Or with wrapper:

```bash
./examples/rss-reader-linux/dist/linux-ui/run-linux-ui.sh
```
