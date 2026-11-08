#!/usr/bin/env bash
# Render the fixed UI review set headless: every review scene at small, laptop and large window
# sizes, light and dark, with the macOS traffic lights painted in (`--chrome`) so overlap shows.
# Usage: scripts/render-review.sh [out-dir]   (default target/review)
set -euo pipefail
cd "$(dirname "$0")/.."
out=${1:-target/review}
cargo build -q -p argo-ui --release
mkdir -p "$out"
scenes=(fixture fixture-hover protocol protocol-hover interp-collapsed dataflow-select branches project-open)
sizes=(1024x640 1440x900 2560x1440)
for scene in "${scenes[@]}"; do
  for size in "${sizes[@]}"; do
    for theme in light dark; do
      # --at 1: let the first layout report the window size and the view settle.
      target/release/argo-ui --snap "$out/$scene-$size-$theme.png" --scene "$scene" --theme "$theme" \
        --size "$size" --scale 1 --chrome --at 1 >/dev/null 2>&1 || echo "failed: $scene $size $theme" >&2
    done
  done
done
ls "$out" | wc -l
