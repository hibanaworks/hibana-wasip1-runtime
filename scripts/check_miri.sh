#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

toolchain=${HIBANA_WASIP1_MIRI_TOOLCHAIN:-nightly-2026-05-28}

if ! rustup component list --toolchain "$toolchain" --installed | rg -q '^miri-'; then
    printf '%s\n' "Miri is not installed for $toolchain" >&2
    printf '%s\n' "run: rustup component add --toolchain $toolchain miri" >&2
    exit 1
fi

export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-1}
export RUST_TEST_THREADS=${RUST_TEST_THREADS:-1}
export MIRIFLAGS="${MIRIFLAGS:-} -Zmiri-strict-provenance"

cargo "+$toolchain" miri test --locked --lib
if ! command -v timeout >/dev/null 2>&1; then
    printf '%s\n' 'GNU timeout is required for the Endpoint execution checks' >&2
    exit 1
fi
timeout 180s cargo "+$toolchain" miri test --locked --example sequenced_choreofs_write
