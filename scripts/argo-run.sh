#!/usr/bin/env bash
# Run the whole local Argo stack: SpacetimeDB plus the published module and the ingest
# (argo-server), a pool of workers, and the GUI.
#
#   scripts/argo-run.sh start [options]     # server, workers, UI — all detached
#   scripts/argo-run.sh stop                # TERM everything this script started
#   scripts/argo-run.sh restart [options]
#   scripts/argo-run.sh status
#   scripts/argo-run.sh logs [-f]           # or: logs server|workers|ui
#   scripts/argo-run.sh key                 # is a key resolvable? (never prints it)
#
# Options for start/restart:
#   --jobs N              workers in the pool (default 64: one claim each)
#   --clear               drop the database and re-ingest (--fresh --delete-data); the
#                         only way past a module whose schema no longer matches
#   --port P              SpacetimeDB port (default 3100; the model proxy is P+1)
#   --repo URL|PATH       repository to ingest (default the TinyEMU checkout)
#   --model M             model to translate with (default nvidia/nemotron-3.5-lightning)
#   --model-provider SLUG pin one OpenRouter provider (default deepinfra; `any` for none)
#   --module NAME         database name (default argo)
#   --debug               use target/debug binaries
#   --no-ui               skip the GUI
#
# The key: OPENROUTER_API_KEY from the environment, else however your zsh startup files
# export it (the script asks zsh for it), else the first line of ~/.config/argo/openrouter.key.
# Nothing secret is written in this script, so it is safe to commit. argo-server holds the
# key and mints one per worker.
#
# Everything runs detached in its own session, so closing the terminal leaves the stack
# running; `stop` is how it comes down. Logs and pid files live in $ARGO_LOG_DIR
# (default /tmp/argo-logs).

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_DIR="${ARGO_LOG_DIR:-/tmp/argo-logs}"
PID_DIR="$LOG_DIR/pids"

JOBS=64
CLEAR=0
PORT=3100
MODULE=argo
REPO="https://github.com/fernandotcl/TinyEMU.git"
MODEL="nvidia/nemotron-3.5-lightning"
PROVIDER="deepinfra"
PROFILE=release
UI=1

say() { printf '%s\n' "$*"; }
die() { printf 'argo-run: %s\n' "$*" >&2; exit 1; }

usage() { awk 'NR > 1 { if (/^set -euo/) exit; sub(/^# ?/, ""); print }' "${BASH_SOURCE[0]}"; }

# The OpenRouter key, never stored in this script: the environment if it is already there,
# else what the user's zsh exports (asked in an interactive shell, so ~/.zshrc is read), else a
# key file. Only a value that looks like a key is taken, since rc files print things too.
openrouter_key() {
    if [ -n "${OPENROUTER_API_KEY:-}" ]; then
        printf '%s' "$OPENROUTER_API_KEY"
        return 0
    fi
    if command -v zsh >/dev/null; then
        local from_zsh
        from_zsh="$(zsh -ic 'print -rn -- "$OPENROUTER_API_KEY"' 2>/dev/null \
            | grep -oE 'sk-[A-Za-z0-9_-]{16,}' | tail -n 1 || true)"
        if [ -n "$from_zsh" ]; then
            printf '%s' "$from_zsh"
            return 0
        fi
    fi
    if [ -f "$HOME/.config/argo/openrouter.key" ]; then
        head -n 1 "$HOME/.config/argo/openrouter.key" | tr -d '\r\n'
    fi
}

pid_of() { cat "$PID_DIR/$1.pid" 2>/dev/null || true; }

# Start "$@" in the background, in its own session where the machine allows it (so it outlives
# the terminal as much as this script), with its output in $LOG_DIR/$name.log. Its pid goes to
# $PID_DIR/$name.pid; nothing here waits for it.
launch() {
    local name="$1"; shift
    local log="$LOG_DIR/$name.log"
    if command -v python3 >/dev/null; then
        # The kernel's setsid, which macOS has no command-line tool for.
        python3 - "$log" "$@" >"$PID_DIR/$name.pid" <<'__ARGO_DETACH__'
import os, sys
log, argv = sys.argv[1], sys.argv[2:]
pid = os.fork()
if pid == 0:
    os.setsid()
    fd = os.open(log, os.O_WRONLY | os.O_CREAT | os.O_TRUNC)
    os.dup2(fd, 1); os.dup2(fd, 2)
    os.dup2(os.open(os.devnull, os.O_RDONLY), 0)
    try:
        os.execvp(argv[0], argv)
    finally:
        os._exit(127)
print(pid)
__ARGO_DETACH__
    elif command -v setsid >/dev/null; then
        setsid "$@" >"$log" 2>&1 </dev/null &
        echo $! >"$PID_DIR/$name.pid"
        disown 2>/dev/null || true
    else
        nohup "$@" >"$log" 2>&1 </dev/null &
        echo $! >"$PID_DIR/$name.pid"
        disown 2>/dev/null || true
    fi
}

listening() { lsof -nP -iTCP:"$1" -sTCP:LISTEN >/dev/null 2>&1; }

alive() { [ -n "$(pid_of "$1")" ] && kill -0 "$(pid_of "$1")" 2>/dev/null; }

# Kill one recorded process group, waiting for it to go.
kill_one() {
    local name="$1" pid
    pid="$(pid_of "$name")"
    [ -n "$pid" ] || return 0
    kill -TERM "-$pid" 2>/dev/null || kill -TERM "$pid" 2>/dev/null || true
    local i=0
    while kill -0 "$pid" 2>/dev/null && [ "$i" -lt 15 ]; do sleep 1; i=$((i + 1)); done
    kill -0 "$pid" 2>/dev/null && kill -KILL "$pid" 2>/dev/null || true
    rm -f "$PID_DIR/$name.pid"
}

