#!/bin/sh
# The Swift platform's checks, as .config/nplan/config.styx runs them:
#
#   swift/check.sh test        the build script's tests, the linking test's
#                              library's, and the Swift suite on macOS
#   swift/check.sh typecheck   deno check of the build script
#
# A check's command is not run by a shell, so its steps live here.
set -eu
cd "$(dirname "$0")/.."
case "${1:-}" in
test)
    node --test swift/build.test.ts
    cargo test --quiet --manifest-path swift/link-probe/Cargo.toml --target-dir target/swift/link-probe
    node swift/build.ts --test
    cd swift
    swift test
    ;;
typecheck)
    cd swift
    deno check
    ;;
*)
    echo "usage: swift/check.sh test|typecheck" >&2
    exit 2
    ;;
esac
