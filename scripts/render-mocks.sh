#!/usr/bin/env bash
# Render the UI review captures headless, dark and light, into target/mocks/ (or the given
# directory), from a sample project served through SpacetimeDB (see sample-db.sh). PORT (default
# 3121) is the sample database's port.
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/sample-db.sh
out=${1:-target/mocks}
mkdir -p "$out"
sample_db mocks "${PORT:-3121}"

# shot NAME [ENV...]: one capture per theme.
shot() {
    local name=$1; shift
    for theme in dark light; do
        env "$@" "$UI" --snap "$out/$name-$theme.png" --theme "$theme" --scale 2 >/dev/null
    done
}
shot compute ARGO_FUNCTION=compute ARGO_OPEN=all
shot compute-hover ARGO_FUNCTION=compute ARGO_OPEN=all ARGO_HOVER='acc = step(acc, i);'
shot walk ARGO_FUNCTION=walk
shot walk-open ARGO_FUNCTION=walk ARGO_OPEN=all
shot ram_read ARGO_FUNCTION=ram_read ARGO_OPEN=all
ls "$out"
