#!/usr/bin/env bash
# Render the understandability-test graphs into the given directory as panned tiles:
# <graph>-r<row>c<col>.png, neighbouring tiles overlapping by under a node. ZOOM (default 1.0)
# sets the capture zoom; graphs are a (compute), b (walk in regions.c), c (ram_read).
set -euo pipefail
cd "$(dirname "$0")/.."
out=${1:?output directory}
zoom=${ZOOM:-1.0}
mkdir -p "$out"
cargo build -q -p Argo -p argo-ui --release
ui=target/release/argo-ui
target/release/main --protocol-json argo/tests/fixtures/regions.c > "$out/walk.json"
target/release/main --protocol-json argo/tests/fixture.c > "$out/ram.json"

# tiles NAME SCENE [ENV...]: capture every tile of one graph.
tiles() {
    local name=$1 scene=$2; shift 2
    local grid
    grid=$(env "$@" ARGO_TILE=0,0 ARGO_ZOOM="$zoom" "$ui" --snap "$out/$name-r0c0.png" --scene "$scene" --theme dark --scale 2 2>&1 >/dev/null | rg -o 'tiles \d+x\d+' | tail -1)
    local cols=${grid#tiles }; local rows=${cols#*x}; cols=${cols%x*}
    for ((r = 0; r < rows; r++)); do
        for ((c = 0; c < cols; c++)); do
            ((r == 0 && c == 0)) && continue
            # A tile showing only empty canvas is dropped, as a person panning would skip it.
            if env "$@" ARGO_TILE="$c,$r" ARGO_ZOOM="$zoom" "$ui" --snap "$out/$name-r${r}c${c}.png" --scene "$scene" --theme dark --scale 2 2>&1 >/dev/null | rg -q 'tile empty'; then
                trash "$out/$name-r${r}c${c}.png"
            fi
        done
    done
    echo "$name: ${cols}x${rows}"
}
tiles a protocol
tiles b snapshot ARGO_SNAPSHOT="$out/walk.json" ARGO_OPEN=all
tiles c snapshot ARGO_SNAPSHOT="$out/ram.json" ARGO_FUNCTION=ram_read ARGO_OPEN=all
