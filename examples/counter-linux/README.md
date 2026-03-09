# counter-linux

Counter UI example for the Linux native UI target.

## Run

```bash
cargo run -p forai -- build examples/counter-linux
./examples/counter-linux/dist/linux-ui/run-linux-ui.sh
```

## GTK backend

Set `FORAI_UI_BACKEND=gtk` before launching to run with GTK rendering.
