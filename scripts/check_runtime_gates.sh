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

cargo +1.95.0 fmt --check
rustfmt +1.95.0 --check --edition 2024 scripts/fixtures/pico2_resources.rs
python3 scripts/check_poll.py
python3 scripts/check_pico2.py
bash scripts/check_wasi_shell_demo.sh
rust_check cargo +1.95.0 doc --locked --no-deps
rust_check bash scripts/check_miri.sh
rust_check cargo +1.95.0 package --locked --allow-dirty
