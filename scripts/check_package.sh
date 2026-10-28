#!/bin/sh
set -eu
repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"
driver=$(mktemp -d "${TMPDIR:-/tmp}/wasi-package-driver.XXXXXX")
cleanup() {
    if [ -d "$driver" ]; then rm -r -- "$driver"; fi
}
trap cleanup EXIT HUP INT TERM
rustup run stable clippy-driver --edition 2024 --forbid long_running_const_eval \
    -D warnings scripts/check_package.rs -o "$driver/check-package"
"$driver/check-package" "$@"
