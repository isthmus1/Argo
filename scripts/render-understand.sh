#!/usr/bin/env bash
# Render the understandability-test graphs into the given directory as panned tiles:
# <graph>-r<row>c<col>.png, neighbouring tiles overlapping by under a node. ZOOM (default 1.0)
# sets the capture zoom; graphs are a (compute), b (walk in regions.c), c (ram_read), each with
# every construct opened, from a sample project served through SpacetimeDB (see sample-db.sh).
# PORT (default 3122) is the sample database's port.
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/sample-db.sh
out=${1:?output directory}
zoom=${ZOOM:-1.0}
mkdir -p "$out"
sample_db understand "${PORT:-3122}"

# The tile grid and emptiness of the last fit: argo-ui fits once per function it shows, and
# the function asked for is shown last.
last_tiles() { rg '^tiles |^tile empty' | awk '/^tiles /{t=$2; e=0} /^tile empty/{e=1} END{print t, e}'; }

# tiles NAME FUNCTION: capture every tile of one function's graph.
tiles() {
    local name=$1 function=$2
    local grid empty
    read -r grid empty < <(ARGO_FUNCTION=$function ARGO_OPEN=all ARGO_TILE=0,0 ARGO_ZOOM="$zoom" \
        "$UI" --snap "$out/$name-r0c0.png" --theme dark --scale 2 2>&1 >/dev/null | last_tiles)
    [[ $grid == *x* ]] || { echo "$name: no tile grid from argo-ui" >&2; exit 1; }
    local cols=${grid%x*} rows=${grid#*x}
    for ((r = 0; r < rows; r++)); do
        for ((c = 0; c < cols; c++)); do
            ((r == 0 && c == 0)) && continue
            read -r grid empty < <(ARGO_FUNCTION=$function ARGO_OPEN=all ARGO_TILE="$c,$r" ARGO_ZOOM="$zoom" \
                "$UI" --snap "$out/$name-r${r}c${c}.png" --theme dark --scale 2 2>&1 >/dev/null | last_tiles)
            # A tile showing only empty canvas is dropped, as a person panning would skip it.
            if ((empty)); then
                trash "$out/$name-r${r}c${c}.png"
            fi
        done
    done
    echo "$name: ${cols}x${rows}"
}
tiles a compute
tiles b walk
tiles c ram_read
