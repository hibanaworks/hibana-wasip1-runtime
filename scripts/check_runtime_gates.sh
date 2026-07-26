#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-1}
export RUST_TEST_THREADS=${RUST_TEST_THREADS:-1}

cargo fmt --check
bash scripts/check_runtime_residue.sh
cargo check --locked --all-targets
cargo check --locked --lib --target thumbv6m-none-eabi
cargo test --locked --lib
cargo check --locked --example sequenced_choreofs_write
cargo check --locked --example direct_choreofs_write_rejection
bash scripts/check_wasi_shell_demo.sh
cargo clippy --locked --all-targets -- -D warnings
cargo doc --locked --no-deps
bash scripts/check_miri.sh
cargo package --locked --allow-dirty
scripts/check_runtime_residue.sh
