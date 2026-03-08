#!/usr/bin/env bash
set -euo pipefail

PORT="${1:-3000}"
ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_REL="examples/web-simple-wasm"

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo is required" >&2
  exit 1
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "error: python3 is required" >&2
  exit 1
fi

fingerprint() {
  (
    cd "$ROOT_DIR"
    if [ -d src ]; then
      find src public -type f -print0 2>/dev/null | sort -z | xargs -0 sha256sum 2>/dev/null || true
    fi
    [ -f forai.json ] && sha256sum forai.json
  ) | sha256sum | awk '{print $1}'
}

build_once() {
  echo "[dev] building $PROJECT_REL"
  (
    cd "$ROOT_DIR/../.."
    cargo run -p forai -- build "$PROJECT_REL"
  )
}

cleanup() {
  if [ -n "${SERVER_PID:-}" ] && kill -0 "$SERVER_PID" >/dev/null 2>&1; then
    kill "$SERVER_PID" >/dev/null 2>&1 || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
}

trap cleanup EXIT INT TERM

build_once
(
  cd "$ROOT_DIR"
  ./start.sh "$PORT"
) &
SERVER_PID=$!

echo "[dev] watching $ROOT_DIR/src, $ROOT_DIR/public, $ROOT_DIR/forai.json"
echo "[dev] server: http://localhost:$PORT"

last_hash="$(fingerprint)"
while true; do
  sleep 1
  next_hash="$(fingerprint)"
  if [ "$next_hash" != "$last_hash" ]; then
    echo "[dev] change detected, rebuilding..."
    if build_once; then
      echo "[dev] rebuild complete"
      last_hash="$next_hash"
    else
      echo "[dev] rebuild failed; waiting for next change" >&2
    fi
  fi
done
