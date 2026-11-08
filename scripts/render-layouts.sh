#!/usr/bin/env bash
# Render every layout scene headless (dark) into the given directory.
set -euo pipefail
cd "$(dirname "$0")/.."
out=${1:?output directory}
cargo build -q -p argo-ui --release
mkdir -p "$out"
for scene in fixture branches dataflow devices interp; do
  target/release/argo-ui --snap "$out/$scene.png" --scene "$scene" --theme dark --scale 2 >/dev/null
done
