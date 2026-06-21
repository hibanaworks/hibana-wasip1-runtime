#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

cargo fmt --check
bash scripts/check_runtime_residue.sh
cargo check --locked --lib
cargo check --locked --lib --target thumbv6m-none-eabi
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
scripts/check_runtime_residue.sh
