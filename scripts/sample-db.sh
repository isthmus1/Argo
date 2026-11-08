# Sourced by the render scripts: serve a small sample C project through SpacetimeDB for headless
# `argo-ui --db … --snap` captures, which is the GUI's only data source.
#
#   sample_db NAME PORT   builds argo-server and argo-ui, makes target/NAME/sample (a git repo of
#                         fixtures/compute/compute.c, argo/tests/fixtures/regions.c and
#                         argo/tests/fixture.c as ram.c: functions compute, step, normalize, walk,
#                         ram_read, ram_fill, mask_for), starts argo-server on PORT with its data
#                         under target/NAME/data, waits until it is serving, and exports ARGO_DB.
#                         The server gets SIGINT when the calling script exits.
#
# Everything stays under target/ (the system temp dir may be small or full).

sample_db() {
    local name=$1 port=$2
    local root
    root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
    local work=$root/target/$name
    local sample=$work/sample log=$work/server.log
    mkdir -p "$work/tmp"
    export TMPDIR=$work/tmp
    (cd "$root" && cargo build -q --release -p argo-server -p argo-ui)
    if [[ ! -d $sample/.git ]]; then
        mkdir -p "$sample"
        cp "$root/fixtures/compute/compute.c" "$sample/compute.c"
        cp "$root/argo/tests/fixtures/regions.c" "$sample/regions.c"
        cp "$root/argo/tests/fixture.c" "$sample/ram.c"
        # Fixed identity and dates: the same files are the same commit on every run.
        GIT_AUTHOR_DATE=2026-01-01T00:00:00Z GIT_COMMITTER_DATE=2026-01-01T00:00:00Z \
            git -C "$sample" -c init.defaultBranch=main -c user.name=argo -c user.email=argo@localhost \
            -c commit.gpgsign=false init -q
        git -C "$sample" add -A
        GIT_AUTHOR_DATE=2026-01-01T00:00:00Z GIT_COMMITTER_DATE=2026-01-01T00:00:00Z \
            git -C "$sample" -c user.name=argo -c user.email=argo@localhost -c commit.gpgsign=false \
            commit -q -m sample
    fi
    # Its own process group: a terminal Ctrl-C reaches this script, whose exit trap stops it.
    set -m
    "$root/target/release/argo-server" "$sample" --port "$port" --data-dir "$work/data" --fresh \
        >"$log" 2>&1 &
    SAMPLE_DB_PID=$!
    set +m
    trap sample_db_stop EXIT
    local waited=0
    until rg -q '^argo-server: serving' "$log"; do
        if ! kill -0 "$SAMPLE_DB_PID" 2>/dev/null; then
            echo "argo-server exited; see $log" >&2
            tail -5 "$log" >&2
            return 1
        fi
        # The first run builds the argo-db module, which takes a while.
        if ((waited++ > 900)); then
            echo "argo-server not serving after 15 minutes; see $log" >&2
            return 1
        fi
        sleep 1
    done
    export ARGO_DB=ws://127.0.0.1:$port
    UI=$root/target/release/argo-ui
}

sample_db_stop() {
    [[ -n ${SAMPLE_DB_PID:-} ]] || return 0
    kill -INT "$SAMPLE_DB_PID" 2>/dev/null || true
    wait "$SAMPLE_DB_PID" 2>/dev/null || true
    SAMPLE_DB_PID=
}
