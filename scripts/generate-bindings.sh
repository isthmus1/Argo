#!/usr/bin/env bash
# Regenerate argo-db-client's SpacetimeDB bindings from the argo-db module. Run after any table or
# reducer change and check the result in; the generated files are never edited by hand.
set -euo pipefail
cd "$(dirname "$0")/.."
spacetime generate --lang rust --yes --module-path argo-db --out-dir argo-db-client/src/bindings
