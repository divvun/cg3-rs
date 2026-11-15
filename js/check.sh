#!/bin/sh
# The TypeScript platform's check, as .config/nplan/config.styx runs it:
#
#   js/check.sh typecheck   deno check of the package and its tests
#
# A check's command is not run by a shell, so its steps live here.
set -eu
cd "$(dirname "$0")"
case "${1:-}" in
typecheck)
    deno check
    ;;
*)
    echo "usage: js/check.sh typecheck" >&2
    exit 2
    ;;
esac