stop_stack() {
    # Workers and UI first, then the server, which owns the database host.
    kill_one ui
    kill_one workers
    kill_one server
    # Anything left from an earlier run that this script did not record.
    pkill -TERM -f 'target/(debug|release)/argo-worker' 2>/dev/null || true
    pkill -TERM -f 'target/(debug|release)/argo-ui' 2>/dev/null || true
    pkill -TERM -f 'target/(debug|release)/argo-server' 2>/dev/null || true
    sleep 1
    say "stopped"
}

optvalue() { [ "$#" -ge 2 ] || die "$1 needs a value"; }

while [ "$#" -gt 0 ]; do
    case "$1" in
        start|stop|restart|status|logs|key) CMD="$1"; shift ;;
        --jobs) optvalue "$@"; JOBS="$2"; shift 2 ;;
        --clear) CLEAR=1; shift ;;
        --port) optvalue "$@"; PORT="$2"; shift 2 ;;
        --repo) optvalue "$@"; REPO="$2"; shift 2 ;;
        --model) optvalue "$@"; MODEL="$2"; shift 2 ;;
        --model-provider) optvalue "$@"; PROVIDER="$2"; shift 2 ;;
        --module) optvalue "$@"; MODULE="$2"; shift 2 ;;
        --debug) PROFILE=debug; shift ;;
        --no-ui) UI=0; shift ;;
        -f|--follow) FOLLOW=1; shift ;;
        -h|--help) usage; exit 0 ;;
        server|workers|ui) WHICH="$1"; shift ;;
        *) die "unknown argument: $1 (try --help)" ;;
    esac
done
CMD="${CMD:-start}"

case "$CMD" in
    status)
        printf 'port %s: %s\n' "$PORT" "$(listening "$PORT" && echo listening || echo free)"
        for name in server workers ui; do
            pid="$(pid_of "$name")"
            if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
                printf '%-8s pid %s  up\n' "$name" "$pid"
            else
                printf '%-8s not running\n' "$name"
            fi
        done
        pgrep -fl 'target/(debug|release)/argo-' 2>/dev/null || true
        ;;
    logs)
        case "${WHICH:-all}" in
            server) files=("$LOG_DIR/server.log") ;;
            workers|ui) files=("$LOG_DIR/$WHICH.log") ;;
            *) files=("$LOG_DIR/server.log" "$LOG_DIR/workers.log" "$LOG_DIR/ui.log") ;;
        esac
        if [ "${FOLLOW:-0}" = 1 ]; then tail -n 40 -f "${files[@]}"; else tail -n 40 "${files[@]}"; fi
        ;;
    key)
        KEY="$(openrouter_key)"
        if [ -z "$KEY" ]; then
            say "no key: export OPENROUTER_API_KEY in ~/.zshrc, or write it to ~/.config/argo/openrouter.key"
            exit 1
        fi
        say "key resolved: ${#KEY} characters, from the environment, your zsh, or ~/.config/argo/openrouter.key"
        ;;
    stop) stop_stack ;;
    start|restart)
        [ "$CMD" = restart ] && stop_stack
        BIN="$ROOT/target/$PROFILE"
        for exe in argo-server argo-worker; do
            [ -x "$BIN/$exe" ] || die "missing $BIN/$exe — build it: cargo build --$PROFILE -p argo-server -p argo-worker -p argo-ui"
        done
        [ "$UI" = 1 ] && [ ! -x "$BIN/argo-ui" ] && die "missing $BIN/argo-ui (or pass --no-ui)"

        KEY="$(openrouter_key)"
        [ -n "$KEY" ] || die "no OpenRouter key: export OPENROUTER_API_KEY in ~/.zshrc (or this shell), or write it to ~/.config/argo/openrouter.key"

        listening "$PORT" && die "port $PORT is already in use — is a stack already up? (scripts/argo-run.sh stop)"

        mkdir -p "$PID_DIR"
        SERVER_ARGS=("$REPO" --port "$PORT" --module "$MODULE" --model "$MODEL")
        [ "$PROVIDER" != any ] && SERVER_ARGS+=(--model-provider "$PROVIDER")
        [ "$CLEAR" = 1 ] && SERVER_ARGS+=(--fresh --delete-data)

        say "server:  ingest $REPO, database $MODULE on 127.0.0.1:$PORT"
        OPENROUTER_API_KEY="$KEY" launch server "$BIN/argo-server" "${SERVER_ARGS[@]}"
        # The module is built and the repository ingested before the database serves; the port
        # opens long before that, so give the server time to say it is serving, then go on
        # regardless: a slow ingest is the workers' problem to wait out, not the script's.
        i=0
        until grep -q 'database .*; Ctrl-C to stop' "$LOG_DIR/server.log" 2>/dev/null || [ "$i" -ge 90 ]; do
            sleep 1
            i=$((i + 1))
        done
        if grep -qi 'publish failed\|manual migration' "$LOG_DIR/server.log" 2>/dev/null; then
            tail -n 6 "$LOG_DIR/server.log" >&2
            die "publish failed (a module whose schema no longer matches the data?) — rerun with --clear"
        fi
        say "         up: pid $(pid_of server), log $LOG_DIR/server.log"

        DB="ws://127.0.0.1:$PORT"
        launch workers "$BIN/argo-worker" --db "$DB" --module "$MODULE" --jobs "$JOBS"
        say "workers: $JOBS in one pool, pid $(pid_of workers), log $LOG_DIR/workers.log"

        if [ "$UI" = 1 ]; then
            launch ui "$BIN/argo-ui" --db "$DB" --module "$MODULE"
            say "ui:      pid $(pid_of ui), log $LOG_DIR/ui.log"
        fi
        say ""
        say "stop it with: $0 stop"
        ;;
esac
