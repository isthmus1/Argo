#!/usr/bin/env bash
# Render the UI review scenes headless, dark and light, into target/mocks/ (or the given directory).
set -euo pipefail
cd "$(dirname "$0")/.."
out=${1:-target/mocks}
cargo build -q -p argo-ui --release
mkdir -p "$out"
for scene in fixture fixture-hover branches-hover dataflow-select interp-collapsed devices; do
  for theme in dark light; do
    target/release/argo-ui --snap "$out/$scene-$theme.png" --scene "$scene" --theme "$theme" --scale 2 >/dev/null
  done
done
ls "$out"
