#!/usr/bin/env bash
#
# check-all.sh — compile, test, and check every example project.
#
# Usage:
#   ./examples/check-all.sh           # run from repo root
#   ./examples/check-all.sh --quick   # compile + test only (skip build)
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

QUICK=false
[[ "${1:-}" == "--quick" ]] && QUICK=true

# Build the forai binary first
echo "building forai..."
cargo build --bin forai --quiet
FORAI="$REPO_ROOT/target/debug/forai"
echo ""

PASS=0
FAIL=0
SKIP=0
FAILURES=()

examples=(
  browser-demo
  cli-ports
  ffi-test
  github-stats
  pipeline
  read-docs
  unit-converter
  wasm-test
  web-simple
)

run_step() {
  local name="$1" step="$2"
  shift 2
  if "$@" >/dev/null 2>&1; then
    return 0
  else
    return 1
  fi
}

for example in "${examples[@]}"; do
  dir="examples/$example"
  if [[ ! -f "$dir/forai.json" ]]; then
    printf "  %-20s skip (no forai.json)\n" "$example"
    SKIP=$((SKIP + 1))
    continue
  fi

  # Read the main entry point from forai.json
  main=$(python3 -c "import json,sys; print(json.load(open(sys.argv[1]))['main'])" "$dir/forai.json" 2>/dev/null)
  source_path="$dir/$main"

  if [[ ! -f "$source_path" ]]; then
    printf "  %-20s FAIL (main not found: %s)\n" "$example" "$main"
    FAIL=$((FAIL + 1))
    FAILURES+=("$example: main file not found ($main)")
    continue
  fi

  # Step 1: Compile
  if ! $FORAI compile "$source_path" >/dev/null 2>&1; then
    printf "  %-20s FAIL compile\n" "$example"
    FAIL=$((FAIL + 1))
    FAILURES+=("$example: compile failed")
    # Show the error
    $FORAI compile "$source_path" 2>&1 | head -5 | sed 's/^/    /'
    continue
  fi

  # Step 2: Test
  test_output=$($FORAI test "$dir" 2>&1) || true
  if echo "$test_output" | grep -q "0 failed"; then
    test_result="ok"
  elif echo "$test_output" | grep -q "failed"; then
    printf "  %-20s FAIL test\n" "$example"
    FAIL=$((FAIL + 1))
    FAILURES+=("$example: tests failed")
    echo "$test_output" | tail -3 | sed 's/^/    /'
    continue
  else
    test_result="ok"
  fi

  # Step 3: Check (format + semantic validation)
  if ! $FORAI check "$dir" >/dev/null 2>&1; then
    printf "  %-20s FAIL check\n" "$example"
    FAIL=$((FAIL + 1))
    FAILURES+=("$example: check failed")
    $FORAI check "$dir" 2>&1 | head -5 | sed 's/^/    /'
    continue
  fi

  printf "  %-20s ok\n" "$example"
  PASS=$((PASS + 1))
done

# Summary
echo ""
total=$((PASS + FAIL + SKIP))
echo "$total examples: $PASS passed, $FAIL failed, $SKIP skipped"

if [[ ${#FAILURES[@]} -gt 0 ]]; then
  echo ""
  echo "failures:"
  for f in "${FAILURES[@]}"; do
    echo "  - $f"
  done
  exit 1
fi
