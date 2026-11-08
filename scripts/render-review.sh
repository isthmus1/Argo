#!/usr/bin/env bash
# Render the fixed UI review set headless: every review capture at small, laptop and large window
# sizes, light and dark, with the macOS traffic lights painted in (`--chrome`) so overlap shows,
# from a sample project served through SpacetimeDB (see sample-db.sh). PORT (default 3123) is the
# sample database's port.
# Usage: scripts/render-review.sh [out-dir]   (default target/review)
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/sample-db.sh
out=${1:-target/review}
mkdir -p "$out"
sample_db review "${PORT:-3123}"

# review NAME [ENV...]: one capture per window size and theme.
review() {
    local name=$1; shift
    for size in 1024x640 1440x900 2560x1440; do
        for theme in light dark; do
            # --at 1: let the first layout report the window size and the view settle.
            env "$@" "$UI" --snap "$out/$name-$size-$theme.png" --theme "$theme" --size "$size" \
                --scale 1 --chrome --at 1 >/dev/null 2>&1 || echo "failed: $name $size $theme" >&2
        done
    done
}
review compute ARGO_FUNCTION=compute
review compute-hover ARGO_FUNCTION=compute ARGO_OPEN=all ARGO_HOVER='acc = step(acc, i);'
review walk-open ARGO_FUNCTION=walk ARGO_OPEN=all
review ram_read ARGO_FUNCTION=ram_read ARGO_OPEN=all
ls "$out" | wc -l
