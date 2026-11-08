#!/usr/bin/env bash
# Render the three understandability-test graphs into the given directory as a.png, b.png, c.png.
set -euo pipefail
cd "$(dirname "$0")/.."
out=${1:?output directory}
mkdir -p "$out"
cargo build -q -p Argo -p argo-ui --release
ui=target/release/argo-ui
"$ui" --snap "$out/a.png" --scene protocol --theme dark --scale 2 >/dev/null 2>&1
target/release/main --protocol-json tests/fixtures/regions.c > "$out/walk.json"
ARGO_SNAPSHOT="$out/walk.json" ARGO_OPEN=all ARGO_FIT=all "$ui" --snap "$out/b.png" --scene snapshot --theme dark --scale 2 >/dev/null 2>&1
target/release/main --protocol-json tests/fixture.c > "$out/ram.json"
ARGO_SNAPSHOT="$out/ram.json" ARGO_FUNCTION=ram_read ARGO_OPEN=all ARGO_FIT=all "$ui" --snap "$out/c.png" --scene snapshot --theme dark --scale 2 >/dev/null 2>&1
ls "$out"
