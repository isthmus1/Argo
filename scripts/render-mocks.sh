#!/usr/bin/env bash
# Render the argo-ui mock headless, light and dark, into target/mocks/.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build -p argo-ui --release
mkdir -p target/mocks
for theme in dark light; do
  target/release/argo-ui --snap "target/mocks/argo-$theme.png" --theme "$theme" --scale 2 >/dev/null
done
ls target/mocks/argo-*.png
