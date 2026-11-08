#!/usr/bin/env bash
# Wipe what previous runs left in the database — every claim, every finished Rust row — so the
# next `scripts/argo-run.sh start` publishes an empty module and ingests the repository afresh.
#
#   scripts/argo-clean.sh              # stop the stack, drop the database for port 3100
#   scripts/argo-clean.sh --port P     # …for another port
#   scripts/argo-clean.sh --workers    # also drop the workers' clones (~/.cache/argo-worker)
#   scripts/argo-clean.sh --dry-run    # say what would go, remove nothing
#
# Why a directory and not a reducer: claims are rows in the published database, and the only
# reducer that removes them (`ingest_reset`) is owner-only — the owner is the running server's
# own identity, which the CLI cannot borrow. Deleting the server's data directory is the same
# thing from outside: the next start publishes a brand-new database with no rows at all, and the
# `.c` chunks are re-ingested from the repository. That takes about half a minute.
#
# With no ports flag it removes $ARGO_STATE_DIR/<port> (default ~/.local/state/argo-server/<port>),
# which also holds the checkout and usage.log of that stack.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT=3100
WORKERS=0
DRY=0

say() { printf '%s\n' "$*"; }
die() { printf 'argo-clean: %s\n' "$*" >&2; exit 1; }

while [ "$#" -gt 0 ]; do
    case "$1" in
        --port) [ "$#" -ge 2 ] || die "--port needs a value"; PORT="$2"; shift 2 ;;
        --workers) WORKERS=1; shift ;;
        --dry-run) DRY=1; shift ;;
        -h|--help) sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) die "unknown argument: $1 (try --help)" ;;
    esac
done

STATE_DIR="${ARGO_STATE_DIR:-$HOME/.local/state/argo-server}/$PORT"
CLONES="${ARGO_CLONE_CACHE:-$HOME/.cache/argo-worker}"

listening() { lsof -nP -iTCP:"$1" -sTCP:LISTEN >/dev/null 2>&1; }

# The stack has to be down: the database is a live process's state, and its host holds the port.
if pgrep -f 'target/(debug|release)/argo-' >/dev/null 2>&1; then
    if [ "$DRY" = 1 ]; then
        say "would stop: $(pgrep -fl 'target/(debug|release)/argo-' | cut -c1-80 | tr '\n' ' ')"
    else
        say "stopping the running stack"
        if [ -x "$ROOT/scripts/argo-run.sh" ]; then
            "$ROOT/scripts/argo-run.sh" stop >/dev/null
        else
            pkill -TERM -f 'target/(debug|release)/argo-worker' 2>/dev/null || true
            pkill -TERM -f 'target/(debug|release)/argo-ui' 2>/dev/null || true
            pkill -TERM -f 'target/(debug|release)/argo-server' 2>/dev/null || true
        fi
    fi
fi

# Wait for the server to take its SpacetimeDB child down with it and free the port.
if [ "$DRY" = 0 ]; then
    i=0
    while listening "$PORT" && [ "$i" -lt 30 ]; do
        sleep 1
        i=$((i + 1))
    done
    listening "$PORT" && die "port $PORT is still held — a SpacetimeDB host is left behind on it"
fi

drop() {
    local what="$1" path="$2"
    if [ ! -e "$path" ]; then
        say "$what: nothing there ($path)"
        return 0
    fi
    local size
    size="$(du -sh "$path" 2>/dev/null | cut -f1)"
    if [ "$DRY" = 1 ]; then
        say "$what: would remove $path ($size)"
    else
        rm -rf "$path"
        say "$what: removed $path ($size)"
    fi
}

drop "database" "$STATE_DIR"
[ "$WORKERS" = 1 ] && drop "worker clones" "$CLONES"

if [ "$DRY" = 0 ]; then
    say ""
    say "clean: the next start publishes an empty database and re-ingests the repository"
    say "       $ROOT/scripts/argo-run.sh start"
fi
