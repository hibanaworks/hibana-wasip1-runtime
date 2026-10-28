#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-1}
export RUST_TEST_THREADS=${RUST_TEST_THREADS:-1}

rust_check() (
    check_target=$(mktemp -d "${TMPDIR:-/tmp}/wasi-runtime-check.XXXXXX")
    trap 'rm -rf "$check_target"' EXIT HUP INT TERM
    export CARGO_TARGET_DIR="$check_target"
    "$@"
)

cargo +stable fmt --check
rustfmt +stable --check --edition 2024 scripts/check_poll.rs scripts/check_pico2.rs scripts/check_package.rs scripts/fixtures/pico2_resources.rs
bash scripts/check_poll.sh
bash scripts/check_pico2.sh
bash scripts/check_wasi_shell_demo.sh
rust_check cargo +stable doc --locked --no-deps
rust_check bash scripts/check_miri.sh
bash scripts/check_package.sh
